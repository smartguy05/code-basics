//! Commands behind the installable **Build & Diagnostics** MCP server.
//!
//! Deliberately the same five-command shape as [`crate::commands::roslyn_mcp`]
//! and [`crate::commands::editor_context_mcp`] — plan, confirm, apply, re-read
//! the status from disk — because it is the same machinery underneath:
//! [`cb_core::build::mcp::install`] reuses `mcp::install`'s `config_target`,
//! `mcp_merge` and `codex_toml` rather than growing a second merge, and the
//! writes go through [`providers::apply_writes_atomically`] for the same reason
//! as there (`~/.claude.json` is 122 KB and rewritten continuously by a running
//! Claude Code, so a truncate-then-write loses somebody's whole configuration if
//! the process dies between the two).
//!
//! **Nothing new crosses IPC.** `InstallPlan`, `PlannedWrite`, `InstallScope`
//! and `ProviderId` already do, and a status is exactly `Option<InstallScope>`.
//!
//! # Why this is a separate module rather than a `kind` parameter
//!
//! The servers are installed under different names, carry different arguments
//! and — the part that matters — carry **different caveats**. A shared command
//! with a discriminator would put the choice of which warnings the user sees
//! behind a parameter; the Build server grants running-and-reading the build of
//! one repository, which is a different thing to reason about than a database
//! login, the semantic model or the live editor state.

use tauri::State;

use cb_core::build::mcp::install as build_install;
use cb_core::intents::providers::{self, InstallPlan, InstallScope};
use cb_core::intents::ProviderId;

use crate::state::AppState;

/// Where the Build MCP server is installed for this workspace and `provider`,
/// if anywhere. Project scope wins over user scope, as it does for the hooks.
#[tauri::command]
pub async fn build_mcp_server_status(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    Ok(build_install::status_for(provider, &root, None))
}

/// Exactly what installing the server for `provider` at `scope` would write.
/// Touches nothing.
#[tauri::command]
pub async fn build_mcp_server_install_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    build_install::install_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform an install the user has confirmed, returning the new status.
#[tauri::command]
pub async fn install_build_mcp_server(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan = build_install::install_plan_for(provider, &root, scope, None)
        .map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(build_install::status_for(provider, &root, None))
}

/// Exactly what removing the server for `provider` at `scope` would rewrite.
/// Touches nothing. A zero-write plan means there was nothing of ours there.
#[tauri::command]
pub async fn build_mcp_server_uninstall_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    build_install::uninstall_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform a removal the user has confirmed, returning the new status.
#[tauri::command]
pub async fn uninstall_build_mcp_server(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan = build_install::uninstall_plan_for(provider, &root, scope, None)
        .map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(build_install::status_for(provider, &root, None))
}
