//! The Roslyn MCP server's handshake, and the shape of every answer it sends.
//!
//! The runner in the app crate (`src-tauri/src/mcp_roslyn.rs`) is I/O and nothing
//! else, for the reason [`crate::mcp::serve`] gives about its own runner: a binary
//! that reads stdin and opens a named pipe is not reachable from a test. So
//! everything between "a line arrived" and "a line goes back" that is a *choice*
//! lives here or in [`super::tools`], [`super::wire`] and [`super::instances`].
//!
//! # Routing is not re-implemented
//!
//! [`crate::mcp::serve::route`], `success`, `failure`, `parse_failure` and
//! `negotiated_version` are about **JSON-RPC and MCP**, not about any one server,
//! so this server reuses them unchanged — the same decision the browser and Tasks
//! servers made. Writing a second recogniser would be the "two implementations of
//! one distinction" the crate warns against.
//!
//! Only three things here are this server's own: who it says it is, what it tells
//! a model before its first call, and how a [`super::wire::ToolAnswer`] becomes a
//! `CallToolResult`.
//!
//! # The lifetime rule: this server starts whether or not the application does
//!
//! A server that fails `initialize` is dropped **silently** by its host and the
//! user never learns why. So nothing here consults [`super::instances`]: the
//! handshake succeeds, `tools/list` advertises all four tools, and a call made
//! with no application running answers with the matching
//! [`super::instances::InstanceError`] sentence, which says what to do.

use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, ServerCapabilities,
    ToolsCapability,
};
use serde_json::{json, Value};

use super::tools;
use super::wire::ToolAnswer;

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "code-basics-roslyn";

/// This build's version, taken from the crate rather than repeated.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a client is told before it reads a single tool description.
///
/// Rules a model cannot infer and will otherwise get wrong: this is the semantic
/// model rather than a text search, it can abstain (and the abstention is not a
/// zero), a location is a position *or* a symbol, and the answer is scoped to the
/// one workspace this server was installed for.
pub const INSTRUCTIONS: &str = "These tools answer semantic questions about the code in one \
workspace, from the language server the code-basics application already keeps warm for it — the \
same engine behind the editor's find-usages, diagnostics, type hierarchy and signature help. They \
are not a text search: a result is what the compiler's model says, not what a string match \
guesses.

Three things to know. Every tool can abstain, and an abstention is not an empty answer: \"the \
server is still loading its projects\", \"this language's server does not support this question\" \
and \"no usages exist\" are different replies with different codes, and only the last is a real \
zero. Give a location as either an exact position (line, 1-based; character, 0-based UTF-16) or a \
symbol name — when a name matches more than one declaration the matches are listed rather than \
guessed at, so pass a position to choose. And the answers are scoped to the workspace this server \
was installed for; it cannot see another repository.";

/// The `initialize` result. Consults nothing — see the module docs.
pub fn initialize_result(requested: Option<&str>) -> InitializeResult {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());

    InitializeResult::new(capabilities)
        .with_protocol_version(crate::mcp::serve::negotiated_version(requested))
        .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
        .with_instructions(INSTRUCTIONS)
}

/// The `tools/list` result. Advertises every tool whether or not an application
/// is running: a tool that appears only sometimes is one a model will never learn
/// to use.
pub fn tools_list_result() -> Value {
    json!({ "tools": tools::descriptors() })
}

/// Turn the application's answer into an MCP result.
///
/// A refusal is a **tool execution error**, never a protocol error: the
/// specification asks a client to hand those to the model so it can correct
/// itself, and every refusal here has a real correction — open the repository,
/// name a window, wait for the server to load.
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

/// A tool name this server does not have, answered by the **shim** before it
/// looks for a running application.
///
/// *No such tool* and *no application to reach* are different answers with
/// different fixes, and the no-application case is the one a client hits most. The
/// wording is [`tools::unknown_tool_sentence`], shared with the application's own
/// refusal so the two cannot drift.
pub fn unknown_tool_answer(tool: &str) -> ToolAnswer {
    ToolAnswer::refused("unknown_tool", tools::unknown_tool_sentence(tool))
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
