//! Shared application state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use cb_core::inspect::InspectGraph;
use cb_core::model::TestRunResult;
use cb_core::process::Supervisor;
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
    /// Both caches below are per-workspace and neither is keyed by one: a
    /// capture is a copy of a process's memory and a test result names tests
    /// that need not exist in another repository. Carrying either across would
    /// attribute one repository's data to another — and in the capture's case
    /// would go on holding somebody's process memory after the reason to hold
    /// it went away.
    ///
    /// Only a change of root clears them. This is also the rescan path (saving
    /// a configuration re-scans and sets the workspace again), and a rescan
    /// must not throw away the results "re-run failed" needs.
    pub fn set_workspace(&self, workspace: Workspace) -> Result<(), String> {
        let mut current = self
            .workspace
            .lock()
            .map_err(|_| "application state is unavailable".to_string())?;

        let changed = current
            .as_ref()
            .is_none_or(|open| open.root != workspace.root);
        *current = Some(workspace);
        drop(current);

        if changed {
            self.clear_inspect();
            if let Ok(mut runs) = self.last_test_run.lock() {
                runs.clear();
            }
        }
        Ok(())
    }

    pub fn record_test_run(&self, config_id: &str, result: TestRunResult) {
        if let Ok(mut runs) = self.last_test_run.lock() {
            runs.insert(config_id.to_string(), result);
        }
    }

    pub fn previous_test_run(&self, config_id: &str) -> Option<TestRunResult> {
        self.last_test_run.lock().ok()?.get(config_id).cloned()
    }

    pub fn record_inspect(&self, graph: InspectGraph) {
        if let Ok(mut last) = self.last_inspect.lock() {
            *last = Some(graph);
        }
    }

    pub fn previous_inspect(&self) -> Option<InspectGraph> {
        self.last_inspect.lock().ok()?.clone()
    }

    pub fn clear_inspect(&self) {
        if let Ok(mut last) = self.last_inspect.lock() {
            *last = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cb_core::inspect::{Caps, InspectTarget, TargetSummary};

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

    fn capture() -> InspectGraph {
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
        state.record_inspect(capture());
        state.record_test_run("cfg", test_run());

        state.set_workspace(workspace_at("/b")).unwrap();

        assert!(state.previous_inspect().is_none());
        assert!(state.previous_test_run("cfg").is_none());
    }

    #[test]
    fn rescanning_the_same_workspace_keeps_what_it_captured() {
        // Saving a configuration re-scans and sets the workspace again. Losing
        // the tree, or the results "re-run failed" reads, on every save would
        // make both features look broken.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();
        state.record_inspect(capture());
        state.record_test_run("cfg", test_run());

        state.set_workspace(workspace_at("/a")).unwrap();

        assert!(state.previous_inspect().is_some());
        assert!(state.previous_test_run("cfg").is_some());
    }
}
