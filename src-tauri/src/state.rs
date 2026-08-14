//! Shared application state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cb_core::inspect::InspectGraph;
use cb_core::lsp::session::LspHandle;
use cb_core::model::TestRunResult;
use cb_core::process::Supervisor;
use cb_core::symbols::index::SymbolIndex;
use cb_core::workspace::Workspace;

#[derive(Default)]
pub struct AppState {
    /// The currently open workspace, if any.
    pub workspace: Mutex<Option<Workspace>>,
    pub supervisor: Supervisor,
    /// The most recent result per test configuration, so "re-run failed" knows
    /// which tests to name.
    pub last_test_run: Mutex<HashMap<String, TestRunResult>>,
    /// The most recent object capture, so switching away from the tree and back
    /// does not re-read a process that may since have moved on. One slot, not a
    /// map: unlike a test run there is nothing to key it by, and a capture is a
    /// copy of somebody's process memory — holding several would be holding
    /// more of it than anything needs.
    pub last_inspect: Mutex<Option<InspectGraph>>,
    /// The symbol index behind the command palette, built in the background
    /// after a workspace is opened.
    ///
    /// Behind an `Arc` so a search can take a handle and drop the lock before
    /// it runs. The index is tens of thousands of entries on a real solution;
    /// cloning it per keystroke would be absurd, and holding the mutex for the
    /// duration of a search would block the rebuild that a save kicks off.
    pub symbols: Mutex<Option<Arc<SymbolIndex>>>,
    /// Whether a build is running, so the palette can say "still indexing"
    /// rather than "nothing here". Separate from the slot above because the two
    /// are orthogonal: a rebuild runs over a perfectly usable index.
    ///
    /// Behind an `Arc` so a build thread can own a share of it and clear it on
    /// the way out however it leaves — see [`SymbolsBuildGuard`]. The state
    /// itself is only reachable through Tauri's managed state, which lends a
    /// borrow tied to the `AppHandle` and cannot be moved into a thread.
    symbols_build: Arc<SymbolsBuild>,
    /// This workspace's language servers, addressed through one actor handle.
    ///
    /// The fifth piece of guarded state, and the only one holding *processes*
    /// rather than data: a published handle owns a
    /// `Microsoft.CodeAnalysis.LanguageServer.exe` and its `BuildHost`
    /// children. That is why [`AppState::record_lsp_session`] hands a rejected
    /// session back instead of returning `false`, and why the swap in
    /// [`AppState::set_workspace`] tears the old one down inside the guard.
    ///
    /// A `std::sync::Mutex` like the rest, which is only safe because
    /// [`LspHandle`]'s two methods used under it —
    /// [`LspHandle::request_teardown`] and [`LspHandle::status`] — are both
    /// synchronous and non-blocking by construction. Nothing here ever awaits a
    /// server.
    pub lsp: Mutex<Option<LspHandle>>,
    /// Which session start is the current one.
    ///
    /// Modelled on [`SymbolsBuild::generation`] and needed for the same reason,
    /// which the root check cannot cover: two starts for the *same* root are
    /// both legitimate (a rescan that changed nothing, two commands racing on a
    /// cold session), and the older one landing afterwards would publish a
    /// second live server tree, or replace a live one with a session whose
    /// resolution is older than the config file that has since been saved.
    lsp_generation: AtomicU64,
}

/// The "still indexing" flag, and the counter that says whose build it is.
///
/// The counter exists because the flag is one bit shared by every build, and
/// clearing it is therefore only correct for the *latest* one. Opening
/// workspace A and then B leaves two threads running against a single flag, and
/// whichever finishes first would otherwise report that indexing had stopped
/// while the other was still walking a tree.
#[derive(Default)]
struct SymbolsBuild {
    building: AtomicBool,
    /// Bumped by every [`AppState::begin_symbols_build`]. A guard clears the
    /// flag only while this still matches the generation it was handed, so a
    /// superseded build is silent rather than wrong.
    generation: AtomicU64,
}

/// Clears [`AppState::symbols_build`]'s flag when the build that owns it ends,
/// however it ends.
///
/// # Why this is a guard and not a line at the end of the build
///
/// The flag used to be cleared in [`AppState::record_symbols`], which meant
/// every other way out of a build left it set forever: a panic in the walk, a
/// poisoned mutex, or simply a rejected stale index with no newer build behind
/// it. And a stuck flag is not a cosmetic problem. The palette
/// disables its Rebuild button while a build is running, so the flag being
/// stuck true **permanently disables the one control that could have
/// recovered**, and the note reads "Indexing the workspace — no results yet."
/// for the rest of the session. The failure removes its own remedy, which is
/// what makes it worth a type rather than a second `store` on the error path.
///
/// # It is now the *only* place the flag is cleared
///
/// Recording an index no longer clears it either. That clear could not be
/// conditioned on anything, because `record_symbols` is given an index and not
/// the build behind it, and two live builds of one root therefore shared a flag
/// that the first to finish turned off for both — see
/// [`AppState::record_symbols`]. Here there is a generation to compare, so the
/// rule holds on every path a build can leave by, which is the point of putting
/// it in one place that all of them run through.
///
/// # What it does and does not survive
///
/// Honestly, per profile, because the two differ and the previous note here
/// contradicted itself:
///
/// * **Every non-panicking exit** — an early `return`, a `?`, a rejected
///   index — is covered in every profile. That is the common case and the one
///   the type is really for.
/// * **An unwinding panic** runs this `Drop` and the flag clears. That is the
///   dev and test profile, which is what `pnpm tauri dev` and `cargo test`
///   build, and it is what the test beside this file exercises.
/// * **A panic in a release build does not reach here at all.** The root
///   `Cargo.toml` sets `panic = "abort"` under `[profile.release]` (verified,
///   line 32), so the process is gone and there is no palette left to disable.
///   Nothing here is a claim to recover from that; a guard cannot outlive the
///   process it guards.
pub struct SymbolsBuildGuard {
    build: Arc<SymbolsBuild>,
    generation: u64,
}

impl Drop for SymbolsBuildGuard {
    fn drop(&mut self) {
        // Only the newest build may say that building has stopped. This is the
        // same rule `record_symbols` states for a rejected index — a newer
        // build set the flag and will clear it — expressed once, here, so that
        // it holds on the paths that never reach `record_symbols` either.
        if self.build.generation.load(Ordering::SeqCst) == self.generation {
            self.build.building.store(false, Ordering::SeqCst);
        }
    }
}

impl AppState {
    /// The open workspace, or an error explaining that one is required.
    pub fn workspace(&self) -> Result<Workspace, String> {
        self.workspace
            .lock()
            .map_err(|_| "application state is unavailable".to_string())?
            .clone()
            .ok_or_else(|| "no workspace is open".to_string())
    }

    pub fn workspace_root(&self) -> Result<PathBuf, String> {
        Ok(self.workspace()?.root)
    }

    /// Open a workspace, discarding what was remembered about a *different* one.
    ///
    /// Everything discarded below is per-workspace and none of it is keyed by
    /// one — including the language-server session, which is the one item that
    /// owns processes rather than data and so is discarded first: a
    /// capture is a copy of a process's memory, a test result names tests that
    /// need not exist in another repository, and every path in a symbol index
    /// is relative to the root it was walked from. Carrying any of them across
    /// would attribute one repository's data to another — and in the capture's
    /// case would go on holding somebody's process memory after the reason to
    /// hold it went away.
    ///
    /// Only a change of root clears them. This is also the rescan path (saving
    /// a configuration re-scans and sets the workspace again), and a rescan
    /// must not throw away the results "re-run failed" needs.
    ///
    /// # The workspace lock is held across the clearing, deliberately
    ///
    /// This used to publish the new root and *then* release the lock before
    /// clearing, which left the same shape of window [`AppState::record_symbols`]
    /// documents, running the other way round:
    ///
    /// 1. The new root is published, and the lock released.
    /// 2. A build already running against that *same* new root finishes, passes
    ///    the root check honestly, and stores its index.
    /// 3. The open resumes and clears it.
    ///
    /// The end state is the correct index thrown away and the flag left saying
    /// nothing is being built. It is self-healing today only because every
    /// caller spawns a build immediately afterwards — a property of the callers,
    /// not of this method — so the window is closed here instead: publishing the
    /// root and discarding what belonged to the old one are one step.
    ///
    /// The lock order is workspace → cache, the same order (and the only order)
    /// used by [`AppState::record_symbols`], [`AppState::update_symbols`],
    /// [`AppState::record_test_run`], [`AppState::record_inspect`] and
    /// [`AppState::record_lsp_session`]; nothing anywhere takes the symbols,
    /// capture, test-run or session lock and then reaches for the workspace, so
    /// there is no second order to deadlock against.
    ///
    /// # A poisoned cache mutex skips its clear, and that is left alone
    ///
    /// The `changed` branch below discards four things, and the *data* clears
    /// among them (symbols, capture, test runs) tolerate a poisoned mutex by
    /// doing nothing, which
    /// looks like stale data surviving an open. It is not, because every reader
    /// tolerates poisoning the same way: [`AppState::previous_test_run`],
    /// [`AppState::previous_inspect`] and [`AppState::symbols`] all return
    /// `None` on a poisoned lock, and [`AppState::record_test_run`] and
    /// [`AppState::record_inspect`] return `false`. A poisoned cache is
    /// therefore unreadable rather than wrongly readable — the failure is
    /// "no answer", which is the side of the rule this codebase wants to fail
    /// on. Only the capture keeps a caveat: its memory stays allocated for the
    /// process's life, unreachable but held.
    ///
    /// Poisoning needs a panic while one of these guards is held, and the
    /// critical sections are a map insert, an assignment and a clear. Recovering
    /// with `Mutex::clear_poison` would add a path that only ever runs after
    /// something has already gone wrong in a section with nothing in it to go
    /// wrong, so the existing contract stands.
    ///
    /// **The fourth discard is the exception, and it is not a cache.** The
    /// argument above turns on "no answer" being the safe failure, which is true
    /// of data and false of a process tree: a poisoned `lsp` mutex read as `None`
    /// would skip the teardown below, leaving the previous workspace's
    /// `Microsoft.CodeAnalysis.LanguageServer.exe` and its `BuildHost` children
    /// running *and* still in the slot — so every later request would start a
    /// session, be refused because the slot is occupied, and tear it down again,
    /// with the feature off for the life of the process and an orphaned server
    /// tree to go with it. So [`AppState::lsp_slot`] recovers the guard instead
    /// of returning nothing, exactly as `LspHandle::status` does with its own.
    pub fn set_workspace(&self, workspace: Workspace) -> Result<(), String> {
        self.set_workspace_interleaved(workspace, || {})
    }

    /// [`AppState::set_workspace`], with a seam at the instant that used to be
    /// unguarded — see [`AppState::record_symbols_interleaved`] for why the seam
    /// exists rather than a test that spawns threads and hopes.
    ///
    /// `interleave` runs after the new root has been published and before the
    /// previous workspace's caches are discarded.
    fn set_workspace_interleaved(
        &self,
        workspace: Workspace,
        interleave: impl FnOnce(),
    ) -> Result<(), String> {
        let mut current = self
            .workspace
            .lock()
            .map_err(|_| "application state is unavailable".to_string())?;

        let changed = current
            .as_ref()
            .is_none_or(|open| open.root != workspace.root);
        *current = Some(workspace);

        if changed {
            interleave();
            // First, because it is the only one of these that owns processes. A
            // language server is started with one workspace's solution and knows
            // nothing about another's; left running it would answer under the new
            // workspace's name and go on holding a solution's worth of memory for
            // a repository nobody is looking at.
            //
            // `request_teardown` only *enqueues*, so this is legal under the
            // guard and the "everything above is one atomic step" property
            // survives: the actor processes its messages in order, so nothing
            // sent after this send is ever served, and the real shutdown happens
            // off-lock afterwards. Bumping the generation alongside it stops a
            // start that is still resolving — for either root — from publishing
            // into the workspace it was not started for.
            if let Some(stale) = self.take_lsp_session() {
                stale.request_teardown();
            }
            self.lsp_generation.fetch_add(1, Ordering::SeqCst);
            self.clear_inspect();
            self.clear_symbols();
            if let Ok(mut runs) = self.last_test_run.lock() {
                runs.clear();
            }
        }
        // Released here, at the end and not before: everything above is the
        // atomic step.
        drop(current);
        Ok(())
    }

    /// Remember a finished run's results, unless they came out of a workspace
    /// that is no longer open.
    ///
    /// # Why the caller must name the root the suite ran under
    ///
    /// Because this cache is keyed by configuration id and nothing else, and a
    /// configuration id does not identify a repository.
    /// [`cb_core::workspace::project_id`] is deliberately relative — pinned by
    /// `project_ids_are_relative_so_they_survive_a_move` — so `src/Api/Api.csproj`
    /// yields `src-Api-Api.csproj:test:debug` in *every* checkout with that
    /// layout. Two clones of one service, or two services laid out
    /// conventionally, collide exactly rather than improbably.
    ///
    /// [`AppState::set_workspace`] clears this cache on a change of root, and
    /// that clear is correct; the defect was one of ordering. `run_tests` reads
    /// the workspace, runs a suite for **seconds to minutes**, and only then
    /// records — so a run started in A and finishing after B was opened
    /// re-populated the cache the open had just emptied, with A's results, under
    /// an id B also uses.
    ///
    /// And a stale panel is not the consequence. "Re-run failed" turns these
    /// case names straight into a runner filter, so the user presses it in B and
    /// is given a run that says it is re-running their failures while naming
    /// somebody else's. That is the governing rule of this codebase inverted: a
    /// confident wrong answer where no answer was available.
    ///
    /// The root cannot be re-read at the call site instead — a check made there
    /// is over before the write begins and would only move the window — so it is
    /// checked here with the workspace guard held across the insert. Same shape
    /// and same lock order (workspace → cache) as [`AppState::record_symbols`].
    ///
    /// Returns whether the result was kept. `run_tests` surfaces a `false` as a
    /// warning on the run rather than swallowing it: the run itself succeeded and
    /// its output is worth showing, but "re-run failed" will now silently have
    /// nothing to filter by, and a control that quietly does something other than
    /// what it is labelled is the failure this whole guard is about.
    pub fn record_test_run(&self, root: &Path, config_id: &str, result: TestRunResult) -> bool {
        self.record_test_run_interleaved(root, config_id, result, || {})
    }

    /// [`AppState::record_test_run`], with a seam at the instant between the
    /// root check and the insert — see [`AppState::record_symbols_interleaved`]
    /// for why the seam exists rather than a test that spawns threads and hopes.
    fn record_test_run_interleaved(
        &self,
        root: &Path,
        config_id: &str,
        result: TestRunResult,
        interleave: impl FnOnce(),
    ) -> bool {
        let Ok(open) = self.workspace.lock() else {
            return false;
        };
        if open.as_ref().map(|w| w.root.as_path()) != Some(root) {
            return false;
        }

        interleave();

        let Ok(mut runs) = self.last_test_run.lock() else {
            return false;
        };
        runs.insert(config_id.to_string(), result);
        drop(runs);
        // The workspace guard is released here and not before: everything above
        // is the atomic step.
        drop(open);
        true
    }

    pub fn previous_test_run(&self, config_id: &str) -> Option<TestRunResult> {
        self.last_test_run.lock().ok()?.get(config_id).cloned()
    }

    /// Hold a completed capture, unless it was taken under a workspace that is
    /// no longer open.
    ///
    /// The same defect as [`AppState::record_test_run`], with no key at all:
    /// `last_inspect` is a single slot, so a capture arriving late is served to
    /// whatever workspace is open by `inspect_last`, with a dump path rooted in
    /// the previous one. `inspect_capture` reads the root, spawns the sidecar,
    /// waits for it, and parses — none of which is quick, and a live capture
    /// clones a process image.
    ///
    /// [`AppState::set_workspace`] says of carrying a capture across that it
    /// "would go on holding somebody's process memory after the reason to hold
    /// it went away". This is that sentence approached from the other side: the
    /// clear ran on time, and the capture landed after it and put the memory
    /// back. Refusing the write is therefore not only about attribution — it is
    /// the same undertaking about not holding a copy of somebody's process.
    ///
    /// Returns whether it was kept. `inspect_capture` still returns the graph to
    /// the window that asked for it — that request was answered honestly and the
    /// data is the caller's own — but notes on the console that it will not
    /// survive a view switch, so a tree quietly disappearing has a stated reason.
    pub fn record_inspect(&self, root: &Path, graph: InspectGraph) -> bool {
        self.record_inspect_interleaved(root, graph, || {})
    }

    /// [`AppState::record_inspect`], with a seam at the instant between the root
    /// check and the store.
    fn record_inspect_interleaved(
        &self,
        root: &Path,
        graph: InspectGraph,
        interleave: impl FnOnce(),
    ) -> bool {
        let Ok(open) = self.workspace.lock() else {
            return false;
        };
        if open.as_ref().map(|w| w.root.as_path()) != Some(root) {
            return false;
        }

        interleave();

        let Ok(mut last) = self.last_inspect.lock() else {
            return false;
        };
        *last = Some(graph);
        drop(last);
        drop(open);
        true
    }

    pub fn previous_inspect(&self) -> Option<InspectGraph> {
        self.last_inspect.lock().ok()?.clone()
    }

    pub fn clear_inspect(&self) {
        if let Ok(mut last) = self.last_inspect.lock() {
            *last = None;
        }
    }

    /// Announce that a symbol index build has started, and hand back the token
    /// that ends it.
    ///
    /// Set by the command that spawns the build rather than by the thread
    /// itself, so that a status request made immediately after opening a
    /// workspace already says "building" instead of racing the thread's first
    /// instruction and reporting an empty workspace.
    ///
    /// The returned [`SymbolsBuildGuard`] must be moved into the build thread
    /// and kept alive for as long as the build is: dropping it is what says the
    /// build has stopped. Returning it rather than leaving the caller to clear
    /// the flag is the point — see the guard's own documentation for the
    /// failure that arrangement produced.
    #[must_use = "the flag is cleared when this guard drops, so dropping it here ends \
                  the build as far as the palette is concerned; move it into the build thread"]
    pub fn begin_symbols_build(&self) -> SymbolsBuildGuard {
        // Claim a generation first. `fetch_add` returns the previous value, so
        // this build's generation is the new one, and any guard issued earlier
        // now compares unequal and has fallen silent before the flag is set.
        let generation = self.symbols_build.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.symbols_build.building.store(true, Ordering::SeqCst);
        SymbolsBuildGuard {
            build: Arc::clone(&self.symbols_build),
            generation,
        }
    }

    /// Whether a symbol index build is running.
    pub fn symbols_building(&self) -> bool {
        self.symbols_build.building.load(Ordering::SeqCst)
    }

    /// Store a freshly built index, unless it describes a workspace that is no
    /// longer open.
    ///
    /// The guard is the reason this is a method and not two lines in the
    /// background thread. Opening workspace A and then B before A's build
    /// finishes is ordinary behaviour — the build takes the better part of a
    /// second on a real solution — and A's result arriving afterwards would be
    /// stored under B, giving a palette full of paths that resolve to nothing
    /// in the repository the user is now looking at. Comparing the root the
    /// index was built from against the one that is open is the test — the
    /// index carries its own root, so there is nothing to keep in step — but
    /// the comparison is only worth anything if nothing can change the answer
    /// between making it and acting on it.
    ///
    /// # The workspace lock is held across the store, deliberately
    ///
    /// This used to read the root, release the workspace lock, and *then* take
    /// the symbols lock, which left a window the guard was powerless in:
    ///
    /// 1. A's build finishes and reads the open root: still `A`. Lock released.
    /// 2. The UI thread opens `B`.
    /// 3. The UI thread clears the index.
    /// 4. A's build resumes and stores A's index.
    ///
    /// The end state is workspace `B` holding an index built from `A`,
    /// reporting `ready: true` — precisely the outcome the check exists to
    /// prevent, reached by passing the check honestly. A check and the action
    /// it authorises have to be one atomic step or they are not a check at all,
    /// so the workspace guard is now held until the index is stored.
    ///
    /// The lock order that makes this safe is workspace → cache, and it is the
    /// only order used anywhere. Six methods now hold two locks at once — this
    /// one, [`AppState::set_workspace`], [`AppState::update_symbols`],
    /// [`AppState::record_test_run`], [`AppState::record_inspect`] and
    /// [`AppState::record_lsp_session`] — and all six take the workspace first.
    /// That is not a claim about intent but about
    /// the whole set: those six are the *only* bodies in this file that mention
    /// two of `workspace`, `symbols`, `last_inspect`, `last_test_run` and `lsp`,
    /// and in
    /// each the `workspace.lock()` is the first statement. Every other accessor
    /// — [`AppState::workspace`], [`AppState::symbols`],
    /// [`AppState::clear_symbols`], [`AppState::previous_test_run`],
    /// [`AppState::previous_inspect`], [`AppState::clear_inspect`],
    /// [`AppState::lsp`], [`AppState::take_lsp_session`] — takes
    /// exactly one and never reaches for a second while holding it. The only
    /// direct touch of any of these mutexes outside this file is
    /// `commands::workspace::current_workspace` (`state.workspace.lock()`, the
    /// one `.lock()` in `src-tauri/src` outside this file), which takes
    /// `workspace` alone. So no site anywhere holds a cache lock and then asks
    /// for the workspace, and there is no second order to deadlock against.
    ///
    /// Returns whether it was kept, which is what the tests assert on.
    ///
    /// # This says nothing about whether indexing has stopped
    ///
    /// It used to, and that was the last place the "only the newest build may
    /// say building has stopped" rule did not hold. This function cannot uphold
    /// it: it is handed an index and not the build that produced it, so it has
    /// no generation to compare, and two live builds of the *same* root — open a
    /// workspace, let the cold build run, save a configuration, which rescans
    /// and spawns a second — are both legitimate and both pass the root check.
    /// Whichever landed first cleared the flag for both, and the palette dropped
    /// its "results may be incomplete" note, re-enabled Rebuild and stopped
    /// polling while the other build was still walking the tree.
    ///
    /// So the flag is now owned in exactly one place, [`SymbolsBuildGuard`],
    /// which does know whose build it is. Recording an index is about the index.
    pub fn record_symbols(&self, index: SymbolIndex) -> bool {
        self.record_symbols_interleaved(index, || {})
    }

    /// [`AppState::record_symbols`], with a seam at the exact instant that used
    /// to be unguarded.
    ///
    /// `interleave` runs after the root has been compared and before the index
    /// is stored, and in production it is `||{}` — the compiler inlines it
    /// away. It exists because the defect being fixed here lives *between* two
    /// statements of this function, and a race that can only be observed from
    /// inside the function cannot be driven from outside it: a test that spawns
    /// threads and hopes for the interleaving proves nothing on the run where
    /// the interleaving does not happen, and a test that sleeps to make it
    /// likely is a slower way of proving the same nothing.
    ///
    /// With the seam, the test drives the interleaving instead of waiting for
    /// it, and the property under test is stated positively: another thread
    /// reaching this point *cannot* change the workspace. See
    /// `the_workspace_cannot_be_replaced_between_the_root_check_and_the_store`.
    fn record_symbols_interleaved(&self, index: SymbolIndex, interleave: impl FnOnce()) -> bool {
        let Ok(open) = self.workspace.lock() else {
            return false;
        };
        if open.as_ref().map(|w| w.root.as_path()) != Some(index.root.as_path()) {
            return false;
        }

        interleave();

        let Ok(mut slot) = self.symbols.lock() else {
            return false;
        };
        *slot = Some(Arc::new(index));
        drop(slot);
        // The workspace guard is released here, at the end of the statement
        // list and not before: everything above is the atomic step.
        drop(open);
        true
    }

    /// A handle on the current index, if there is one.
    ///
    /// Returns the `Arc` rather than lending a reference so the caller holds
    /// the index and not the lock — a search over sixteen thousand symbols must
    /// not be run inside a mutex that a save is waiting on.
    pub fn symbols(&self) -> Option<Arc<SymbolIndex>> {
        self.symbols.lock().ok()?.clone()
    }

    /// Apply an in-place edit to the current index, if there is one.
    ///
    /// The single-file reindex a save performs is a read-modify-write, and
    /// doing it as separate `symbols()` and `record_symbols()` calls would let
    /// a background build land in between and be overwritten by an index built
    /// from what was there before. Holding the lock across the closure keeps it
    /// atomic; the closure is only ever the microsecond splice in
    /// `cb_core::symbols::index::replace_file`, never a search.
    ///
    /// `Arc::make_mut` clones only if a search is holding the same handle,
    /// which is the one case where mutating in place would be visible to
    /// somebody mid-query.
    ///
    /// # Why the caller must name the root it built the edit for
    ///
    /// Because the edit is only meaningful under that root, and by the time it
    /// arrives the slot may hold somebody else's index. A save reads the open
    /// workspace, reads the file off disk — 0.3 ms warm, unbounded on a cold or
    /// network path — and only then splices. Opening another workspace inside
    /// that window used to put the first workspace's symbols into the second
    /// workspace's index *and* delete the second's own entries for that path,
    /// because the splice ran unconditionally: the entry keyed `Program.cs`
    /// under root A simply replaced whatever was keyed `Program.cs` under root
    /// B. Symbols from a repository the user is not looking at, offered under
    /// labels that belong to it, in place of the ones that do.
    ///
    /// The root cannot be re-read at the call site instead, because a check made
    /// there is over before the mutation begins and would only move the window.
    /// So it is checked here, against both the open workspace and the index in
    /// the slot, with the workspace guard held across the edit — the same shape,
    /// and the same lock order, as [`AppState::record_symbols`].
    ///
    /// Returns whether the edit was applied. A caller that has raced is told so
    /// rather than left believing it refreshed something.
    pub fn update_symbols(&self, root: &Path, edit: impl FnOnce(&mut SymbolIndex)) -> bool {
        self.update_symbols_interleaved(root, edit, || {})
    }

    /// [`AppState::update_symbols`], with a seam at the instant between the root
    /// check and the splice — see [`AppState::record_symbols_interleaved`] for
    /// why the seam exists rather than a test that spawns threads and hopes.
    fn update_symbols_interleaved(
        &self,
        root: &Path,
        edit: impl FnOnce(&mut SymbolIndex),
        interleave: impl FnOnce(),
    ) -> bool {
        let Ok(open) = self.workspace.lock() else {
            return false;
        };
        if open.as_ref().map(|w| w.root.as_path()) != Some(root) {
            return false;
        }

        interleave();

        let Ok(mut slot) = self.symbols.lock() else {
            return false;
        };
        // The index in the slot is checked as well as the open workspace: the
        // two agree in every ordinary state, and when they do not it is because
        // a build for the newly opened root has not landed yet, which is exactly
        // when a splice keyed under the old one would be wrong.
        let Some(index) = slot.as_mut() else {
            return false;
        };
        if index.root != root {
            return false;
        }
        edit(Arc::make_mut(index));
        drop(slot);
        drop(open);
        true
    }

    pub fn clear_symbols(&self) {
        if let Ok(mut slot) = self.symbols.lock() {
            *slot = None;
        }
    }

    /// Claim the generation a session start is about to run under.
    ///
    /// Taken *before* the start rather than after it, so that a second start
    /// beginning while the first is still resolving supersedes it: the number
    /// this returns is what [`AppState::record_lsp_session`] compares against.
    #[must_use = "the generation must be handed to `session::start`, or the \
                  session it produces can never be published"]
    pub fn begin_lsp_session(&self) -> u64 {
        // `fetch_add` returns the previous value, so this start's generation is
        // the new one and every earlier claim now compares unequal.
        self.lsp_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Publish a freshly started session, unless the workspace it was started
    /// for is no longer open — or a newer start has replaced it.
    ///
    /// # Why this returns the handle back rather than `false`
    ///
    /// Because a rejected session is a **running process tree**, not inert data
    /// like a stale test result or a stale index. Every other guard in this file
    /// refuses a write and the refused value is then dropped harmlessly; here
    /// the refused value owns a `Microsoft.CodeAnalysis.LanguageServer.exe` and
    /// its `BuildHost` children, and a `bool` would leave the caller holding the
    /// only reference to something it had just been told to forget. The
    /// consequence is not a wrong answer but a leak that lasts for the life of
    /// the app, once per rejected open. So the handle comes back, and the
    /// caller's obligation is to tear it down.
    ///
    /// # Why the workspace lock is held across the publish
    ///
    /// The same reason as [`AppState::record_symbols`], over the widest window
    /// in the file. `session::start` resolves all four languages before it
    /// returns — directory reads, which are microseconds warm and unbounded on a
    /// cold or network path — and the caller read the root before that. A check
    /// made before that window and acted on after it is not a check.
    ///
    /// Lock order is workspace → cache, as everywhere else: this is the sixth
    /// body in this file to hold two of the five mutexes, and like the other
    /// five its first statement is `workspace.lock()`. Nothing anywhere takes
    /// `lsp` and then reaches for `workspace`.
    ///
    /// # Why an occupied slot is a refusal
    ///
    /// Because overwriting it would drop the incumbent handle on the floor, and
    /// the incumbent is a live server tree that nothing else holds a reference
    /// to. A slot is only ever occupied by a session for the *open* root — a
    /// change of root empties it, inside the same guard — so the incumbent is
    /// always as good an answer as the newcomer, and keeping it costs nothing
    /// while replacing it costs a leaked language server.
    #[must_use = "a rejected session is still running; tear it down"]
    pub fn record_lsp_session(&self, handle: LspHandle) -> Result<(), LspHandle> {
        self.record_lsp_session_interleaved(handle, || {})
    }

    /// [`AppState::record_lsp_session`], with a seam at the instant between the
    /// root check and the publish — see
    /// [`AppState::record_symbols_interleaved`] for why the seam exists rather
    /// than a test that spawns threads and hopes.
    fn record_lsp_session_interleaved(
        &self,
        handle: LspHandle,
        interleave: impl FnOnce(),
    ) -> Result<(), LspHandle> {
        let Ok(open) = self.workspace.lock() else {
            return Err(handle);
        };
        if open.as_ref().map(|w| w.root.as_path()) != Some(handle.root()) {
            return Err(handle);
        }
        // The root check cannot separate two starts for the same root, and both
        // are ordinary: a rescan that changed nothing, or two commands racing on
        // a cold session. Only the newest may publish.
        if self.lsp_generation.load(Ordering::SeqCst) != handle.generation() {
            return Err(handle);
        }

        interleave();

        let mut slot = self.lsp_slot();
        if slot.is_some() {
            return Err(handle);
        }
        *slot = Some(handle);
        drop(slot);
        // The workspace guard is released here and not before: everything above
        // is the atomic step.
        drop(open);
        Ok(())
    }

    /// The session slot, with a poisoned mutex recovered rather than obeyed.
    ///
    /// The one exception to this file's poisoning contract, and the reasoning is
    /// in [`AppState::set_workspace`]: "no answer" is a safe failure for data and
    /// an unsafe one for a slot that owns processes. The critical sections under
    /// this guard are an `is_some`, an assignment, a `take` and a clone, so
    /// poisoning it needs a panic in code that has nothing in it to panic — but
    /// the *consequence* if it ever happened would be a language server nothing
    /// can reach and a feature that is off until the process restarts, which is
    /// worth one `unwrap_or_else`. `LspHandle::status` treats its own mutex the
    /// same way and for the same reason.
    fn lsp_slot(&self) -> std::sync::MutexGuard<'_, Option<LspHandle>> {
        self.lsp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Remove the session from the slot and hand it to the caller to shut down.
    ///
    /// Private, and it stays private: whoever takes a session out of the slot
    /// becomes the only thing holding a language server, so the callers are the
    /// workspace swap (which tears it down immediately) and the two tests that
    /// stand in for it.
    fn take_lsp_session(&self) -> Option<LspHandle> {
        self.lsp_slot().take()
    }

    /// A handle on this workspace's session, if one has been published.
    ///
    /// Cloned out rather than lent, so a command holds the handle and not the
    /// lock: every request on it is `async`, and awaiting one inside a
    /// `std::sync::Mutex` guard is not something this file does anywhere.
    pub fn lsp(&self) -> Option<LspHandle> {
        self.lsp_slot().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use cb_core::inspect::{Caps, InspectTarget, TargetSummary};
    use cb_core::lsp::registry::Probe;
    use cb_core::model::{TestCase, TestOutcome, TestSummary};

    fn workspace_at(root: &str) -> Workspace {
        Workspace {
            root: PathBuf::from(root),
            name: "w".into(),
            projects: Vec::new(),
            configs: Vec::new(),
            solutions: Vec::new(),
            favorites: Vec::new(),
            order: Vec::new(),
        }
    }

    fn test_run() -> TestRunResult {
        TestRunResult {
            summary: Default::default(),
            cases: Vec::new(),
            duration_ms: None,
        }
    }

    /// A finished run with one failed test named after the repository it came
    /// from.
    ///
    /// The name is the point. "Re-run failed" turns exactly these `full_name`s
    /// into a runner filter, so a result recorded under the wrong root does not
    /// merely sit there — it becomes the list of tests the next run claims to
    /// be re-running.
    fn failing_run_from(root: &str) -> TestRunResult {
        let case = TestCase {
            id: format!("{root}::Failing"),
            name: "Failing".into(),
            full_name: format!("{root}::Failing"),
            suite: None,
            project: None,
            outcome: TestOutcome::Failed,
            duration_ms: None,
            message: None,
            stack_trace: None,
            stdout: None,
        };
        TestRunResult {
            summary: TestSummary::from_cases(std::slice::from_ref(&case)),
            cases: vec![case],
            duration_ms: None,
        }
    }

    fn capture() -> InspectGraph {
        capture_under("/a")
    }

    /// A capture whose dump path lies under `root`, so a misattributed graph can
    /// be recognised by the repository its dump came out of rather than by a
    /// bare `is_some()`.
    fn capture_under(root: &str) -> InspectGraph {
        let mut graph = blank_capture();
        graph.target.target = InspectTarget::Dump {
            path: PathBuf::from(format!("{root}/.code-basics/dumps/Api.exe_1_2.dmp")),
        };
        graph
    }

    fn blank_capture() -> InspectGraph {
        InspectGraph {
            session_id: "s".into(),
            snapshot_id: "snap".into(),
            captured_at: "2026-01-01T00:00:00Z".into(),
            target: TargetSummary {
                target: InspectTarget::Dump {
                    path: PathBuf::from("/a/.code-basics/dumps/Api.exe_1_2.dmp"),
                },
                bitness: None,
                runtime_version: None,
                process_name: None,
            },
            roots: Vec::new(),
            caps: Caps::default(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn opening_another_workspace_forgets_the_first_ones_capture() {
        // A capture is a copy of somebody's process memory. Showing it under an
        // unrelated repository is both a wrong attribution and a reason to be
        // still holding it.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_inspect(Path::new("/a"), capture());
        state.record_test_run(Path::new("/a"), "cfg", test_run());

        state.set_workspace(workspace_at("/b")).unwrap();

        assert!(state.previous_inspect().is_none());
        assert!(state.previous_test_run("cfg").is_none());
    }

    fn index_at(root: &str) -> SymbolIndex {
        SymbolIndex {
            root: PathBuf::from(root),
            files: vec![PathBuf::from("a.rs")],
            symbols: Vec::new(),
            truncated: false,
        }
    }

    #[test]
    fn opening_another_workspace_forgets_the_first_ones_symbol_index() {
        // Every path in an index is relative to the root it was built from.
        // Serving one repository's files and symbols under another would offer
        // rows that open nothing, under labels that belong to somebody else's
        // code.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_symbols(index_at("/a"));

        state.set_workspace(workspace_at("/b")).unwrap();

        assert!(state.symbols().is_none());
    }

    #[test]
    fn rescanning_the_same_workspace_keeps_its_index() {
        // Saving a configuration re-scans and sets the workspace again. Losing
        // the index on every save would leave the palette empty for the
        // second or so each rebuild takes, over and over.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_symbols(index_at("/a"));

        state.set_workspace(workspace_at("/a")).unwrap();

        assert!(state.symbols().is_some());
    }

    #[test]
    fn an_index_built_for_a_workspace_that_is_no_longer_open_is_not_recorded() {
        // Opening A then immediately B leaves A's background build still
        // running. It must not land under B.
        let state = AppState::default();
        state.set_workspace(workspace_at("/b")).unwrap();

        let recorded = state.record_symbols(index_at("/a"));

        assert!(!recorded);
        assert!(state.symbols().is_none());
    }

    #[test]
    fn a_stale_build_does_not_report_that_the_current_build_has_finished() {
        // The flag belongs to the build that set it, never to one that was
        // rejected: saying "not building" on A's arrival would report "finished,
        // nothing found" while B is still running and there is no index at all.
        //
        // The second half changed with the fix for two live builds of one root
        // and is stated more strongly than before, not less. Recording an index
        // no longer ends anything — the build that set the flag ends it, on its
        // way out — so the flag survives a recorded index too, and is clear only
        // once that build is actually over.
        let state = AppState::default();
        state.set_workspace(workspace_at("/b")).unwrap();
        let building = state.begin_symbols_build();

        state.record_symbols(index_at("/a"));

        assert!(state.symbols_building());

        state.record_symbols(index_at("/b"));

        assert!(
            state.symbols_building(),
            "an index landing ended a build that had not finished"
        );

        drop(building);

        assert!(!state.symbols_building());
    }

    #[test]
    fn a_rejected_stale_build_leaves_the_flag_to_the_build_that_is_still_walking() {
        // Open A, then B before A's build finishes. A's index is refused, and
        // A's build then ends — with nothing recorded and nothing to record.
        // B is still walking, so the palette must still say so; the alternative
        // is "finished, nothing found" over a workspace nobody has looked at
        // yet.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        let for_a = state.begin_symbols_build();

        state.set_workspace(workspace_at("/b")).unwrap();
        let _for_b = state.begin_symbols_build();

        assert!(!state.record_symbols(index_at("/a")));
        drop(for_a);

        assert!(
            state.symbols_building(),
            "the refused build reported that indexing had finished"
        );
    }

    #[test]
    fn the_workspace_cannot_be_replaced_between_the_root_check_and_the_store() {
        // The defect this pins is a window, not a value: the root was compared,
        // the workspace lock was released, and only then was the index stored.
        // Another thread getting in between produced workspace B holding an
        // index built from A — a palette of paths that open nothing, reporting
        // itself ready.
        //
        // The interleaving is driven rather than hoped for. `interleave` runs
        // at exactly the instant that used to be unguarded, and the thread it
        // spawns attempts the swap the way the real UI thread would: through
        // the workspace lock. If that lock is free at this moment, the race is
        // reachable; if it is held, it is not. Nothing here depends on timing,
        // and nothing sleeps.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();

        let stolen = Arc::new(AtomicBool::new(false));
        let interleave = {
            let state = Arc::clone(&state);
            let stolen = Arc::clone(&stolen);
            move || {
                std::thread::spawn(move || {
                    // A real second thread, and `try_lock` rather than `lock`
                    // so that the fixed code reports the refusal instead of
                    // deadlocking against a guard the recorder is holding.
                    if let Ok(mut open) = state.workspace.try_lock() {
                        *open = Some(workspace_at("/b"));
                        drop(open);
                        state.clear_symbols();
                        stolen.store(true, Ordering::SeqCst);
                    }
                })
                .join()
                .expect("the interleaved thread panicked");
            }
        };

        let recorded = state.record_symbols_interleaved(index_at("/a"), interleave);

        assert!(recorded);
        assert_eq!(
            state
                .symbols()
                .expect("an index should have been stored")
                .root,
            state.workspace().unwrap().root,
            "an index was stored under a workspace it was not built from \
             (the interleaved open {} the workspace mid-record)",
            if stolen.load(Ordering::SeqCst) {
                "replaced"
            } else {
                "could not replace"
            }
        );
        assert!(
            !stolen.load(Ordering::SeqCst),
            "the workspace was replaceable between the check and the store, \
             which is the whole race"
        );
    }

    /// Attempts, from a real second thread, to open workspace `root` the way the
    /// UI thread would, and reports whether it got in.
    ///
    /// `try_lock` rather than `lock`, so that code holding the workspace guard
    /// reports the refusal instead of deadlocking the test against itself: the
    /// question being asked is whether the lock is *free* at this instant, which
    /// is the whole difference between a window and no window. The thread is
    /// real, and the interleaving is driven rather than waited for — see
    /// `the_workspace_cannot_be_replaced_between_the_root_check_and_the_store`.
    ///
    /// It discards *everything* [`AppState::set_workspace`] discards on a change
    /// of root — all three caches and the language-server session — and not just
    /// the one a given test looks at. A helper that cleared less would let that
    /// test pass over a window the real open would have fallen into.
    fn steal_workspace(state: &Arc<AppState>, root: &'static str) -> Arc<AtomicBool> {
        let stolen = Arc::new(AtomicBool::new(false));
        let state = Arc::clone(state);
        let flag = Arc::clone(&stolen);
        std::thread::spawn(move || {
            if let Ok(mut open) = state.workspace.try_lock() {
                *open = Some(workspace_at(root));
                drop(open);
                state.clear_symbols();
                state.clear_inspect();
                if let Ok(mut runs) = state.last_test_run.lock() {
                    runs.clear();
                }
                if let Some(stale) = state.take_lsp_session() {
                    stale.request_teardown();
                }
                flag.store(true, Ordering::SeqCst);
            }
        })
        .join()
        .expect("the interleaved thread panicked");
        stolen
    }

    #[test]
    fn the_workspace_cannot_be_replaced_between_the_reindex_root_check_and_the_splice() {
        // The save's own root check is worth nothing if the workspace can be
        // swapped after it passes: the splice would then land in the index of a
        // repository the edited file is not in, replacing that repository's
        // entries for the same relative path.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_symbols(index_at("/a"));

        let mut stolen = None;
        let applied = state.update_symbols_interleaved(
            Path::new("/a"),
            |index| index.files.push(PathBuf::from("from-a.rs")),
            || stolen = Some(steal_workspace(&state, "/b")),
        );

        // The window first, because it is the defect; the two assertions below
        // are its consequences and would otherwise report the symptom while the
        // cause went unnamed.
        assert!(
            !stolen.unwrap().load(Ordering::SeqCst),
            "the workspace was replaceable between the check and the splice, \
             which is the whole race"
        );
        assert!(applied);
        assert_eq!(
            state.symbols().unwrap().root,
            state.workspace().unwrap().root,
            "an edit was spliced into an index built from another workspace"
        );
    }

    #[test]
    fn an_index_for_the_newly_opened_workspace_cannot_land_before_its_caches_are_cleared() {
        // Opening B published B's root and only then cleared the caches. A build
        // already running against B could pass its root check in that gap and
        // store an index that the clear then threw away — leaving no index and
        // no build to produce one. Self-healing only because today's callers
        // spawn a build immediately afterwards, which is not a property of this
        // method.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();

        let mut stolen = None;
        state
            .set_workspace_interleaved(workspace_at("/b"), || {
                stolen = Some(steal_workspace(&state, "/b"))
            })
            .unwrap();

        assert!(
            !stolen.unwrap().load(Ordering::SeqCst),
            "the workspace lock was free between publishing the new root and \
             discarding the old one's caches, so a build for the new root could \
             have stored an index into the gap"
        );
    }

    #[test]
    fn every_pair_of_these_locks_is_taken_in_one_order_under_load() {
        // Six methods now hold two of the five mutexes at once, and a second
        // order between any pair would be a deadlock rather than a wrong answer.
        // This is not proof of correctness — no stress test is — but a reversed
        // order shows up as a hang here rather than in the user's window, and
        // the ordering argument in `record_symbols` is made by reading, which
        // this backs up.
        //
        // All four of these mutexes are exercised, not just the two the original
        // version covered: the capture and test-run caches are now taken under
        // the workspace guard as well, so leaving them out would stress three of
        // the pairs and vouch for six.
        //
        // The fifth mutex, `lsp`, is *written* by
        // `the_lsp_slot_is_taken_in_the_same_order_as_every_other_cache` and not
        // here: this loop calls no `record_lsp_session` at all, because publishing
        // one needs a live session per iteration and that test is where the
        // sessions live. Here `lsp` is only read — which still earns its place,
        // since a reader that took the two in the other order would deadlock
        // against the swap below.
        let state = Arc::new(AppState::default());
        let threads: Vec<_> = (0..8)
            .map(|n| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let root = if n % 2 == 0 { "/a" } else { "/b" };
                    for _ in 0..20_000 {
                        state.set_workspace(workspace_at(root)).unwrap();
                        state.record_symbols(index_at(root));
                        state.update_symbols(Path::new(root), |index| {
                            index.files.push(PathBuf::from("x.rs"))
                        });
                        state.record_test_run(Path::new(root), "cfg", failing_run_from(root));
                        state.record_inspect(Path::new(root), capture_under(root));
                        let _ = state.symbols();
                        let _ = state.symbols_building();
                        let _ = state.previous_test_run("cfg");
                        let _ = state.previous_inspect();
                        let _ = state.lsp();
                        state.clear_symbols();
                        state.clear_inspect();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("a worker deadlocked or panicked");
        }
    }

    #[test]
    fn a_build_thread_that_panics_leaves_the_flag_clear_so_the_palette_can_be_rebuilt() {
        // The Rebuild button is disabled while the flag is set, so a build that
        // dies without clearing it disables the only control that could
        // recover, and the palette says "indexing" for the rest of the session.
        //
        // This is the unwinding profile — `cargo test` and `pnpm tauri dev`.
        // `panic = "abort"` in `[profile.release]` takes the process instead,
        // and no guard can help there; see `SymbolsBuildGuard`.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();

        let died = std::thread::spawn({
            let state = Arc::clone(&state);
            move || {
                let _building = state.begin_symbols_build();
                panic!("the walk hit something unexpected");
            }
        })
        .join();

        assert!(died.is_err(), "the thread was supposed to panic");
        assert!(!state.symbols_building());
    }

    #[test]
    fn a_build_that_ends_without_recording_anything_still_says_it_has_stopped() {
        // Not every failure is a panic. A poisoned lock, or a rejected index
        // with no newer build behind it, returns normally and records nothing —
        // and used to leave the flag set just as permanently.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        {
            let _building = state.begin_symbols_build();
            assert!(state.symbols_building());
        }

        assert!(!state.symbols_building());
    }

    #[test]
    fn a_superseded_builds_guard_leaves_the_flag_to_the_build_that_replaced_it() {
        // The behaviour `a_stale_build_does_not_report_that_the_current_build_
        // has_finished` pins, on the path that never reaches `record_symbols`:
        // A's guard dropping must not announce that indexing has finished while
        // B is still walking B's tree.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let first = state.begin_symbols_build();
        let _second = state.begin_symbols_build();

        drop(first);

        assert!(
            state.symbols_building(),
            "the older build cleared the flag out from under the newer one"
        );
    }

    #[test]
    fn a_reindex_from_a_workspace_that_is_no_longer_open_does_not_touch_the_new_ones_index() {
        // A save in workspace A reads A's root, reads the file from disk, and
        // only then splices. Opening B inside that window used to put A's
        // symbols into B's index and delete B's own entries for that path.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_symbols(index_at("/a"));

        state.set_workspace(workspace_at("/b")).unwrap();
        state.record_symbols(index_at("/b"));

        let applied = state.update_symbols(Path::new("/a"), |index| {
            index.files.push(PathBuf::from("from-a.rs"))
        });

        assert!(
            !applied,
            "the save was applied to a workspace it did not come from"
        );

        assert_eq!(
            state.symbols().unwrap().files,
            [PathBuf::from("a.rs")],
            "a save from the previous workspace was spliced into this one's index"
        );
    }

    #[test]
    fn a_build_that_lands_first_does_not_end_a_newer_build_of_the_same_workspace() {
        // Open A, start a cold build, rescan, start a second build of the same
        // root. Both are live and both are legitimate; whichever lands first
        // used to clear the one shared flag, so the palette stopped saying
        // "results may be incomplete" while the other was still walking.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let first = state.begin_symbols_build();
        let _second = state.begin_symbols_build();

        assert!(state.record_symbols(index_at("/a")));
        drop(first);

        assert!(
            state.symbols_building(),
            "the older build reported that indexing had finished while the newer one was still walking"
        );
    }

    #[test]
    fn editing_the_index_when_there_is_none_does_nothing_rather_than_creating_one() {
        // A save that arrives before the first build finishes must not
        // fabricate an index containing one file, which would report `ready`
        // and answer every other query with nothing.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let applied = state.update_symbols(Path::new("/a"), |index| {
            index.files.push(PathBuf::from("b.rs"))
        });

        assert!(!applied);
        assert!(state.symbols().is_none());
    }

    #[test]
    fn editing_the_index_is_visible_to_the_next_search() {
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_symbols(index_at("/a"));

        let applied = state.update_symbols(Path::new("/a"), |index| {
            index.files.push(PathBuf::from("b.rs"))
        });

        assert!(applied);
        assert_eq!(state.symbols().unwrap().files.len(), 2);
    }

    #[test]
    fn rescanning_the_same_workspace_keeps_what_it_captured() {
        // Saving a configuration re-scans and sets the workspace again. Losing
        // the tree, or the results "re-run failed" reads, on every save would
        // make both features look broken.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_inspect(Path::new("/a"), capture());
        state.record_test_run(Path::new("/a"), "cfg", test_run());

        state.set_workspace(workspace_at("/a")).unwrap();

        assert!(state.previous_inspect().is_some());
        assert!(state.previous_test_run("cfg").is_some());
    }

    /// Where the graph's dump came from, for an assertion that can name the
    /// repository rather than say "wrong".
    fn dump_path(graph: &InspectGraph) -> String {
        match &graph.target.target {
            InspectTarget::Dump { path } => path.display().to_string(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn a_test_run_that_finishes_after_another_workspace_is_opened_is_not_recorded_under_it() {
        // The widest window in the file: `run_tests` reads the workspace, runs a
        // suite for seconds to minutes, and only then records — keyed by config
        // id alone. `set_workspace` clears the cache correctly; the run then
        // re-populates it with the previous repository's results.
        //
        // The ids collide in practice rather than in theory. `workspace::
        // project_id` is deliberately relative — pinned by
        // `project_ids_are_relative_so_they_survive_a_move` — so
        // `src/Api/Api.csproj` is `src-Api-Api.csproj:test:debug` in *any*
        // checkout with that layout. Two clones of one service collide exactly.
        //
        // And the consequence is not a stale panel. "Re-run failed" turns these
        // names into a runner filter, so the user presses it in workspace B and
        // is shown a run that claims to re-run their failures while naming
        // workspace A's.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        // The switch happens while /a's suite is still running, which is why the
        // record arrives afterwards carrying /a's root.
        state.set_workspace(workspace_at("/b")).unwrap();
        let recorded = state.record_test_run(Path::new("/a"), "cfg", failing_run_from("/a"));

        assert!(
            !recorded,
            "a run from a workspace that is no longer open reported itself recorded"
        );
        assert_eq!(
            state
                .previous_test_run("cfg")
                .map(|run| run.cases[0].full_name.clone()),
            None,
            "\"re-run failed\" in this workspace would build a filter naming the \
             previous workspace's failed tests"
        );
    }

    #[test]
    fn the_workspace_cannot_be_replaced_between_the_test_run_root_check_and_the_store() {
        // The check and the write have to be one step. Comparing the root, then
        // releasing the workspace lock, then inserting leaves the same window the
        // check exists to close, reached by passing the check honestly.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();

        let mut stolen = None;
        let recorded = state.record_test_run_interleaved(
            Path::new("/a"),
            "cfg",
            failing_run_from("/a"),
            || stolen = Some(steal_workspace(&state, "/b")),
        );

        // The window first, because it is the defect; what follows is its
        // consequence and would otherwise report the symptom with the cause
        // unnamed.
        assert!(
            !stolen.unwrap().load(Ordering::SeqCst),
            "the workspace was replaceable between the check and the store, \
             which is the whole race"
        );
        assert!(recorded);
        assert_eq!(state.workspace().unwrap().root, PathBuf::from("/a"));
    }

    #[test]
    fn a_capture_that_completes_after_another_workspace_is_opened_is_not_served_to_it() {
        // Same shape as the test run, with no key at all: `last_inspect` is one
        // slot. `inspect_capture` reads the root, spawns the sidecar, waits,
        // parses, and records — and a capture landing after a switch is handed
        // to the new workspace by `inspect_last`, carrying a dump path under the
        // old root.
        //
        // `set_workspace`'s own note says carrying a capture across "would go on
        // holding somebody's process memory after the reason to hold it went
        // away". This is that sentence from the other direction: the clear ran,
        // and the capture arrived after it and put the memory back.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        state.set_workspace(workspace_at("/b")).unwrap();
        let recorded = state.record_inspect(Path::new("/a"), capture_under("/a"));

        assert!(
            !recorded,
            "a capture of a workspace that is no longer open reported itself recorded"
        );
        assert_eq!(
            state.previous_inspect().as_ref().map(dump_path),
            None,
            "the object tree of one repository was left where the next \
             `inspect_last` would serve it as another's"
        );
    }

    #[test]
    fn the_workspace_cannot_be_replaced_between_the_capture_root_check_and_the_store() {
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();

        let mut stolen = None;
        let recorded =
            state.record_inspect_interleaved(Path::new("/a"), capture_under("/a"), || {
                stolen = Some(steal_workspace(&state, "/b"))
            });

        assert!(
            !stolen.unwrap().load(Ordering::SeqCst),
            "the workspace was replaceable between the check and the store, \
             which is the whole race"
        );
        assert!(recorded);
        assert_eq!(
            state.previous_inspect().as_ref().map(dump_path).unwrap(),
            dump_path(&capture_under("/a")),
        );
    }

    #[test]
    fn rescanning_the_same_workspace_still_accepts_what_it_produced() {
        // The guard must reject another repository's result, not every result.
        // Saving a configuration re-scans and sets the workspace again under the
        // same root, and a run or capture finishing across that must still land
        // — otherwise the guard has traded a wrong answer for no answer.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        state.set_workspace(workspace_at("/a")).unwrap();

        assert!(state.record_test_run(Path::new("/a"), "cfg", failing_run_from("/a")));
        assert!(state.record_inspect(Path::new("/a"), capture_under("/a")));
        assert!(state.previous_test_run("cfg").is_some());
        assert!(state.previous_inspect().is_some());
    }

    // -----------------------------------------------------------------------
    // The language-server session
    // -----------------------------------------------------------------------

    /// A machine with no language server on it anywhere.
    ///
    /// The point is that a real session can be started in a unit test without
    /// starting a real server: every language resolves to `NotFound`, so the
    /// actor comes up with a complete status snapshot, four rows in it, and no
    /// child process of any kind. It is the same argument
    /// `cb_core::lsp::registry::Probe` exists for, and the reason
    /// `session::start_with_probe` is public.
    struct NothingInstalled;

    impl Probe for NothingInstalled {
        fn on_path(&self, _name: &str) -> Option<PathBuf> {
            None
        }
        fn is_file(&self, _path: &Path) -> bool {
            false
        }
        fn is_dir(&self, _path: &Path) -> bool {
            false
        }
        fn read_dir(&self, _path: &Path) -> Vec<PathBuf> {
            Vec::new()
        }
        fn home(&self) -> Option<PathBuf> {
            None
        }
        fn env(&self, _key: &str) -> Option<String> {
            None
        }
    }

    /// A live session for `root`, claiming its generation from `state` the way
    /// `commands::lsp::ensure_session` does.
    fn session_for(state: &AppState, root: &str) -> LspHandle {
        cb_core::lsp::session::start_with_probe(
            PathBuf::from(root),
            None,
            state.begin_lsp_session(),
            Arc::new(NothingInstalled),
        )
    }

    /// Whether this session is still able to answer anything.
    ///
    /// Read from the status snapshot, which teardown *clears* — "cleared, not
    /// frozen", in `Session::teardown`'s own words, precisely so that a torn-down
    /// session cannot be mistaken for one that can answer. Every test that uses
    /// this asserts the live reading first, so a false negative from an empty
    /// snapshot for some other reason cannot pass for a teardown.
    fn answers(handle: &LspHandle) -> bool {
        !handle.status().servers.is_empty()
    }

    /// Wait for the actor to finish tearing down, or fail saying it never did.
    ///
    /// Bounded, because a test that can hang is worse than no test. The wait is
    /// real work and not a `sleep` guess: `request_teardown` only *enqueues*, so
    /// the observable effect lands whenever the runtime next polls the actor.
    async fn tears_down(handle: &LspHandle) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while answers(handle) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the session never tore down");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_started_session_is_published_for_the_workspace_it_was_started_for() {
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let handle = session_for(&state, "/a");
        assert!(
            answers(&handle),
            "the fake session was inert, so nothing below would be measuring a teardown"
        );

        assert!(state.record_lsp_session(handle).is_ok());
        assert_eq!(
            state.lsp().map(|h| h.root().to_path_buf()),
            Some(PathBuf::from("/a"))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_started_for_a_workspace_that_is_no_longer_open_is_handed_back_for_teardown()
    {
        // The widest window in this file. Resolution reads directories, the
        // caller then publishes, and the workspace can be replaced in between —
        // so the check has to be here. What makes this different from every other
        // stale-write guard is what is being refused: not a stale test result but
        // a running process tree. A `bool` would leave the caller holding nothing
        // it could shut down, and Roslyn and its `BuildHost` children would
        // outlive the workspace they were started for.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        let handle = session_for(&state, "/a");
        assert!(answers(&handle));

        state.set_workspace(workspace_at("/b")).unwrap();

        let Err(rejected) = state.record_lsp_session(handle) else {
            panic!("a session for a workspace that is no longer open was published");
        };
        assert!(state.lsp().is_none());

        // The handback is the point. What comes back is the live session itself,
        // so the caller can do the one thing a `false` would not have let it do.
        assert!(answers(&rejected));
        rejected.request_teardown();
        tears_down(&rejected).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_for_the_wrong_root_is_refused_even_when_its_start_is_the_current_one() {
        // The test above passes with the root check deleted — verified by
        // mutation — because opening another workspace also bumps the
        // generation, so the generation check catches it. That makes the root
        // check unexercised there, and an unexercised guard is one somebody
        // deletes as dead. Here the generation is claimed *after* the last swap
        // and therefore matches, so nothing but the root can refuse this.
        let state = AppState::default();
        state.set_workspace(workspace_at("/b")).unwrap();

        let handle = session_for(&state, "/a");
        assert_eq!(
            state.lsp_generation.load(Ordering::SeqCst),
            handle.generation(),
            "the generation check would have refused this on its own"
        );

        let Err(rejected) = state.record_lsp_session(handle) else {
            panic!("a session for a root other than the open workspace was published");
        };
        assert!(state.lsp().is_none());
        rejected.request_teardown();
        tears_down(&rejected).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn opening_another_workspace_tears_down_the_first_ones_session() {
        // Servers are per-workspace: Roslyn was started with `/a`'s solution and
        // knows nothing about `/b`. Left running it would go on answering under
        // the new workspace's name, and go on holding a solution's worth of
        // memory for a repository nobody is looking at.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        let handle = session_for(&state, "/a");
        state.record_lsp_session(handle.clone()).unwrap();

        state.set_workspace(workspace_at("/b")).unwrap();

        assert!(
            state.lsp().is_none(),
            "the previous workspace's session was left where the next request would use it"
        );
        tears_down(&handle).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rescanning_the_same_workspace_keeps_its_session() {
        // A rescan happens on every configuration save. Restarting Roslyn each
        // time would cost tens of seconds of "still loading" over an unchanged
        // solution, which reads as a broken feature rather than as a rescan.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        let handle = session_for(&state, "/a");
        state.record_lsp_session(handle.clone()).unwrap();

        state.set_workspace(workspace_at("/a")).unwrap();

        assert!(state.lsp().is_some(), "a rescan threw the session away");
        // Not merely still in the slot: still able to answer. A handle whose
        // actor had been told to shut down would satisfy the assertion above and
        // nothing else.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            answers(&handle),
            "the session was torn down by a rescan that changed nothing"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_superseded_start_does_not_report_the_current_one_ready() {
        // Two starts for the *same* root, which the root check cannot separate:
        // both name the open workspace and both pass it. The older one landing
        // second would publish a second live server tree over the newer one —
        // one of which nothing would ever shut down — and would serve a
        // resolution taken before whatever change prompted the restart.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let superseded = session_for(&state, "/a");
        let current = session_for(&state, "/a");

        let Err(rejected) = state.record_lsp_session(superseded) else {
            panic!("a superseded start published itself over the current one");
        };
        rejected.request_teardown();

        state.record_lsp_session(current.clone()).unwrap();
        assert_eq!(
            state.lsp().map(|h| h.generation()),
            Some(current.generation()),
            "the slot holds a session other than the current start's"
        );
        current.request_teardown();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_session_for_one_workspace_is_refused_rather_than_leaked() {
        // Two commands arriving on a cold session both find the slot empty and
        // both start. The loser must be handed back, not overwritten: an
        // overwritten handle is a language server nothing holds a reference to.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        let first = session_for(&state, "/a");
        state.record_lsp_session(first.clone()).unwrap();

        let second = session_for(&state, "/a");
        let Err(rejected) = state.record_lsp_session(second) else {
            panic!("a second session was published over a live one, leaking it");
        };
        rejected.request_teardown();
        tears_down(&rejected).await;

        assert!(answers(&first), "the incumbent was torn down instead");
        first.request_teardown();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_workspace_cannot_be_replaced_between_the_sessions_root_check_and_its_publish() {
        // The same window as every other guard in this file, driven rather than
        // hoped for. If the workspace lock is free at this instant the race is
        // reachable, and what lands in the gap is a live server tree published
        // under a workspace it was not started for.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();
        let handle = session_for(&state, "/a");

        let mut stolen = None;
        let published = state.record_lsp_session_interleaved(handle.clone(), || {
            stolen = Some(steal_workspace(&state, "/b"))
        });

        assert!(
            !stolen.unwrap().load(Ordering::SeqCst),
            "the workspace was replaceable between the check and the publish, \
             which is the whole race"
        );
        assert!(published.is_ok());
        assert_eq!(
            state.lsp().map(|h| h.root().to_path_buf()),
            Some(state.workspace().unwrap().root),
            "a session was published under a workspace it was not started for"
        );
        handle.request_teardown();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_lsp_slot_is_taken_in_the_same_order_as_every_other_cache() {
        // The fifth mutex, under contention, with **both** bodies that hold
        // `workspace` and then reach for `lsp` live at once — because that is
        // what a lock-order test costs: a deadlock needs two bodies with opposite
        // orders, so a test running only one of them vouches for a pair it never
        // put in contention. There are exactly two such bodies, and each group of
        // threads below runs one of them:
        //
        // * `set_workspace`'s `changed` branch (`take_lsp_session` under the
        //   workspace guard) — reached only when the root actually *changes*, so
        //   the swappers alternate roots. A version of this test that used one
        //   root never executed this branch at all.
        // * `record_lsp_session` (the slot check and publish under the same
        //   guard) — reached only by a record whose root and generation both
        //   match, so the recorders claim a fresh generation each iteration
        //   rather than re-using one the swaps have already superseded.
        //
        // Reverse the two statements in either body and this hangs.
        let state = Arc::new(AppState::default());
        state.set_workspace(workspace_at("/a")).unwrap();
        let runtime = tokio::runtime::Handle::current();
        // Counted, so a run in which nothing ever reached the `lsp` lock fails
        // instead of passing quietly — which is the failure mode of every stress
        // test written without one.
        let published = Arc::new(AtomicU64::new(0));

        let mut threads = Vec::new();
        for _ in 0..4 {
            let state = Arc::clone(&state);
            threads.push(std::thread::spawn(move || {
                for _ in 0..300 {
                    state.set_workspace(workspace_at("/b")).unwrap();
                    state.set_workspace(workspace_at("/a")).unwrap();
                }
            }));
        }
        for _ in 0..4 {
            let state = Arc::clone(&state);
            let published = Arc::clone(&published);
            let runtime = runtime.clone();
            threads.push(std::thread::spawn(move || {
                // The sessions are spawned tasks, so this thread needs the test's
                // runtime; `session_for` would panic on a bare `std::thread`.
                let _entered = runtime.enter();
                for _ in 0..300 {
                    match state.record_lsp_session(session_for(&state, "/a")) {
                        Ok(()) => {
                            published.fetch_add(1, Ordering::SeqCst);
                            // Taken straight back out, so the next iteration is
                            // refused by the *generation* rather than by a slot
                            // this test filled and forgot — and so nothing is
                            // left running when it ends.
                            if let Some(session) = state.take_lsp_session() {
                                session.request_teardown();
                            }
                        }
                        Err(rejected) => rejected.request_teardown(),
                    }
                    let _ = state.lsp();
                }
            }));
        }
        for thread in threads {
            thread.join().expect("a worker deadlocked or panicked");
        }

        assert!(
            published.load(Ordering::SeqCst) > 0,
            "no record ever reached the `lsp` lock, so this vouched for a pair it \
             never exercised"
        );
        if let Some(session) = state.take_lsp_session() {
            session.request_teardown();
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_poisoned_session_slot_still_gives_up_the_superseded_workspaces_servers() {
        // The one place this file does not treat "no answer" as the safe failure.
        // A poisoned `lsp` mutex read as `None` would make `set_workspace`'s
        // `if let Some(stale)` a no-op: the previous workspace's server tree keeps
        // running *and* stays in the slot, so every later request starts a session
        // that is refused because the slot is occupied and torn down again — the
        // feature off for the life of the process, with an orphan to go with it.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        let handle = session_for(&state, "/a");
        state.record_lsp_session(handle.clone()).unwrap();
        assert!(answers(&handle));

        // Poisons the mutex. The panic message below is expected output of a
        // passing test.
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.lsp.lock().unwrap();
            panic!("deliberately poisoning the session slot");
        }));
        assert!(poisoned.is_err());
        assert!(state.lsp.is_poisoned(), "the slot was not poisoned");

        assert!(
            state.lsp().is_some(),
            "a poisoned slot went silent about a live session, so every later \
             request would start one and be refused"
        );

        state.set_workspace(workspace_at("/b")).unwrap();
        assert!(
            state.lsp().is_none(),
            "the superseded workspace's session was left in the slot"
        );
        tears_down(&handle).await;
    }
}
