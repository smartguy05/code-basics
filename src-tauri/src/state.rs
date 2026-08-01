//! Shared application state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

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

    pub fn set_workspace(&self, workspace: Workspace) -> Result<(), String> {
        *self
            .workspace
            .lock()
            .map_err(|_| "application state is unavailable".to_string())? = Some(workspace);
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
}
