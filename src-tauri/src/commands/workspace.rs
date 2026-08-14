//! Workspace and configuration commands.

use std::path::PathBuf;

use cb_core::adapters::dotnet;
use cb_core::config;
use cb_core::importers::rider;
use cb_core::model::RunConfig;
use cb_core::workspace::{self, Workspace};
use serde::Serialize;
use tauri::State;

use crate::commands::symbols::{spawn_build, Rebuild};
use crate::state::AppState;

/// Scan a workspace and layer saved configurations on top of detected ones.
///
/// The saved file is read first because it carries the scan options — opting
/// into MSBuild evaluation has to be known before the scan runs, not after.
fn load(root: &std::path::Path) -> Result<Workspace, String> {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let saved = config::load(&root).map_err(|e| format!("{e:#}"))?;

    let options = workspace::ScanOptions {
        msbuild_evaluation: saved.msbuild_evaluation,
    };
    let mut scanned = workspace::scan_with(&root, options).map_err(|e| format!("{e:#}"))?;

    config::apply(&mut scanned, saved);
    Ok(scanned)
}

/// Open a workspace and return it, without waiting for its symbol index.
///
/// The index build is spawned and abandoned. That is deliberate and is the
/// reason it is not built inline: a cold build is 20 ms on this repository but
/// 637 ms on the 2,864-file .NET solution this application was written for, and
/// 9.4 s against a cold filesystem cache. None of that may sit between the user
/// choosing a folder and seeing what is in it — the palette can be empty for a
/// second, the project list cannot.
///
/// `app` is injected by Tauri and is not part of the arguments the frontend
/// sends. The background thread needs an owned, `'static` handle on the state,
/// which `State<'_, _>` cannot give it.
#[tauri::command]
pub async fn open_workspace(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Workspace, String> {
    let workspace = load(&PathBuf::from(path))?;
    state.set_workspace(workspace.clone())?;
    // Started here as well as on demand, for the same reason as the index: the
    // status surface should already be able to say "no Python server is
    // installed" when the user first looks, and Roslyn's project load should
    // already be under way when they first ask a question. Both are spawned and
    // abandoned, so neither sits between choosing a folder and seeing it.
    crate::commands::lsp::spawn_session(app.clone());
    spawn_build(app, workspace.clone(), Rebuild::Cached);
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
///
/// Re-indexes too, in the background and on the cache, since a rescan is
/// exactly when files have appeared or a project has moved — and a project
/// moving changes what every symbol is attributed to, which the per-file
/// fingerprint cannot see. `build_cached` recognises that itself and discards
/// the cache when the project list has changed, so this stays the cheap call
/// even though it sometimes does the expensive thing.
///
/// The other commands in this file also re-scan and set the workspace, and
/// deliberately do not re-index: saving, deleting, starring or reordering a
/// configuration cannot move a single declaration. Rebuilding on every star
/// would spend the better part of a second for nothing.
#[tauri::command]
pub async fn rescan_workspace(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Workspace, String> {
    let root = state.workspace_root()?;
    let workspace = load(&root)?;
    state.set_workspace(workspace.clone())?;
    // A rescan does not change the root, so `set_workspace` keeps the existing
    // session and this is a no-op against it — deliberately. Restarting Roslyn on
    // every configuration save would cost tens of seconds of "still loading" over
    // a solution that did not change. It matters on the path where there is no
    // session yet, which a rescan is as good a moment to notice as any.
    crate::commands::lsp::spawn_session(app.clone());
    spawn_build(app, workspace.clone(), Rebuild::Cached);
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

/// The launch profiles a .NET project defines, for the config editor.
///
/// Every profile is returned, including the ones `dotnet run --launch-profile`
/// cannot apply: `LaunchProfile::launchable` marks those, and showing them
/// disabled explains why a project appears to have no profiles far better than
/// an empty dropdown does. The environment variables, arguments and
/// application URL come along so the editor can show what a profile will
/// actually do.
#[tauri::command]
pub async fn launch_profiles(
    state: State<'_, AppState>,
    project: String,
) -> Result<Vec<dotnet::LaunchProfile>, String> {
    let root = state.workspace_root()?;
    Ok(workspace::launch_profiles(&root.join(project)))
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
