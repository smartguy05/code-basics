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
use cb_core::symbols::index::SymbolIndex;
use cb_core::testing::changecov::ChangeCoverage;
use cb_core::workspace::Workspace;

#[derive(Default)]
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
    fn new(workspace: Workspace) -> Self {
        Self {
            root: workspace.root.clone(),
            workspace: Mutex::new(workspace),
            supervisor: Supervisor::default(),
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

    /// Take this slot's language-server handle out, for teardown on close.
    pub fn take_lsp(&self) -> Option<LspHandle> {
        self.lsp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
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
                    map.insert(root.clone(), Arc::new(WorkspaceSlot::new(workspace)));
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
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod state_tests;
