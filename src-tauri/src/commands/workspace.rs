//! Workspace and configuration commands.

use std::path::PathBuf;

use cb_core::config;
use cb_core::importers::rider;
use cb_core::model::RunConfig;
use cb_core::workspace::{self, Workspace};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Scan a workspace and layer saved configurations on top of detected ones.
fn load(root: &std::path::Path) -> Result<Workspace, String> {
    let mut scanned = workspace::scan(root).map_err(|e| format!("{e:#}"))?;
    let saved = config::load(&scanned.root).map_err(|e| format!("{e:#}"))?;

    scanned.configs = config::merge(scanned.configs, saved.configs);
    Ok(scanned)
}

#[tauri::command]
pub async fn open_workspace(
    state: State<'_, AppState>,
    path: String,
) -> Result<Workspace, String> {
    let workspace = load(&PathBuf::from(path))?;
    state.set_workspace(workspace.clone())?;
    Ok(workspace)
}

#[tauri::command]
pub async fn current_workspace(state: State<'_, AppState>) -> Result<Option<Workspace>, String> {
    Ok(state
        .workspace
        .lock()
        .map_err(|_| "application state is unavailable".to_string())?
        .clone())
}

/// Re-detect projects, picking up files added since the workspace was opened.
#[tauri::command]
pub async fn rescan_workspace(state: State<'_, AppState>) -> Result<Workspace, String> {
    let root = state.workspace_root()?;
    let workspace = load(&root)?;
    state.set_workspace(workspace.clone())?;
    Ok(workspace)
}

#[tauri::command]
pub async fn save_config(
    state: State<'_, AppState>,
    config: RunConfig,
) -> Result<Workspace, String> {
    let root = state.workspace_root()?;
    config::upsert(&root, config).map_err(|e| format!("{e:#}"))?;

    let workspace = load(&root)?;
    state.set_workspace(workspace.clone())?;
    Ok(workspace)
}

#[tauri::command]
pub async fn delete_config(state: State<'_, AppState>, id: String) -> Result<Workspace, String> {
    let root = state.workspace_root()?;
    config::remove(&root, &id).map_err(|e| format!("{e:#}"))?;

    let workspace = load(&root)?;
    state.set_workspace(workspace.clone())?;
    Ok(workspace)
}

/// What a Rider import would produce, for the review step.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiderImportPreview {
    pub configs: Vec<RunConfig>,
    /// Configurations recognised but not importable, as `(name, type)` pairs.
    pub skipped: Vec<(String, String)>,
}

/// Convert Rider's run configurations *without* saving them.
///
/// JetBrains publishes no schema for these files, so the results are always
/// reviewed before being written.
#[tauri::command]
pub async fn preview_rider_import(
    state: State<'_, AppState>,
) -> Result<RiderImportPreview, String> {
    let root = state.workspace_root()?;
    let result = rider::import(&root);

    Ok(RiderImportPreview {
        configs: result.configs,
        skipped: result.skipped,
    })
}

/// Save the configurations the user accepted from an import preview.
#[tauri::command]
pub async fn apply_rider_import(
    state: State<'_, AppState>,
    configs: Vec<RunConfig>,
) -> Result<Workspace, String> {
    let root = state.workspace_root()?;

    for config in configs {
        config::upsert(&root, config).map_err(|e| format!("{e:#}"))?;
    }

    let workspace = load(&root)?;
    state.set_workspace(workspace.clone())?;
    Ok(workspace)
}
