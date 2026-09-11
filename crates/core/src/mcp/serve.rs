//! What the server answers, decided without touching a database.
//!
//! The runner in the app crate (`src-tauri/src/mcp_sql.rs`) is I/O and nothing
//! else, for the reason the crate docs give: a binary that reads stdin and
//! spawns drivers is not reachable from a test. So everything between "a line
//! arrived" and "a line goes back" that is a *choice* lives here, and the
//! runner's job is to read, dispatch, connect, and write.
//!
//! Three groups, and each is here because the runner would otherwise be the
//! place it was decided:
//!
//! 1. **The handshake.** Which protocol version this server answers with, and
//!    what it declares it can do. Never a literal date: [`negotiated_version`]
//!    echoes a version the SDK knows and otherwise answers with
//!    [`ProtocolVersion::LATEST`], so a spec release moves this build without
//!    an edit here.
//! 2. **Routing.** Which of the five methods a request is, and the two — and
//!    only two — ways a request can be a *protocol* error. See
//!    [`crate::mcp::tools`] for why every other failure is a tool execution
//!    error instead.
//! 3. **The two answers that need no connection.** `sql.list_connections` is
//!    read out of the store, and a catalog result is turned into rows or into
//!    the refusal that says why it could not be.
//!
//! # The scope is not an absence of consent
//!
//! [`list_connections_answer`] refuses an empty listing — and a listing left
//! empty by `--workspace` is **not** one of those refusals. Saying "nothing is
//! exposed" there would send the reader to a toggle that is already on, when
//! the real answer is that this server was started scoped. The scoped-out count
//! is an answer, not an absence.

use std::path::Path;

use rmcp::model::{
    ErrorCode, ErrorData, Implementation, InitializeResult, JsonObject, ProtocolVersion,
    ServerCapabilities, ToolsCapability,
};
use serde_json::{json, Value};

use super::answer::McpRefusal;
use super::expose;
use super::render;
use super::tools;
use crate::lsp::jsonrpc::RequestId;
use crate::sql::catalog::{self, SqlColumn, SqlObject};
use crate::sql::model::SqlResultSet;
use crate::sql::store::SqlConnectionsFile;

/// The method names this server answers.
pub const INITIALIZE: &str = "initialize";
pub const TOOLS_LIST: &str = "tools/list";
pub const TOOLS_CALL: &str = "tools/call";
pub const PING: &str = "ping";

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "code-basics-sql";

/// This build's version, taken from the crate rather than repeated, so a
/// release cannot report the previous one.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a client is told before it reads a single tool description.
///
/// Both sentences are rules a model has to know to use this server correctly
/// and cannot infer: that read-only is forced rather than requested, and that a
/// connection it cannot see is a choice the user made rather than a missing
/// database. They are repeated on each tool description too — a client may show
/// one without the other.
pub const INSTRUCTIONS: &str = "These tools read the databases the user has explicitly exposed to \
agents in the code-basics SQL console. Exposure is per connection and off by default: a database \
that is not listed is one the user chose not to share, not one that is missing. Every statement \
is forced read-only — there is no argument, and no setting on a connection, that lifts that — so \
do not plan a write and do not ask the user to enable one here. Call sql.read_only_enforcement \
before relying on the words read-only: for one engine a write executes inside a transaction that \
is rolled back rather than being refused.";

// ---------------------------------------------------------------------------
// The handshake
// ---------------------------------------------------------------------------

/// The version to answer `initialize` with.
///
/// A version this SDK knows is echoed, which is what the specification asks
/// for. Anything else — including an absent one — is answered with this build's
/// latest rather than refused: a client that named a version nobody has heard
/// of can still speak the current one, and refusing the session outright would
/// turn a version disagreement into no server at all.
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
    // Declared one field at a time on a default rather than through rmcp's
    // builder, which lives behind the `server` feature this crate deliberately
    // does not take (the model types are all it wants). The rule is the same
    // either way: only the capability this server actually has is named, so
    // nothing can be advertised by inheriting somebody else's default.
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());

    InitializeResult::new(capabilities)
        .with_protocol_version(negotiated_version(requested))
        .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
        .with_instructions(INSTRUCTIONS)
}

/// The `tools/list` result, with the tools the user has switched off removed.
pub fn tools_list_result(gate: &crate::tool_gate::ToolGateFile) -> Value {
    json!({
        "tools": crate::tool_gate::filter_descriptors(
            gate,
            crate::tool_gate::ServerId::Sql,
            tools::descriptors(),
        )
    })
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// One request, already recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Initialize {
        /// The version string the client asked for, exactly as it sent it.
        /// [`None`] when it sent none — which is not the same as an unknown
        /// one, even though [`negotiated_version`] answers both the same way.
        requested: Option<String>,
    },
    ToolsList,
    ToolsCall {
        name: String,
        /// [`None`] means the client sent no arguments at all. An empty object
        /// is a set of arguments that happens to be empty, and
        /// [`tools::parse_call`] treats them the same — but this layer does not
        /// invent one, because that would be this layer answering a question
        /// the client did not.
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

/// A successful response.
///
/// The id is echoed **in the shape it arrived** — [`RequestId`] keeps a numeric
/// and a string id apart even when they print the same, which is what stops one
/// response resolving another request's waiter on the client's side.
pub fn success(id: &RequestId, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A failed response. Carries `error` and no `result` key at all: a response
/// with both is out of spec and a client is entitled to believe either.
pub fn failure(id: &RequestId, error: &ErrorData) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// The answer to a line that was not JSON, which therefore has no id to echo.
///
/// A null id is the specification's own answer for this case. It is reported
/// rather than swallowed for the reason [`crate::mcp::ndjson`] reports a
/// malformed line rather than dropping it: silence leaves a request looking
/// like one that was never sent.
pub fn parse_failure(detail: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": ErrorData::parse_error(format!("The line was not valid JSON: {detail}"), None),
    })
}

// ---------------------------------------------------------------------------
// The answers that need no connection
// ---------------------------------------------------------------------------

/// `sql.list_connections`, straight out of the store.
///
/// An empty listing is a refusal, and *which* refusal is the point: nothing
/// saved and nothing exposed are fixed by different actions. A listing emptied
/// by the `--workspace` scope is neither, and is answered rather than refused —
/// see the module docs.
pub fn list_connections_answer(
    file: &SqlConnectionsFile,
    workspace: Option<&Path>,
) -> Result<String, McpRefusal> {
    let listed = expose::listing(file, workspace);
    if listed.connections.is_empty() && listed.scoped_out == 0 {
        return Err(expose::empty_listing_refusal(file));
    }
    Ok(render::connections(&listed))
}

/// The tables in a catalog result, or the refusal that says why there are none
/// to report.
///
/// The cap is checked **first**. [`catalog::table_objects`] refuses a capped
/// result too, but as a sentence, and reading a truncation as an unreadable row
/// would name the wrong cause — the rows it did return were perfectly readable.
pub fn catalog_objects(result: &SqlResultSet) -> Result<Vec<SqlObject>, McpRefusal> {
    truncation(result)?;
    catalog::table_objects(result).map_err(|detail| McpRefusal::CatalogRowUnreadable { detail })
}

/// One table's columns, under the same two rules as [`catalog_objects`].
pub fn catalog_columns(result: &SqlResultSet) -> Result<Vec<SqlColumn>, McpRefusal> {
    truncation(result)?;
    catalog::table_columns(result).map_err(|detail| McpRefusal::CatalogRowUnreadable { detail })
}

fn truncation(result: &SqlResultSet) -> Result<(), McpRefusal> {
    match result.row_cap {
        Some(cap) => Err(McpRefusal::CatalogTruncated { cap }),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
