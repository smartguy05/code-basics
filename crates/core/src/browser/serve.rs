//! The browser MCP server's handshake, and the shape of every answer it sends.
//!
//! The runner in the app crate (`src-tauri/src/mcp_browser.rs`) is I/O and
//! nothing else, for the reason [`crate::mcp::serve`] gives about its own
//! runner: a binary that reads stdin and opens a named pipe is not reachable
//! from a test. So everything between "a line arrived" and "a line goes back"
//! that is a *choice* lives here or in [`super::tools`], [`super::wire`],
//! [`super::instances`] and [`super::render`].
//!
//! # Routing is not re-implemented
//!
//! [`crate::mcp::serve::route`], `success`, `failure`, `parse_failure` and
//! `negotiated_version` are about **JSON-RPC and MCP**, not about SQL — they
//! name no connection and no engine — so this server uses them unchanged.
//! Writing a second `initialize`/`tools/list`/`tools/call` recogniser would be
//! the "two implementations of one distinction" that [`crate::lsp::jsonrpc`]'s
//! docs specifically warn against, and the two would drift on exactly the
//! things that are easy to get wrong (a notification must not be answered; a
//! string id is not a numeric one).
//!
//! Only three things here are this server's own: who it says it is, what it
//! tells a model before its first call, and how a [`super::wire::ToolAnswer`]
//! becomes a `CallToolResult`.
//!
//! # The lifetime rule: this server starts whether or not the application does
//!
//! A server that fails `initialize` is dropped **silently** by its host and the
//! user never learns why — they see an agent that simply cannot see their
//! browser. So nothing here consults [`super::instances`]: the handshake
//! succeeds, `tools/list` advertises all thirteen tools, and a call made with
//! no application running answers with
//! [`super::instances::InstanceError::NoneRunning`]'s sentence, which says what
//! to do. The application's absence is a *tool answer*, never a startup
//! failure, and that is pinned by
//! `the_handshake_consults_no_registry_and_lists_every_tool_regardless`.

use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, ServerCapabilities,
    ToolsCapability,
};
use serde_json::{json, Value};

use super::tools;
use super::wire::ToolAnswer;

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "code-basics-browser";

/// This build's version, taken from the crate rather than repeated.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a client is told before it reads a single tool description.
///
/// Four rules a model cannot infer and will otherwise get wrong, and each one
/// costs something real if it does: it will read a loading page as a finished
/// one, it will retry a refusal that only a human can lift, it will report a
/// partial network list as a complete one, and it will assume a permission
/// carries across a navigation.
pub const INSTRUCTIONS: &str = "These tools look at the web page in the code-basics application's \
browser panel — the page the user is looking at, in their own logged-in session. They are not a \
headless browser and they do not open pages of their own.

Four things to know before the first call. Call browser_status first: it always answers, and it \
distinguishes no application running, the plugin switched off, no panel open, a blank panel, a \
page still loading, a page ready, and a failed navigation — these have different fixes and only \
one of them licenses a read. Everything except browser_status needs the user's permission, \
granted per page in the panel and never remembered: a refusal for want of permission is a \
request for the user to click something, so ask them and do not retry. The state-changing tools \
(navigate, click, type, press key, back, forward, reload) need the stronger \"read and control\" \
permission, because they change what the user is looking at. And the console and network tools \
report what page-side instrumentation can see, which is less than a browser's developer tools: \
each answer states its own coverage, and an empty list is not evidence that nothing happened.

There is deliberately no tool that evaluates JavaScript in the page. Use browser_read_page to \
find an element and browser_click, browser_type or browser_press_key to act on it by reference.";

/// The `initialize` result. Consults nothing — see the module docs.
pub fn initialize_result(requested: Option<&str>) -> InitializeResult {
    // Declared field by field on a default rather than through rmcp's builder,
    // which lives behind the `server` feature this crate deliberately does not
    // take. Only the capability this server has is named.
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());

    InitializeResult::new(capabilities)
        .with_protocol_version(crate::mcp::serve::negotiated_version(requested))
        .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
        .with_instructions(INSTRUCTIONS)
}

/// The `tools/list` result. Advertises every tool whether or not an application
/// is running: a tool that appears only sometimes is one a model will never
/// learn to use.
pub fn tools_list_result(gate: &crate::tool_gate::ToolGateFile) -> Value {
    json!({
        "tools": crate::tool_gate::filter_descriptors(
            gate,
            crate::tool_gate::ServerId::Browser,
            tools::descriptors(),
        )
    })
}

/// A disabled-but-known tool, refused by the **shim** before it looks for a
/// running application — the disabled-tool sibling of [`unknown_tool_answer`].
pub fn disabled_tool_answer(tool: &str) -> ToolAnswer {
    ToolAnswer::refused(
        crate::tool_gate::DISABLED_CODE,
        crate::tool_gate::disabled_tool_sentence(tool),
    )
}

/// Turn the application's answer into an MCP result.
///
/// A refusal is a **tool execution error**, never a protocol error, for the
/// reason [`crate::mcp::tools::refusal_result`] gives: the specification asks a
/// client to hand those to the model so it can correct itself, and every
/// refusal here has a real correction — ask the user to click Allow, ask them to
/// open the panel, ask which window, wait for the page to load.
///
/// The code leads the text so a model can branch without parsing prose, and is
/// also carried structurally.
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

/// The answer on a platform where the control pipe does not exist.
///
/// The panel is a WebView2 child window and the control pipe is a named pipe, so the pipe
/// module is `#[cfg(windows)]` — but this server still **builds and starts**
/// everywhere, for the same reason it starts with no application running: a
/// server that fails to initialize is dropped silently by its host and the user
/// never learns why. So every tool answers, and answers with this.
///
/// It names the platform rather than saying "unavailable", because *this build
/// cannot do it here* and *your application is not running* are different
/// things and only one of them can be fixed by opening a panel.
pub fn unsupported_answer(platform: &str) -> ToolAnswer {
    ToolAnswer::refused(
        "unsupported_platform",
        format!(
            "The code-basics browser panel is a Windows-only feature (it embeds a WebView2 child window), and this MCP server is running on {platform}. Nothing was read and there is nothing to open: no build of code-basics on {platform} has a browser panel for these tools to look at."
        ),
    )
}

/// A tool name this server does not have.
///
/// Answered by the **shim**, before it looks for a running application, and
/// that ordering is the point: `call_tool` forwards the name it was sent, so an
/// unknown name used to be answered "No code-basics application is running" —
/// which tells an agent to start the application and retry a tool that will
/// never exist. *No such tool* and *no browser to reach* are different answers
/// with different fixes, and the no-application case is the one a client hits
/// most often. The wording is [`tools::unknown_tool_sentence`], shared with the
/// application's own refusal so the two cannot drift apart.
pub fn unknown_tool_answer(tool: &str) -> ToolAnswer {
    ToolAnswer::refused("unknown_tool", tools::unknown_tool_sentence(tool))
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
