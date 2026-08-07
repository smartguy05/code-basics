//! .NET user secrets commands.
//!
//! `project` is the workspace-relative path a `RunConfig.project` holds for a
//! .NET configuration — the `.csproj` — resolved against the open workspace so
//! the frontend never sends an absolute path.

use std::path::PathBuf;

use cb_core::secrets::{self, ProjectSecrets};
use tauri::State;

use crate::state::AppState;

/// Resolve a workspace-relative project path, refusing anything that escapes
/// the workspace root. The containment rule itself lives in
/// `cb_core::secrets::resolve_project_path`, where it is testable headlessly;
/// this only supplies the open workspace's root.
fn resolve(state: &State<'_, AppState>, project: &str) -> Result<PathBuf, String> {
    secrets::resolve_project_path(&state.workspace_root()?, project)
}

/// A project's user secrets: its id, where the secrets file lives, and the
/// file's contents when it exists.
#[tauri::command]
pub async fn read_project_secrets(
    state: State<'_, AppState>,
    project: String,
) -> Result<ProjectSecrets, String> {
    let path = resolve(&state, &project)?;
    secrets::read(&path).map_err(|e| format!("{e:#}"))
}

/// Save a project's secrets, adding a `<UserSecretsId>` to the project file
/// first when it has none.
#[tauri::command]
pub async fn write_project_secrets(
    state: State<'_, AppState>,
    project: String,
    content: String,
) -> Result<ProjectSecrets, String> {
    let path = resolve(&state, &project)?;
    secrets::write(&path, &content).map_err(|e| format!("{e:#}"))
}
