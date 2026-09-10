//! The handshake, the routing, and the response envelopes.
//!
//! The runner in the app crate (`src-tauri/src/mcp_tasks.rs`) is I/O and nothing
//! else, for the reason the crate docs give: a binary that reads stdin and
//! writes a file is not reachable from a test. So everything between "a line
//! arrived" and "a line goes back" that is a *choice* lives here (and in
//! [`super::execute`]), and the runner's job is to read, dispatch, load, save
//! and write.
//!
//! Copied in shape from [`crate::mcp::serve`], minus the SQL server's
//! "answers that need no connection": every Tasks answer needs the store, which
//! [`super::execute::apply`] reaches from the loaded file, so there is no
//! store-free answer to special-case here.

use rmcp::model::{
    ErrorCode, ErrorData, Implementation, InitializeResult, JsonObject, ProtocolVersion,
    ServerCapabilities, ToolsCapability,
};
use serde_json::{json, Value};

use super::tools;
use crate::lsp::jsonrpc::RequestId;

/// The method names this server answers.
pub const INITIALIZE: &str = "initialize";
pub const TOOLS_LIST: &str = "tools/list";
pub const TOOLS_CALL: &str = "tools/call";
pub const PING: &str = "ping";

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "code-basics-tasks";

/// This build's version, taken from the crate rather than repeated, so a
/// release cannot report the previous one.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a client is told before it reads a single tool description.
///
/// Two rules a model has to know and cannot infer: the list is **per-workspace
/// and personal** (a gitignored file, not shared with the team), and this
/// server both reads and writes it — unlike the SQL server, there is nothing to
/// warn is forbidden.
pub const INSTRUCTIONS: &str = "These tools read and write the task list for one workspace, kept \
in a personal, gitignored file (.code-basics/tasks.json). A task has a title, a longer body, a \
status (open or done) and an owner (me for the user, ai for you). You may create, update, \
complete, delete and reassign tasks. Assigning a task to ai only records the owner here; the app \
is what launches any work. The list is scoped to the workspace this server was installed for.";

// ---------------------------------------------------------------------------
// The handshake
// ---------------------------------------------------------------------------

/// The version to answer `initialize` with.
///
/// A version this SDK knows is echoed, which is what the specification asks
/// for. Anything else — including an absent one — is answered with this build's
/// latest rather than refused.
pub fn negotiated_version(requested: Option<&str>) -> ProtocolVersion {
    let Some(requested) = requested else {
        return ProtocolVersion::LATEST;
    };
    ProtocolVersion::KNOWN_VERSIONS
        .iter()
        .find(|known| known.as_str() == requested)
        .cloned()
        .unwrap_or(ProtocolVersion::LATEST)
}

/// The `initialize` result: the negotiated version, the one capability this
/// server has, who it is, and the rules a model needs before its first call.
pub fn initialize_result(requested: Option<&str>) -> InitializeResult {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());

    InitializeResult::new(capabilities)
        .with_protocol_version(negotiated_version(requested))
        .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
        .with_instructions(INSTRUCTIONS)
}

/// The `tools/list` result.
pub fn tools_list_result() -> Value {
    json!({ "tools": tools::descriptors() })
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// One request, already recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Initialize {
        /// The version string the client asked for, exactly as it sent it.
        requested: Option<String>,
    },
    ToolsList,
    ToolsCall {
        name: String,
        /// [`None`] means the client sent no arguments at all. An empty object
        /// is a set of arguments that happens to be empty; this layer does not
        /// invent one.
        arguments: Option<JsonObject>,
    },
    Ping,
}

/// Recognise a request, or produce the protocol error that says why it could
/// not be recognised.
pub fn route(method: &str, params: Option<&Value>) -> Result<Route, ErrorData> {
    match method {
        INITIALIZE => Ok(Route::Initialize {
            requested: params
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        TOOLS_LIST => Ok(Route::ToolsList),
        PING => Ok(Route::Ping),
        TOOLS_CALL => {
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ErrorData::invalid_params(
                        "tools/call requires \"name\" to be the string name of a tool.",
                        None,
                    )
                })?;
            Ok(Route::ToolsCall {
                name: name.to_string(),
                arguments: params
                    .and_then(|p| p.get("arguments"))
                    .and_then(Value::as_object)
                    .cloned(),
            })
        }
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            format!(
                "This server does not implement {unknown:?}. It answers: {INITIALIZE}, \
                 {TOOLS_LIST}, {TOOLS_CALL}, {PING}."
            ),
            None,
        )),
    }
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

/// A successful response. The id is echoed **in the shape it arrived**.
pub fn success(id: &RequestId, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A failed response. Carries `error` and no `result` key.
pub fn failure(id: &RequestId, error: &ErrorData) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// The answer to a line that was not JSON, which therefore has no id to echo.
pub fn parse_failure(detail: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": ErrorData::parse_error(format!("The line was not valid JSON: {detail}"), None),
    })
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
