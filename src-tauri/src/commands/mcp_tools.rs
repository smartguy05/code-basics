//! Per-tool MCP gating commands.
//!
//! The bridge and nothing else: which servers and tools exist, what a corrupt
//! store tolerates, and the enable/disable rule all live in [`cb_core::tool_gate`].
//! Like [`crate::commands::features`], these take no `AppState` — the store is
//! user-global, not per-workspace, so there is no workspace to resolve. The MCP
//! server processes read this same file directly at their own path.

use cb_core::tool_gate::{self, McpServerToolsInfo, ServerId};

/// Every MCP server with its tools and their current enabled state.
#[tauri::command]
pub async fn list_mcp_tools() -> Result<Vec<McpServerToolsInfo>, String> {
    Ok(tool_gate::load(&tool_gate::mcp_tools_path()).list())
}

/// Turn one tool on or off, and report the resulting set.
///
/// Returns the whole list rather than nothing so the caller re-renders from what
/// was actually persisted, exactly like [`crate::commands::features::set_feature`].
/// A server that is fully switched off through its feature flag is unaffected here
/// — that is a separate control.
#[tauri::command]
pub async fn set_mcp_tool(
    server: String,
    tool: String,
    enabled: bool,
) -> Result<Vec<McpServerToolsInfo>, String> {
    let server = ServerId::from_id(&server)?;
    let path = tool_gate::mcp_tools_path();
    let mut file = tool_gate::load(&path);
    file.set(server, &tool, enabled);
    tool_gate::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(file.list())
}
