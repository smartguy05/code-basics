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

    config::apply(&mut scanned, saved);
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

/// Names of the launch profiles a .NET project defines, for the config
/// editor's profile dropdown. Only `Project` profiles are returned — the only
/// kind `dotnet run --launch-profile` can apply.
#[tauri::command]
pub async fn launch_profiles(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<String>, String> {
    let root = state.workspace_root()?;
    Ok(workspace::launch_profiles(&root.join(project))
        .into_iter()
        .map(|p| p.name)
        .collect())
}

/// Star or unstar a configuration. Favourites sort to the top of every list.
#[tauri::command]
pub async fn set_favorite(
    state: State<'_, AppState>,
    id: String,
    favorite: bool,
) -> Result<Workspace, String> {
    let root = state.workspace_root()?;
    config::set_favorite(&root, &id, favorite).map_err(|e| format!("{e:#}"))?;

    let workspace = load(&root)?;
    state.set_workspace(workspace.clone())?;
    Ok(workspace)
}

/// Persist the user's preferred configuration ordering, as a list of ids.
#[tauri::command]
pub async fn set_config_order(
    state: State<'_, AppState>,
    order: Vec<String>,
) -> Result<Workspace, String> {
    let root = state.workspace_root()?;
    config::set_order(&root, order).map_err(|e| format!("{e:#}"))?;

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
    let workspace = state.workspace()?;
    let mut result = rider::import(&workspace.root);

    // Compound members are recorded as Rider display names; rewrite them into
    // ids of configurations that actually exist here.
    rider::resolve_compounds(&mut result.configs, &workspace.configs);

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
