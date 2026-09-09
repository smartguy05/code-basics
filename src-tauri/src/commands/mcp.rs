//! Commands behind the installable SQL MCP server.
//!
//! Deliberately the same shape as [`crate::commands::qgate`]: a pure
//! [`InstallPlan`] is previewed, the user confirms, the writes are applied, and
//! the resulting status is read back from disk rather than assumed. Every
//! decision — which file each provider keeps its servers in, how an existing one
//! is merged into, what to warn about — lives in [`cb_core::mcp::install`].
//!
//! # Two differences from the gate, both deliberate
//!
//! **Nothing new crosses IPC.** `InstallPlan`, `PlannedWrite`, `InstallScope`
//! and `ProviderId` already do, and a status is exactly `Option<InstallScope>`.
//! An `McpInstallStatus` struct would carry nothing that does not, while adding
//! a type the frontend has to be kept in step with.
//!
//! **The writes go through [`providers::apply_writes_atomically`], not
//! `apply_writes`.** The gate edits a small `.claude/settings.json`; this edits
//! `~/.claude.json`, which is 122 KB and rewritten continuously by a running
//! Claude Code. A truncate-then-write there loses somebody's whole configuration
//! if the process dies between the two, so the write is a temp file and a
//! rename.

use tauri::State;

use cb_core::intents::providers::{self, InstallPlan, InstallScope};
use cb_core::intents::ProviderId;
use cb_core::mcp::install::plan as mcp_plan;

use crate::state::AppState;

/// Where the SQL MCP server is installed for this workspace and `provider`, if
/// anywhere. Project scope wins over user scope, as it does for the hooks.
#[tauri::command]
pub async fn mcp_server_status(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    Ok(mcp_plan::status_for(provider, &root, None))
}

/// Exactly what installing the server for `provider` at `scope` would write.
/// Touches nothing.
#[tauri::command]
pub async fn mcp_server_install_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    mcp_plan::install_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform an install the user has confirmed, returning the new status.
#[tauri::command]
pub async fn install_mcp_server(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan =
        mcp_plan::install_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(mcp_plan::status_for(provider, &root, None))
}

/// Exactly what removing the server for `provider` at `scope` would rewrite.
/// Touches nothing. A zero-write plan means there was nothing of ours there.
#[tauri::command]
pub async fn mcp_server_uninstall_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    mcp_plan::uninstall_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform a removal the user has confirmed, returning the new status.
#[tauri::command]
pub async fn uninstall_mcp_server(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let root = state.workspace_root()?;
    let plan =
        mcp_plan::uninstall_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(mcp_plan::status_for(provider, &root, None))
}
