//! Commands behind the installable **Editor context** MCP server, plus the push
//! that feeds it.
//!
//! The five install verbs are deliberately the same shape as
//! [`crate::commands::roslyn_mcp`], [`crate::commands::mcp`] and
//! [`crate::commands::browser_mcp`] — plan, confirm, apply, re-read the status
//! from disk — because it is the same machinery underneath:
//! [`cb_core::editor_context::install`] reuses `mcp::install`'s `config_target`,
//! `mcp_merge` and `codex_toml` rather than growing a second merge, and the writes
//! go through [`providers::apply_writes_atomically`] for the same reason as there.
//!
//! **Nothing new crosses IPC for the install verbs.** `InstallPlan`,
//! `PlannedWrite`, `InstallScope` and `ProviderId` already do, and a status is
//! exactly `Option<InstallScope>`.
//!
//! # The one thing this module has that the others do not
//!
//! [`set_editor_context`] — the push. Editor state lives in the React frontend,
//! so unlike every other MCP server (whose data the backend already holds), this
//! one is *fed* from the frontend, exactly as the browser's automation consent is.
//! The command only records; whether to push at all — the feature-off gate — is
//! the frontend's decision, and the pipe host re-checks the feature before
//! answering so a stale push is never served.

use tauri::State;

use cb_core::editor_context::install as editor_install;
use cb_core::intents::providers::{self, InstallPlan, InstallScope};
use cb_core::intents::ProviderId;
use cb_core::model::EditorContext;

use crate::state::AppState;

/// Where the editor-context MCP server is installed for this workspace and
/// `provider`, if anywhere. Project scope wins over user scope, as it does for the
/// hooks.
#[tauri::command]
pub async fn editor_mcp_server_status(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    Ok(editor_install::status_for(provider, &root, None))
}

/// Exactly what installing the server for `provider` at `scope` would write.
/// Touches nothing.
#[tauri::command]
pub async fn editor_mcp_server_install_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    editor_install::install_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform an install the user has confirmed, returning the new status.
#[tauri::command]
pub async fn install_editor_mcp_server(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan = editor_install::install_plan_for(provider, &root, scope, None)
        .map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(editor_install::status_for(provider, &root, None))
}

/// Exactly what removing the server for `provider` at `scope` would rewrite.
/// Touches nothing. A zero-write plan means there was nothing of ours there.
#[tauri::command]
pub async fn editor_mcp_server_uninstall_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    editor_install::uninstall_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform a removal the user has confirmed, returning the new status.
#[tauri::command]
pub async fn uninstall_editor_mcp_server(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan = editor_install::uninstall_plan_for(provider, &root, scope, None)
        .map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(editor_install::status_for(provider, &root, None))
}

/// Push the live editor state for `root` into the backend.
///
/// Called by the frontend (debounced) while the `EditorContextMcp` feature is on;
/// the editor-context MCP pipe reads it back to answer an agent. This only
/// records — the feature-off gate is the frontend's (it stops calling this), and
/// the pipe host re-checks the feature before serving, so a context left here by a
/// feature switched off after a push is never exposed.
///
/// A push whose workspace has since closed is a harmless no-op: the caller's data
/// is dropped and nothing reads a removed slot again.
#[tauri::command]
pub async fn set_editor_context(
    state: State<'_, AppState>,
    root: String,
    ctx: EditorContext,
) -> Result<(), String> {
    state.record_editor_context(std::path::Path::new(&root), ctx);
    Ok(())
}
