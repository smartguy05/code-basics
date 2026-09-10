//! The `mcp-tasks` mode: this executable as the Tasks MCP stdio server.
//!
//! The fifth thing this binary can be, after the application, the intent
//! recorder, the quality gate, the SQL MCP server and the browser MCP server —
//! and like those it re-invokes itself rather than shipping a second artifact
//! ([`cb_core::tasks::mcp::argv`]).
//!
//! **Everything it decides lives in [`cb_core::tasks::mcp`]**, which is unit
//! tested with no client and no filesystem: what a version negotiation answers,
//! which method is which, what a refusal says, and how a tool call changes the
//! store are all there ([`cb_core::tasks::mcp::execute`]). What is left here is
//! I/O — read stdin, dispatch, load the store, apply, save, write a line — and
//! it is here precisely because none of it is reachable from a test.
//!
//! This mirrors `mcp_sql.rs` closely; the differences are the point:
//!
//! - **It writes.** A mutating tool call
//!   ([`cb_core::tasks::mcp::execute::Outcome::mutated`]) is persisted with
//!   [`cb_core::tasks::save`] before the answer goes back. The SQL server never
//!   writes anything.
//! - **The store is re-loaded per call**, for the same reason the SQL server
//!   re-reads consent per call: the human may be editing the same list in the
//!   floating panel, and a cached copy would clobber their edits on the next
//!   save. Load, apply, save — no in-memory cache.
//! - **`--workspace` is mandatory in effect.** The store is per-workspace, so an
//!   unscoped invocation has nothing to reach and every tool answers
//!   [`cb_core::tasks::mcp::answer::McpRefusal::NoWorkspace`].
//!
//! # Nothing may reach stdout except MCP bytes
//!
//! stdout **is** the transport. A single stray line desynchronises the client,
//! so this mode installs no `tracing` subscriber (the default writes to stdout)
//! and every diagnostic goes to stderr, which the specification reserves for it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cb_core::lsp::jsonrpc::{self, Incoming, RequestId};
use cb_core::mcp::ndjson;
use cb_core::tasks::mcp::answer::McpRefusal;
use cb_core::tasks::mcp::tools::ToolCall;
use cb_core::tasks::mcp::{argv, execute, render, serve, tools};
use cb_core::tasks::{self, TasksFile};
use rmcp::model::CallToolResult;
use serde_json::Value;
use tokio::io::AsyncReadExt;

/// Did the command line ask for the Tasks MCP server rather than the
/// application?
pub fn is_mcp_tasks_invocation() -> bool {
    argv::is_mcp_tasks_invocation(&std::env::args().collect::<Vec<_>>())
}

/// Serve until stdin closes, then exit.
///
/// A **current-thread** runtime: this process serves one client and touches one
/// small file at a time, so a multi-thread runtime's worker pool would sit idle
/// for the life of an agent session.
pub fn run() -> ! {
    let args: Vec<String> = std::env::args().collect();
    let Some(invocation) = argv::parse_mcp_tasks_args(&args) else {
        eprintln!("code-basics: not an {} invocation.", argv::SUBCOMMAND);
        std::process::exit(2);
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("code-basics {}: {error}", argv::SUBCOMMAND);
            std::process::exit(1);
        }
    };

    let workspace = invocation.workspace.map(PathBuf::from);
    runtime.block_on(serve_stdio(workspace.as_deref()));
    // The specification asks a server to exit promptly once its input closes.
    std::process::exit(0)
}

/// The read loop. Line framing is reused from the SQL server
/// ([`cb_core::mcp::ndjson`]) — it is generic to any stdio MCP server.
async fn serve_stdio(workspace: Option<&Path>) {
    let mut decoder = ndjson::LineDecoder::new();
    let mut stdin = tokio::io::stdin();
    let mut chunk = [0u8; 8192];

    loop {
        let read = match stdin.read(&mut chunk).await {
            Ok(0) => return,
            Ok(count) => count,
            Err(error) => {
                eprintln!("code-basics {}: stdin: {error}", argv::SUBCOMMAND);
                return;
            }
        };

        // A malformed *line* is one bad message and the loop continues; an
        // `Err` is the stream itself, and where the next message begins is no
        // longer knowable. See `cb_core::mcp::ndjson`.
        let lines = match decoder.push(&chunk[..read]) {
            Ok(lines) => lines,
            Err(error) => {
                eprintln!("code-basics {}: {error}", argv::SUBCOMMAND);
                return;
            }
        };

        for line in lines {
            if let Some(response) = answer_line(line, workspace).await {
                write_line(&response);
            }
        }
    }
}

/// The response one line calls for, or [`None`] when it calls for none.
async fn answer_line(line: ndjson::Line, workspace: Option<&Path>) -> Option<Value> {
    let message = match line {
        ndjson::Line::Malformed { error, .. } => return Some(serve::parse_failure(&error)),
        ndjson::Line::Message(message) => message,
    };

    let bytes = serde_json::to_vec(&message).ok()?;
    match jsonrpc::classify(&bytes) {
        Err(error) => Some(serve::parse_failure(&error.to_string())),
        // A notification must never be answered — a reply addressed to nothing.
        Ok(Incoming::Notification { .. }) => None,
        // This server sends no requests, so nothing can be answering one.
        Ok(Incoming::Response { .. }) => {
            eprintln!(
                "code-basics {}: ignoring a response to a request this server never sent.",
                argv::SUBCOMMAND
            );
            None
        }
        Ok(Incoming::Request { id, method, params }) => {
            Some(answer_request(&id, &method, params.as_ref(), workspace).await)
        }
    }
}

async fn answer_request(
    id: &RequestId,
    method: &str,
    params: Option<&Value>,
    workspace: Option<&Path>,
) -> Value {
    let route = match serve::route(method, params) {
        Ok(route) => route,
        Err(error) => return serve::failure(id, &error),
    };

    match route {
        serve::Route::Initialize { requested } => {
            let result = serve::initialize_result(requested.as_deref());
            match serde_json::to_value(result) {
                Ok(value) => serve::success(id, value),
                Err(error) => serve::failure(
                    id,
                    &rmcp::model::ErrorData::internal_error(error.to_string(), None),
                ),
            }
        }
        serve::Route::ToolsList => serve::success(id, serve::tools_list_result()),
        serve::Route::Ping => serve::success(id, Value::Object(Default::default())),
        serve::Route::ToolsCall { name, arguments } => {
            match tools::parse_call(&name, arguments.as_ref()) {
                Err(error) => serve::failure(id, &error),
                Ok(call) => {
                    let result = call_tool(call, workspace);
                    match serde_json::to_value(result) {
                        Ok(value) => serve::success(id, value),
                        Err(error) => serve::failure(
                            id,
                            &rmcp::model::ErrorData::internal_error(error.to_string(), None),
                        ),
                    }
                }
            }
        }
    }
}

/// Run one tool call. A refusal is a **tool execution error**, never a protocol
/// one — see [`cb_core::tasks::mcp::tools`].
fn call_tool(call: ToolCall, workspace: Option<&Path>) -> CallToolResult {
    match dispatch(call, workspace) {
        Ok(text) => tools::text_result(text),
        Err(refusal) => tools::refusal_result(&refusal),
    }
}

/// Load the store, apply the call, persist it if it mutated, and render the
/// answer. The store is re-read on **every** call — see the module docs.
fn dispatch(call: ToolCall, workspace: Option<&Path>) -> Result<String, McpRefusal> {
    // No workspace means no per-workspace store to reach. Refused rather than
    // guessing a directory.
    let root = workspace.ok_or(McpRefusal::NoWorkspace)?;
    let path = tasks::tasks_path(root);
    let mut file: TasksFile = tasks::load(&path);

    let outcome = execute::apply(&mut file, call, now_ms(), &new_id())?;

    if outcome.mutated() {
        // The filesystem's own error is dropped, not forwarded — see
        // `McpRefusal::StoreWriteFailed`.
        tasks::save(&path, &file).map_err(|error| {
            eprintln!("code-basics {}: save: {error}", argv::SUBCOMMAND);
            McpRefusal::StoreWriteFailed
        })?;
    }

    Ok(render::outcome(&outcome))
}

/// The wall clock, in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A fresh task id. A uuid so two agents (or an agent and the panel) creating a
/// task in the same millisecond cannot collide.
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Write one message. stdout is the transport and nothing else may touch it.
fn write_line(message: &Value) {
    let bytes = ndjson::encode(message);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    if let Err(error) = handle.write_all(&bytes).and_then(|()| handle.flush()) {
        eprintln!("code-basics {}: stdout: {error}", argv::SUBCOMMAND);
    }
}
