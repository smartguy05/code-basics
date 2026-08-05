//! Workspace file commands, for the Run tab's directory tree and file editor.

use std::path::PathBuf;

use cb_core::files::{self, DirEntry};
use tauri::State;

use crate::state::AppState;

/// List one directory of the workspace, filtered like the project scan.
#[tauri::command]
pub async fn fs_list_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<DirEntry>, String> {
    let root = state.workspace_root()?;
    files::list_dir(&root, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

/// Read a workspace file for the editor.
#[tauri::command]
pub async fn fs_read_file(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let root = state.workspace_root()?;
    files::read_file(&root, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

/// Save the editor's contents back to a workspace file.
#[tauri::command]
pub async fn fs_write_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), String> {
    let root = state.workspace_root()?;
    files::write_file(&root, &PathBuf::from(path), &content).map_err(|e| format!("{e:#}"))
}
