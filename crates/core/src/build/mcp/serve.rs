//! The Build MCP server's handshake, and the shape of every answer it sends.
//!
//! The runner in the app crate (`src-tauri/src/mcp_build.rs`) is I/O and nothing
//! else, for the reason [`crate::mcp::serve`] gives about its own runner: a binary
//! that reads stdin and opens a named pipe is not reachable from a test. So
//! everything between "a line arrived" and "a line goes back" that is a *choice*
//! lives here or in [`super::tools`], [`super::wire`] and [`super::instances`].
//!
//! # Routing is not re-implemented
//!
//! [`crate::mcp::serve::route`], `success`, `failure`, `parse_failure` and
//! `negotiated_version` are about **JSON-RPC and MCP**, not about any one server, so
//! this server reuses them unchanged — the same decision the Roslyn, browser and
//! Tasks servers made.
//!
//! Only three things here are this server's own: who it says it is, what it tells a
//! model before its first call, and how a [`super::wire::ToolAnswer`] becomes a
//! `CallToolResult`.
//!
//! # The lifetime rule: this server starts whether or not the application does
//!
//! A server that fails `initialize` is dropped **silently** by its host and the
//! user never learns why. So nothing here consults [`super::instances`]: the
//! handshake succeeds, `tools/list` advertises all four tools, and a call made with
//! no application running answers with the matching
//! [`super::instances::InstanceError`] sentence.

use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, ServerCapabilities,
    ToolsCapability,
};
use serde_json::{json, Value};

use super::tools;
use super::wire::ToolAnswer;

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "code-basics-build";

/// This build's version, taken from the crate rather than repeated.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a client is told before it reads a single tool description.
///
/// Rules a model cannot infer and will otherwise get wrong: the diagnostics are
/// parsed from what the compiler wrote (not console colour), the answers abstain
/// (never-built is not an empty success), and the answer is scoped to the one
/// workspace this server was installed for.
pub const INSTRUCTIONS: &str = "These tools run and read the build of one workspace: build the \
projects, and get the errors, warnings and status of the most recent build. The diagnostics are \
parsed from what MSBuild wrote — file, line, column, code, message — not scraped from console \
colour, so you can act on them directly.

Two things to know. The status has six distinct answers — never built, building, succeeded clean, \
succeeded with warnings, failed, and could-not-start — and asking for errors before any build has \
run is refused rather than reported as an empty success, so an empty answer is a real \"no \
errors\", not \"no build\". And the answers are scoped to the workspace this server was installed \
for; it cannot build another repository.";

/// The `initialize` result. Consults nothing — see the module docs.
pub fn initialize_result(requested: Option<&str>) -> InitializeResult {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());

    InitializeResult::new(capabilities)
        .with_protocol_version(crate::mcp::serve::negotiated_version(requested))
        .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
        .with_instructions(INSTRUCTIONS)
}

/// The `tools/list` result. Advertises every tool whether or not an application is
/// running: a tool that appears only sometimes is one a model will never learn to
/// use.
pub fn tools_list_result(gate: &crate::tool_gate::ToolGateFile) -> Value {
    json!({
        "tools": crate::tool_gate::filter_descriptors(
            gate,
            crate::tool_gate::ServerId::Build,
            tools::descriptors(),
        )
    })
}

/// A disabled-but-known tool, refused by the **shim** before it looks for a running
/// application.
pub fn disabled_tool_answer(tool: &str) -> ToolAnswer {
    ToolAnswer::refused(
        crate::tool_gate::DISABLED_CODE,
        crate::tool_gate::disabled_tool_sentence(tool),
    )
}

/// Turn the application's answer into an MCP result.
///
/// A refusal is a **tool execution error**, never a protocol error.
pub fn answer_result(answer: &ToolAnswer) -> CallToolResult {
    if answer.ok {
        return CallToolResult::success(vec![ContentBlock::text(answer.text.clone())]);
    }
    let code = answer.code.clone().unwrap_or_else(|| "refused".to_string());
    let mut result =
        CallToolResult::error(vec![ContentBlock::text(format!("{code}: {}", answer.text))]);
    result.structured_content = Some(json!({ "code": code }));
    result
}

/// A tool name this server does not have, answered by the **shim** before it looks
/// for a running application. The wording is [`tools::unknown_tool_sentence`],
/// shared with the application's own refusal so the two cannot drift.
pub fn unknown_tool_answer(tool: &str) -> ToolAnswer {
    ToolAnswer::refused("unknown_tool", tools::unknown_tool_sentence(tool))
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
