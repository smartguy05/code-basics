//! Change-group commands.
//!
//! Local bookkeeping rather than git operations: every one of these reads and
//! writes `.code-basics/changelists.json` and touches no repository state. Each
//! returns the full set afterwards, so the Changes tab re-renders from one
//! round trip instead of following up with a read.

use cb_core::changelists::{self, Changelists};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn git_changelists(state: State<'_, AppState>) -> Result<Changelists, String> {
    let root = state.workspace_root()?;
    changelists::load(&root).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_create_changelist(
    state: State<'_, AppState>,
    name: String,
) -> Result<Changelists, String> {
    let root = state.workspace_root()?;
    changelists::create(&root, &name).map_err(|e| format!("{e:#}"))
}

/// Delete a group. Its files become ungrouped rather than disappearing.
#[tauri::command]
pub async fn git_delete_changelist(
    state: State<'_, AppState>,
    name: String,
) -> Result<Changelists, String> {
    let root = state.workspace_root()?;
    changelists::remove(&root, &name).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_rename_changelist(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<Changelists, String> {
    let root = state.workspace_root()?;
    changelists::rename(&root, &from, &to).map_err(|e| format!("{e:#}"))
}

/// Move files into a group, or out of every group when `group` is absent.
#[tauri::command]
pub async fn git_assign_to_changelist(
    state: State<'_, AppState>,
    paths: Vec<String>,
    group: Option<String>,
) -> Result<Changelists, String> {
    let root = state.workspace_root()?;
    changelists::assign(&root, &paths, group.as_deref()).map_err(|e| format!("{e:#}"))
}
