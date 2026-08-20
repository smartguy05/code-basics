//! Commands behind the installable quality-gate Stop hook.
//!
//! Installed the same way the intent hooks are: a pure [`InstallPlan`] is
//! previewed, then applied with [`providers::apply_writes`]. Every decision
//! lives in [`cb_core::qgate`]; these wrappers only resolve the workspace and
//! delegate, so there is nothing here a test could check that the core tests do
//! not already cover.

use tauri::State;

use cb_core::intents::providers::{self, InstallPlan, InstallScope};
use cb_core::qgate::install as qgate_install;

use crate::state::AppState;

/// Where the quality gate is installed for this workspace, if anywhere.
#[tauri::command]
pub async fn quality_gate_status(
    state: State<'_, AppState>,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    Ok(qgate_install::status(&root, None))
}

/// Exactly what installing the quality gate would write. Touches nothing.
#[tauri::command]
pub async fn quality_gate_install_plan(
    state: State<'_, AppState>,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    qgate_install::install_plan(&root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform an install the user has confirmed, returning the new status.
#[tauri::command]
pub async fn install_quality_gate(
    state: State<'_, AppState>,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan = qgate_install::install_plan(&root, scope, None).map_err(|e| format!("{e:#}"))?;
    providers::apply_writes(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(qgate_install::status(&root, None))
}
