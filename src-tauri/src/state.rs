//! Shared application state.
//!
//! # Several workspaces are open at once; one of them is *active*
//!
//! The app holds a codebase per top-level tab. Each open codebase is a
//! [`WorkspaceSlot`] in [`AppState::workspaces`], keyed by its canonical root —
//! the root path *is* the workspace id. [`AppState::active`] names the one the
//! foreground tab is showing, and the argument-free commands (git, run, symbols,
//! the LSP queries) resolve *that* slot through [`AppState::active_slot`] and the
//! `workspace()`/`symbols()`/`lsp()` accessors below.
//!
//! Switching tabs is [`AppState::set_active`] — a pointer move that tears nothing
//! down, so a background tab's run, terminal and language server keep running.
//! Teardown happens only on [`AppState::close`].
//!
//! # Why a slot is an `Arc`, and why that makes this file simpler than it was
//!
//! A command clones its slot handle out from under a microsecond map-lock and
//! then does all its slow work — a search over sixteen thousand symbols, a
//! minutes-long test run — holding no lock on `AppState` at all. The `Arc` it
//! holds cannot be swapped or freed under it, so the elaborate
//! check-then-store-under-one-guard machinery the single-slot design needed
//! (interleave seams, a workspace→cache lock order across five mutexes) is gone:
//! each record method looks its slot up by an **explicit root** and stores into
//! that slot's one mutex. A record whose slot has since been closed is a
//! harmless no-op — the caller's `Arc` keeps the object alive and nothing reads a
//! removed slot again — so the method returns `false`/`Err` and the caller
//! surfaces it, exactly as before.
//!
//! Keying by explicit root is also *more* correct than the old equality check:
//! a background workspace's test run or symbol build finishing while another tab
//! is active now records into the workspace it actually came from, rather than
//! being refused because it is not the foreground one.
//!
//! The one remaining lock rule: never hold the `workspaces` map lock while taking
//! a slot mutex, and never take two slot mutexes at once. Every method here takes
//! the map lock only to look a slot up (or insert/remove one) and releases it
//! before touching the slot.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cb_core::inspect::InspectGraph;
use cb_core::lsp::session::LspHandle;
use cb_core::model::TestRunResult;
use cb_core::process::Supervisor;
use cb_core::pty::PtyManager;
use cb_core::running::RunningStore;
use cb_core::sql::session::SqlSessions;
use cb_core::symbols::index::SymbolIndex;
use cb_core::testing::changecov::ChangeCoverage;
use cb_core::workspace::Workspace;

pub struct AppState {
    /// Every open workspace, keyed by canonical root (the workspace id).
    workspaces: Mutex<HashMap<PathBuf, Arc<WorkspaceSlot>>>,
    /// Which open workspace the argument-free commands resolve to — the tab in
    /// the foreground. `None` only before anything is opened.
    active: Mutex<Option<PathBuf>>,
    /// The **global** supervisor, for processes that are not keyed by a
    /// workspace-relative id and so cannot collide across open workspaces: the
    /// adversarial review (a constant id) and the behavioral before/after run.
    /// Configuration runs live in the per-slot supervisor instead — see
    /// [`WorkspaceSlot::supervisor`].
    pub supervisor: Supervisor,
    /// The interactive floating terminals. A cheap-to-clone handle over its own
    /// map of PTY sessions, keyed by an id minted per open. Global and *not*
    /// per-workspace: a terminal is keyed by its own id, its cwd is chosen at
    /// open time, and nothing here is cleared by a tab switch or a rescan. The
    /// frontend scopes terminals to a tab; the backend does not need to.
    pub pty: PtyManager,
    /// The running-process registry behind the Running panel. One instance,
    /// injected into every supervisor (global and per-slot) and the PTY manager
    /// so they all record into it. Global like `pty`, because the panel spans
    /// every open codebase and orphans are not tied to one.
    pub running: RunningStore,
    /// Every in-flight SQL statement, and the handles that stop them.
    ///
    /// Global and **not** per-workspace, and cleared by nothing a tab does: a
    /// connection belongs to the connection profile, which is user-global (see
    /// [`cb_core::sql::store`]), not to whichever codebase happens to be open.
    /// A statement running against a database must survive a tab switch, a
    /// rescan and a `close` of the workspace it was started from — exactly the
    /// argument [`AppState::pty`] makes for a terminal.
    pub sql: SqlSessions,
    /// The embedded browser panels' **data**, one slot per open codebase keyed
    /// by workspace root — availability, url, title, consent and bounded logs.
    ///
    /// Only the data. The wry `WebView`s themselves are `!Send` and
    /// main-thread-affine and live in a `thread_local!` map in [`crate::browser`]
    /// keyed by the same root; this state is `std::sync::Mutex`-guarded and
    /// `set_workspace` clears caches while holding a guard, so nothing here may
    /// need the main thread or an `.await`. [`AppState::browser`] pairs one
    /// root's slot with an `AppHandle` to make the clone-cheap handle that can
    /// reach the thread owning the webview — the same reasoning as
    /// [`cb_core::lsp::session::LspHandle`]: never an `Arc<Mutex<the resource>>`.
    ///
    /// **Per-workspace**, and that is the whole of bugs 6+7: each open codebase
    /// keeps its own live page, and only the *active* one is ever visible. An OS
    /// webview composites above the DOM and a background `WorkspaceTab` is only
    /// `hidden`, so a single app-wide webview left a *visible* page painting over
    /// whichever codebase the user switched to. Consent lives inside each slot's
    /// `BrowserShared`, so a grant made in one codebase is per-root **structurally**
    /// and cannot reach an agent acting on another. The agent pipe resolves the
    /// active root per call ([`AppState::active_browser`]); [`AppState::close`]
    /// drops the closed codebase's slot.
    browser: Mutex<HashMap<PathBuf, Arc<Mutex<crate::browser::BrowserShared>>>>,

    /// The browser control pipe, while a browser panel is open.
    ///
    /// **Tied to the panel, not to the process**, which is the smaller surface:
    /// with no panel open there is no pipe on the machine at all, and *panel
    /// closed* becomes an answer a client reads out of the registry without
    /// connecting to anything. `browser_open` starts it and `browser_close`
    /// stops it, and the registry entry is rewritten on both.
    ///
    /// Ordinary data, unlike the webview it serves: a `PipeListener` is a join
    /// handle and a flag, so it is `Send + Sync` and belongs here rather than in
    /// the main thread's `thread_local!`.
    #[cfg(windows)]
    browser_pipe: Mutex<Option<crate::browser::pipe::PipeListener>>,

    /// The Roslyn/LSP control pipe — the one an agent's `mcp-roslyn` server
    /// reaches this application through.
    ///
    /// **Tied to the process, not to any panel**, unlike [`Self::browser_pipe`]:
    /// the semantic model is per-workspace but the pipe is one per application,
    /// opened once at startup and never taken down while the app runs. Every call
    /// resolves its own `--workspace` afresh through [`Self::lsp_for_root`], so a
    /// live application always carries a listener and there is no "panel closed"
    /// state to publish. Ordinary `Send + Sync` data — a join handle and a flag —
    /// exactly as `browser_pipe` is.
    #[cfg(windows)]
    roslyn_pipe: Mutex<Option<crate::roslyn::pipe::PipeListener>>,
}

impl Default for AppState {
    fn default() -> Self {
        // One registry, shared by every process-spawning handle. `new` does no
        // I/O and no orphan detection — that is an explicit startup step
        // (`running.load_orphans`) so constructing an `AppState` (including in
        // tests) has no side effect.
        let running = RunningStore::new(cb_core::running::running_path());
        Self {
            workspaces: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            supervisor: Supervisor::with_store(running.clone()),
            pty: PtyManager::with_store(running.clone()),
            running,
            sql: SqlSessions::new(),
            browser: Mutex::new(HashMap::new()),
            #[cfg(windows)]
            browser_pipe: Mutex::new(None),
            #[cfg(windows)]
            roslyn_pipe: Mutex::new(None),
        }
    }
}

/// Everything the app remembers about one open codebase.
///
/// A slot exists in the map for exactly as long as the workspace is open, so
/// `workspace` is a plain value rather than an `Option`. Every field is guarded
/// independently; nothing here takes two of these mutexes at once.
pub struct WorkspaceSlot {
    /// The canonical root — the map key, duplicated here so a slot handed out on
    /// its own knows which workspace it is.
    pub root: PathBuf,
    /// The scanned workspace (projects, configurations). Replaced wholesale by a
    /// rescan; never partially mutated.
    workspace: Mutex<Workspace>,
    /// This workspace's process table for **configuration runs** (run / test /
    /// build) and this workspace's object captures.
    ///
    /// Per-slot rather than global because a configuration id is *root-relative*
    /// — `src-Api-Api.csproj:test:debug` is identical in every checkout with
    /// that layout — so two open workspaces with the same shape would mint
    /// colliding ids in one shared table, and starting a run in one would
    /// silently evict the other's handle. Its own table per workspace removes the
    /// collision without namespacing ids, and lets `close` cancel exactly this
    /// workspace's processes.
    pub supervisor: Supervisor,
    /// Debug adapter processes, keyed exactly like ordinary configuration runs.
    /// Killing the adapter's process tree also kills the debuggee it launched.
    pub debug: DebugSessions,
    /// The most recent result per test configuration, so "re-run failed" knows
    /// which tests to name. Keyed by config id, which is unique *within* a
    /// workspace.
    last_test_run: Mutex<HashMap<String, TestRunResult>>,
    /// The most recent coverage-of-change map per test configuration, so the
    /// Changes tab can show which changed lines the last coverage run missed
    /// without re-running anything. Keyed by config id, mirroring
    /// [`WorkspaceSlot::last_test_run`].
    last_coverage: Mutex<HashMap<String, ChangeCoverage>>,
    /// The most recent object capture for this workspace. One slot: a capture is
    /// a copy of a process's memory, and holding several would hold more of it
    /// than anything needs.
    last_inspect: Mutex<Option<InspectGraph>>,
    /// The symbol index behind the command palette, built in the background.
    /// Behind an `Arc` so a search takes a handle and drops the lock.
    symbols: Mutex<Option<Arc<SymbolIndex>>>,
    /// The "still indexing" flag and its generation counter, per workspace so two
    /// workspaces indexing at once do not clear each other's flag.
    symbols_build: Arc<SymbolsBuild>,
    /// This workspace's language servers, addressed through one actor handle.
    /// Torn down on [`AppState::close`], not on a tab switch.
    lsp: Mutex<Option<LspHandle>>,
    /// Which session start for this workspace is the current one. Per-slot: a
    /// global counter would reject a legitimate start for one workspace because a
    /// start for another had bumped it.
    lsp_generation: AtomicU64,
}

impl WorkspaceSlot {
    fn new(workspace: Workspace, running: RunningStore) -> Self {
        Self {
            root: workspace.root.clone(),
            workspace: Mutex::new(workspace),
            // Tracked so this codebase's runs/builds appear in the Running panel
            // and are recoverable as orphans after a crash.
            supervisor: Supervisor::with_store(running),
            debug: DebugSessions::default(),
            last_test_run: Mutex::new(HashMap::new()),
            last_coverage: Mutex::new(HashMap::new()),
            last_inspect: Mutex::new(None),
            symbols: Mutex::new(None),
            symbols_build: Arc::new(SymbolsBuild::default()),
            lsp: Mutex::new(None),
            lsp_generation: AtomicU64::new(0),
        }
    }

    /// A clone of the scanned workspace.
    pub fn workspace(&self) -> Workspace {
        self.workspace
            .lock()
            .map(|w| w.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    /// A clone of this slot's language-server handle, if one has been published.
    ///
    /// Unlike [`Self::take_lsp`], leaves the handle in place: the Roslyn MCP pipe
    /// resolves a workspace's session per call and must not disturb it.
    pub fn lsp(&self) -> Option<LspHandle> {
        self.lsp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Take this slot's language-server handle out, for teardown on close.
    pub fn take_lsp(&self) -> Option<LspHandle> {
        self.lsp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// The live debug-adapter processes for one workspace.
#[derive(Clone, Default)]
pub struct DebugSessions {
    running: Arc<tokio::sync::Mutex<HashMap<String, u32>>>,
}

impl DebugSessions {
    pub async fn register(&self, id: &str, pid: u32) {
        self.cancel(id).await;
        self.running.lock().await.insert(id.to_string(), pid);
    }

    pub async fn finish(&self, id: &str, pid: u32) -> bool {
        let mut running = self.running.lock().await;
        if running.get(id) == Some(&pid) {
            running.remove(id);
            return true;
        }
        false
    }

    pub async fn cancel(&self, id: &str) -> bool {
        let Some(pid) = self.running.lock().await.remove(id) else {
            return false;
        };
        cb_core::process::kill_tree_async(pid).await
    }

    pub async fn ids(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
    }
}

/// The "still indexing" flag, and the counter that says whose build it is.
///
/// One per workspace slot. The counter exists because the flag is one bit shared
/// by every build *of that workspace* (a cold build, then a rescan's build), and
/// clearing it is only correct for the latest one.
#[derive(Default)]
struct SymbolsBuild {
    building: AtomicBool,
    generation: AtomicU64,
}

/// Clears a slot's [`SymbolsBuild`] flag when the build that owns it ends, however
/// it ends — an early return, a `?`, or an unwinding panic (in the dev/test
/// profile; a release build aborts and there is no palette left to disable). Only
/// the newest build of a workspace may say indexing has stopped, which is what the
/// generation compare below enforces.
pub struct SymbolsBuildGuard {
    build: Arc<SymbolsBuild>,
    generation: u64,
}

impl Drop for SymbolsBuildGuard {
    fn drop(&mut self) {
        if self.build.generation.load(Ordering::SeqCst) == self.generation {
            self.build.building.store(false, Ordering::SeqCst);
        }
    }
}

impl AppState {
    // -- The map and the active pointer -------------------------------------

    /// The workspaces map guard, poisoning recovered rather than obeyed: the
    /// critical sections are map lookups and inserts with nothing in them to
    /// panic, and a poisoned map read as unavailable would make the whole app
    /// unusable for the life of the process.
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Arc<WorkspaceSlot>>> {
        self.workspaces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn active_lock(&self) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The slot for `root`, if that workspace is open.
    pub fn slot(&self, root: &Path) -> Option<Arc<WorkspaceSlot>> {
        self.map().get(root).cloned()
    }

    /// The active workspace's slot, or an error explaining that one is required.
    pub fn active_slot(&self) -> Result<Arc<WorkspaceSlot>, String> {
        let root = self
            .active_lock()
            .clone()
            .ok_or_else(|| "no workspace is open".to_string())?;
        self.slot(&root)
            .ok_or_else(|| "no workspace is open".to_string())
    }

    /// Open a workspace (or refresh an already-open one) and make it active.
    ///
    /// This is both the open path and the rescan path: opening a new root inserts
    /// a fresh slot, and re-setting a root that is already open replaces that
    /// slot's scanned workspace **in place**, keeping its live language server,
    /// symbol index, running processes and caches — which is what makes a rescan
    /// on every configuration save cheap. It tears **nothing** down; a previous
    /// workspace is left running in its own slot for its own tab. Only
    /// [`AppState::close`] tears a workspace down.
    pub fn set_workspace(&self, workspace: Workspace) -> Result<(), String> {
        let root = workspace.root.clone();
        {
            let mut map = self.map();
            match map.get(&root) {
                Some(slot) => {
                    // Refresh the scanned workspace without disturbing anything
                    // live in the slot.
                    if let Ok(mut current) = slot.workspace.lock() {
                        *current = workspace;
                    }
                }
                None => {
                    map.insert(
                        root.clone(),
                        Arc::new(WorkspaceSlot::new(workspace, self.running.clone())),
                    );
                }
            }
        }
        *self.active_lock() = Some(root);
        Ok(())
    }

    /// Make an already-open workspace the active one. A cheap pointer move.
    pub fn set_active(&self, root: &Path) -> Result<(), String> {
        if self.map().contains_key(root) {
            *self.active_lock() = Some(root.to_path_buf());
            Ok(())
        } else {
            Err(format!("{} is not open", root.display()))
        }
    }

    /// Close a workspace: remove its slot and pick the next active one.
    ///
    /// Returns `(removed slot, new active root)`. The caller is handed the slot so
    /// it can tear down its language server and cancel its running processes off
    /// the lock — the slot owns live process trees, so this cannot be silent. When
    /// the closed workspace was the active one, the map's next entry (if any)
    /// becomes active; the frontend decides its own tab order, so any survivor is
    /// an acceptable backend default.
    pub fn close(&self, root: &Path) -> (Option<Arc<WorkspaceSlot>>, Option<PathBuf>) {
        let removed = self.map().remove(root);

        // Drop this codebase's browser data slot. The webview itself
        // (`HOST[root]`) is dropped by the frontend unmount path
        // (`BrowserPanel` cleanup → `browser_close(root)`); this is the
        // belt-and-braces for the `Send`-only data half.
        self.browser
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(root);

        let mut active = self.active_lock();
        if active.as_deref() == Some(root) {
            *active = self.map().keys().next().cloned();
        }
        let new_active = active.clone();
        drop(active);
        (removed, new_active)
    }

    /// Every open workspace, in no particular order.
    pub fn open_workspaces(&self) -> Vec<Workspace> {
        self.map().values().map(|slot| slot.workspace()).collect()
    }

    // -- Active-resolving accessors (kept signature-compatible) --------------

    /// The active workspace, or an error explaining that one is required.
    pub fn workspace(&self) -> Result<Workspace, String> {
        Ok(self.active_slot()?.workspace())
    }

    /// The active workspace, or `None` — for `current_workspace`, which reports
    /// "nothing open" as a value rather than an error.
    pub fn active_workspace_opt(&self) -> Option<Workspace> {
        self.active_slot().ok().map(|slot| slot.workspace())
    }

    pub fn workspace_root(&self) -> Result<PathBuf, String> {
        Ok(self.active_slot()?.root.clone())
    }

    /// A clone of the active workspace's per-slot supervisor.
    pub fn active_supervisor(&self) -> Result<Supervisor, String> {
        Ok(self.active_slot()?.supervisor.clone())
    }

    // -- Test runs (by explicit root) ---------------------------------------

    /// Remember a finished run's results in the slot they came from.
    ///
    /// Keyed by the explicit root the suite ran under, so a run started in one
    /// workspace and finishing after another tab is active records into the right
    /// workspace. Returns whether it was kept — `false` if that workspace has
    /// since been closed, which `run_tests` surfaces as a warning.
    pub fn record_test_run(&self, root: &Path, config_id: &str, result: TestRunResult) -> bool {
        let Some(slot) = self.slot(root) else {
            return false;
        };
        let Ok(mut runs) = slot.last_test_run.lock() else {
            return false;
        };
        runs.insert(config_id.to_string(), result);
        true
    }

    /// The last recorded result for a configuration in the active workspace.
    pub fn previous_test_run(&self, config_id: &str) -> Option<TestRunResult> {
        let slot = self.active_slot().ok()?;
        let runs = slot.last_test_run.lock().ok()?;
        runs.get(config_id).cloned()
    }

    // -- Coverage of change (by explicit root for record, active for read) --

    /// Remember a coverage-of-change map in the slot the run came from.
    ///
    /// Keyed by the explicit root, exactly like [`AppState::record_test_run`],
    /// so a run finishing after another tab is active records into the right
    /// workspace. Returns whether it was kept — `false` if that workspace has
    /// since been closed.
    pub fn record_coverage(&self, root: &Path, config_id: &str, coverage: ChangeCoverage) -> bool {
        let Some(slot) = self.slot(root) else {
            return false;
        };
        let Ok(mut store) = slot.last_coverage.lock() else {
            return false;
        };
        store.insert(config_id.to_string(), coverage);
        true
    }

    /// The last recorded coverage map for a configuration in the active
    /// workspace, or the newest one recorded if `config_id` is `None`.
    pub fn previous_coverage(&self, config_id: Option<&str>) -> Option<ChangeCoverage> {
        let slot = self.active_slot().ok()?;
        let store = slot.last_coverage.lock().ok()?;
        match config_id {
            Some(id) => store.get(id).cloned(),
            // No configuration named: hand back any one recorded map. The
            // coverage command reads the workspace's last coverage regardless
            // of which configuration produced it.
            None => store.values().next().cloned(),
        }
    }

    // -- Captures (by explicit root for record, active for read) ------------

    pub fn record_inspect(&self, root: &Path, graph: InspectGraph) -> bool {
        let Some(slot) = self.slot(root) else {
            return false;
        };
        let Ok(mut last) = slot.last_inspect.lock() else {
            return false;
        };
        *last = Some(graph);
        true
    }

    pub fn previous_inspect(&self) -> Option<InspectGraph> {
        let slot = self.active_slot().ok()?;
        let last = slot.last_inspect.lock().ok()?;
        last.clone()
    }

    pub fn clear_inspect(&self) {
        if let Ok(slot) = self.active_slot() {
            if let Ok(mut last) = slot.last_inspect.lock() {
                *last = None;
            }
        }
    }

    // -- Symbols ------------------------------------------------------------

    /// Announce that a symbol build has started for the active workspace, and
    /// hand back the token that ends it. Called by `spawn_build` synchronously at
    /// open/rescan time, when the workspace being built is the active one, so the
    /// guard binds to that workspace's flag regardless of what becomes active
    /// later. With nothing open the guard is over a throwaway flag nobody reads.
    #[must_use = "the build's flag is cleared when this guard drops; move it into the build thread"]
    pub fn begin_symbols_build(&self) -> SymbolsBuildGuard {
        let build = match self.active_slot() {
            Ok(slot) => Arc::clone(&slot.symbols_build),
            Err(_) => Arc::new(SymbolsBuild::default()),
        };
        let generation = build.generation.fetch_add(1, Ordering::SeqCst) + 1;
        build.building.store(true, Ordering::SeqCst);
        SymbolsBuildGuard { build, generation }
    }

    /// Whether the active workspace's index is being built.
    pub fn symbols_building(&self) -> bool {
        self.active_slot()
            .map(|slot| slot.symbols_build.building.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// Store a freshly built index in the slot it describes.
    ///
    /// Keyed by the index's own root, so a background build for one workspace
    /// finishing after another tab is active lands in the workspace it was built
    /// from — not the foreground one. `false` if that workspace has been closed.
    pub fn record_symbols(&self, index: SymbolIndex) -> bool {
        let Some(slot) = self.slot(&index.root) else {
            return false;
        };
        let Ok(mut store) = slot.symbols.lock() else {
            return false;
        };
        *store = Some(Arc::new(index));
        true
    }

    /// A handle on the active workspace's index, if there is one.
    pub fn symbols(&self) -> Option<Arc<SymbolIndex>> {
        let slot = self.active_slot().ok()?;
        let store = slot.symbols.lock().ok()?;
        store.clone()
    }

    /// Apply an in-place edit to `root`'s index, if that workspace is open and has
    /// one whose root matches. Returns whether it was applied.
    pub fn update_symbols(&self, root: &Path, edit: impl FnOnce(&mut SymbolIndex)) -> bool {
        let Some(slot) = self.slot(root) else {
            return false;
        };
        let Ok(mut store) = slot.symbols.lock() else {
            return false;
        };
        let Some(index) = store.as_mut() else {
            return false;
        };
        if index.root != root {
            return false;
        }
        edit(Arc::make_mut(index));
        true
    }

    // -- Language server (active-resolving) ---------------------------------

    /// Claim the generation the active workspace's next session start runs under.
    #[must_use = "the generation must be handed to `session::start`, or the session \
                  it produces can never be published"]
    pub fn begin_lsp_session(&self) -> u64 {
        match self.active_slot() {
            Ok(slot) => slot.lsp_generation.fetch_add(1, Ordering::SeqCst) + 1,
            // Nothing open: a session started against no workspace cannot be
            // published anyway (the root check below refuses it), so any number
            // will do.
            Err(_) => 0,
        }
    }

    /// Publish a freshly started session for the active workspace, unless the
    /// workspace it was started for is no longer active — or a newer start has
    /// replaced it, or a session is already published.
    ///
    /// Returns the handle **back** on refusal rather than a `bool`, because a
    /// rejected session is a running `Microsoft.CodeAnalysis.LanguageServer.exe`
    /// and its `BuildHost` children; the caller's obligation is to tear it down.
    #[must_use = "a rejected session is still running; tear it down"]
    pub fn record_lsp_session(&self, handle: LspHandle) -> Result<(), LspHandle> {
        let Ok(slot) = self.active_slot() else {
            return Err(handle);
        };
        if slot.root.as_path() != handle.root() {
            return Err(handle);
        }
        if slot.lsp_generation.load(Ordering::SeqCst) != handle.generation() {
            return Err(handle);
        }
        let mut store = slot
            .lsp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if store.is_some() {
            return Err(handle);
        }
        *store = Some(handle);
        Ok(())
    }

    /// A handle on the active workspace's session, if one has been published.
    pub fn lsp(&self) -> Option<LspHandle> {
        let slot = self.active_slot().ok()?;
        let store = slot
            .lsp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.clone()
    }

    /// A handle on **a named** workspace's session, if that workspace is open and
    /// has published one.
    ///
    /// The Roslyn MCP pipe answers by `--workspace`, not by the active pointer, so
    /// it resolves the session for the repository the agent was scoped to
    /// regardless of which window is in front. `None` means the workspace is not
    /// open, or its session is torn down (or never started for a language) — the
    /// caller turns that into the [`cb_core::roslyn::answer::RoslynRefusal::NoSession`]
    /// refusal.
    ///
    /// Tries the path as given first, then its canonical form, because the
    /// registry publishes `Workspace.root.display()` and the slot map is keyed by
    /// the canonical root — the two agree for an ordinary path but a
    /// canonicalize fallback keeps a separator or short-name difference from
    /// reading as "not open".
    pub fn lsp_for_root(&self, root: &Path) -> Option<LspHandle> {
        if let Some(handle) = self.slot(root).and_then(|slot| slot.lsp()) {
            return Some(handle);
        }
        let canonical = dunce::canonicalize(root).ok()?;
        self.slot(&canonical).and_then(|slot| slot.lsp())
    }

    /// The active workspace's root, or `None` when nothing is open.
    ///
    /// A thin clone of the active pointer, exposed for the browser host's
    /// `set_visible` gate: only the foreground codebase's page may be shown, and
    /// the host reads this to refuse a non-active root.
    pub fn active_root_pathbuf(&self) -> Option<PathBuf> {
        self.active_lock().clone()
    }

    /// The per-root browser data slot, created on first ask.
    ///
    /// Create-on-demand mirrors how the single `BrowserShared` persisted for the
    /// process life in the app-wide design: a root that has never opened a
    /// browser gets an empty (`PanelClosed`) slot, which reads correctly and
    /// costs one small allocation.
    pub fn browser_shared(&self, root: &Path) -> Arc<Mutex<crate::browser::BrowserShared>> {
        let mut map = self
            .browser
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.entry(root.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(crate::browser::BrowserShared::new())))
            .clone()
    }

    /// The **active** workspace's browser data, and the root it belongs to.
    ///
    /// **The security choke point.** It reads the active pointer and nothing
    /// else, so an agent — whose only handle is minted from here — can never be
    /// routed to a background workspace's authenticated page. When there is no
    /// active workspace, or the active one never opened a browser, it returns a
    /// transient `PanelClosed` slot (never another root's data), which the agent
    /// gate refuses with "open the panel" guidance.
    pub fn active_browser_shared(&self) -> (PathBuf, Arc<Mutex<crate::browser::BrowserShared>>) {
        let active = self.active_lock().clone();
        match active {
            Some(root) => {
                let existing = {
                    let map = self
                        .browser
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    map.get(&root).cloned()
                };
                match existing {
                    Some(shared) => (root, shared),
                    // Active workspace, browser never opened → a fresh
                    // PanelClosed slot. Deliberately NOT another root's data.
                    None => (
                        root,
                        Arc::new(Mutex::new(crate::browser::BrowserShared::new())),
                    ),
                }
            }
            // Nothing open at all → a PanelClosed placeholder; the root is
            // irrelevant because the availability gate refuses before any
            // webview is reached.
            None => (
                PathBuf::new(),
                Arc::new(Mutex::new(crate::browser::BrowserShared::new())),
            ),
        }
    }

    /// A browser handle for one specific codebase's page.
    ///
    /// Takes the `AppHandle` as an argument rather than storing one, because
    /// `AppState` is constructed **before** there is an app — `AppState::default`
    /// runs in `run()` ahead of `tauri::Builder`, and `workspace_from_args` uses
    /// it there. Storing an `Option<BrowserHandle>` filled in by `setup` would
    /// add a "the browser is not initialised yet" failure that no caller could
    /// do anything about; a command already has the `AppHandle` (as
    /// `start_debug` does), so pairing the two here costs nothing and cannot be
    /// in the wrong state.
    pub fn browser(&self, app: tauri::AppHandle, root: &Path) -> crate::browser::BrowserHandle {
        crate::browser::BrowserHandle::from_parts(
            app,
            root.to_path_buf(),
            self.browser_shared(root),
        )
    }

    /// The handle the agent pipe answers on: always the **active** root's page.
    pub fn active_browser(&self, app: tauri::AppHandle) -> crate::browser::BrowserHandle {
        let (root, shared) = self.active_browser_shared();
        crate::browser::BrowserHandle::from_parts(app, root, shared)
    }

    /// The listener currently published for the browser control pipe, if any.
    ///
    /// Read by `browser_close` to re-publish an intact listener when other
    /// workspaces still have a browser open — the pipe is per-process and must
    /// outlive every individual panel.
    #[cfg(windows)]
    pub fn browser_pipe_published(&self) -> Option<cb_core::browser::instances::Listener> {
        self.browser_pipe
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|p| p.published())
    }

    /// Hand the browser control pipe over to this state, stopping whatever was
    /// there.
    ///
    /// Replacing rather than refusing: a panel re-open after a close that did
    /// not get as far as stopping the old listener must not end up with two,
    /// and the pipe name is per-process so the second could never bind anyway.
    #[cfg(windows)]
    pub fn set_browser_pipe(&self, listener: Option<crate::browser::pipe::PipeListener>) {
        let previous = {
            let mut slot = self
                .browser_pipe
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::replace(&mut *slot, listener)
        };
        // Stopped **after** the guard is dropped: `stop` aborts a task, and
        // holding a `std::sync::Mutex` across that is the shape `set_workspace`
        // avoids for the same reason.
        if let Some(previous) = previous {
            previous.stop();
        }
    }

    /// Stop and forget the browser control pipe. Idempotent.
    #[cfg(windows)]
    pub fn clear_browser_pipe(&self) {
        self.set_browser_pipe(None);
    }

    /// The listener currently published for the Roslyn control pipe, if any.
    ///
    /// Read whenever the registry entry is republished — a workspace opening or
    /// closing changes the `workspaces` list but not the pipe, which is
    /// process-global and outlives every individual workspace.
    #[cfg(windows)]
    pub fn roslyn_pipe_published(&self) -> Option<cb_core::roslyn::instances::Listener> {
        self.roslyn_pipe
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|p| p.published())
    }

    /// Hand the Roslyn control pipe over to this state, stopping whatever was
    /// there.
    ///
    /// Replacing rather than refusing, exactly as [`Self::set_browser_pipe`]: the
    /// pipe name carries the pid so a second could never bind, and a stale
    /// listener must not be left running beside a new one.
    #[cfg(windows)]
    pub fn set_roslyn_pipe(&self, listener: Option<crate::roslyn::pipe::PipeListener>) {
        let previous = {
            let mut slot = self
                .roslyn_pipe
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::replace(&mut *slot, listener)
        };
        // Stopped after the guard is dropped: `stop` aborts a task, and holding a
        // `std::sync::Mutex` across that is the shape to avoid.
        if let Some(previous) = previous {
            previous.stop();
        }
    }

    /// Read one codebase's browser data without an `AppHandle` and without
    /// touching the main thread.
    ///
    /// Separate from [`AppState::browser`] because the *reads* — status, the
    /// console log, the network log — need no webview at all, and a command that
    /// crossed to the main thread to answer them would be a poll that can be
    /// blocked by whatever the page is doing.
    pub fn browser_data<T>(
        &self,
        root: &Path,
        read: impl FnOnce(&crate::browser::BrowserShared) -> T,
    ) -> T {
        let shared = self.browser_shared(root);
        let guard = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        read(&guard)
    }

    /// Change one codebase's browser data. Consent is the one thing that moves
    /// through here without the webview being involved at all.
    pub fn browser_data_mut<T>(
        &self,
        root: &Path,
        write: impl FnOnce(&mut crate::browser::BrowserShared) -> T,
    ) -> T {
        let shared = self.browser_shared(root);
        let mut guard = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write(&mut guard)
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod state_tests;
