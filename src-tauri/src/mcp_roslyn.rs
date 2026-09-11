//! The `mcp-roslyn` mode: this executable as an MCP stdio server that forwards
//! semantic questions to a *running* application over a named pipe.
//!
//! The sixth thing this binary can be, after the application, the intent recorder,
//! the quality gate, the SQL MCP server, the browser MCP server and the Tasks MCP
//! server — and like those it re-invokes itself rather than shipping a second
//! artifact ([`cb_core::roslyn::argv`]).
//!
//! **Everything it decides lives in [`cb_core::roslyn`]**: which application to
//! talk to and the five reasons there may be none (`instances`), what a request
//! and a reply look like (`wire`), the four tools and their schemas (`tools`), the
//! handshake (`serve`). What is left here is I/O — read stdin, connect a pipe,
//! write a line, wait — and it is here precisely because none of it is reachable
//! from a test.
//!
//! # It is the browser server's twin, not the SQL server's
//!
//! Like `mcp_browser`, this server answers nothing itself: the semantic model
//! lives in a warm session in another process, so every call is forwarded and the
//! reply is the application's own words. The one structural difference is the
//! boundary — the browser resolves the *active* window, this one resolves the
//! `--workspace` the entry was installed with, so an agent configured for repo X
//! reaches repo X regardless of which window is focused.
//!
//! # Nothing may reach stdout except MCP bytes
//!
//! stdout **is** the transport. A single stray line desynchronises the client, so
//! this mode installs no `tracing` subscriber and every diagnostic goes to stderr.
//!
//! # A connection per call, and a generous deadline
//!
//! The registry is re-read per call, so an application started after this server
//! did is found and one that exited is reported as gone rather than answered from
//! a stale handle. [`TOOL_TIMEOUT`] is deliberately generous: Roslyn `references`
//! on a large solution is legitimately slow, and a timeout is its own answer
//! ([`cb_core::roslyn::wire::PipeFailure::Timeout`]), never folded into "not
//! running".

use std::time::Duration;

use cb_core::lsp::jsonrpc::{self, Incoming, RequestId};
use cb_core::mcp::{ndjson, serve as mcp_serve};
use cb_core::roslyn::answer::RoslynRefusal;
use cb_core::roslyn::instances::{self, InstanceError};
use cb_core::roslyn::wire::{self, PipeFailure, ToolAnswer};
use cb_core::roslyn::{argv, liveness, serve, tools};
use cb_core::tool_gate;
use serde_json::Value;
use tokio::io::AsyncReadExt;

/// How long the application has to answer one tool call.
///
/// 60 s, three times the browser's: `find_references` on a large solution is
/// legitimately slow, and an agent waiting is better than an agent told the wrong
/// thing.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// The per-tool gate, re-read on every use. See [`cb_core::tool_gate`].
fn load_gate() -> tool_gate::ToolGateFile {
    tool_gate::load(&tool_gate::mcp_tools_path())
}

/// Did the command line ask for the Roslyn MCP server rather than the application?
pub fn is_mcp_roslyn_invocation() -> bool {
    argv::is_mcp_roslyn_invocation(&std::env::args().collect::<Vec<_>>())
}

/// Serve until stdin closes, then exit.
pub fn run() -> ! {
    let args: Vec<String> = std::env::args().collect();
    let Some(invocation) = argv::parse_mcp_roslyn_args(&args) else {
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

    runtime.block_on(serve_stdio(
        invocation.workspace.as_deref(),
        invocation.instance.as_deref(),
    ));
    // The specification asks a server to exit promptly once its input closes.
    std::process::exit(0)
}

/// The read loop.
async fn serve_stdio(workspace: Option<&str>, instance: Option<&str>) {
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
        let lines = match decoder.push(&chunk[..read]) {
            Ok(lines) => lines,
            Err(error) => {
                eprintln!("code-basics {}: {error}", argv::SUBCOMMAND);
                return;
            }
        };
        for line in lines {
            if let Some(response) = answer_line(line, workspace, instance).await {
                write_line(&response);
            }
        }
    }
}

/// The response one line calls for, or [`None`] when it calls for none.
async fn answer_line(
    line: ndjson::Line,
    workspace: Option<&str>,
    instance: Option<&str>,
) -> Option<Value> {
    let message = match line {
        ndjson::Line::Malformed { error, .. } => return Some(mcp_serve::parse_failure(&error)),
        ndjson::Line::Message(message) => message,
    };

    let bytes = serde_json::to_vec(&message).ok()?;
    match jsonrpc::classify(&bytes) {
        Err(error) => Some(mcp_serve::parse_failure(&error.to_string())),
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
            Some(answer_request(&id, &method, params.as_ref(), workspace, instance).await)
        }
    }
}

async fn answer_request(
    id: &RequestId,
    method: &str,
    params: Option<&Value>,
    workspace: Option<&str>,
    instance: Option<&str>,
) -> Value {
    let route = match mcp_serve::route(method, params) {
        Ok(route) => route,
        Err(error) => return mcp_serve::failure(id, &error),
    };

    match route {
        mcp_serve::Route::Initialize { requested } => {
            match serde_json::to_value(serve::initialize_result(requested.as_deref())) {
                Ok(value) => mcp_serve::success(id, value),
                Err(error) => mcp_serve::failure(
                    id,
                    &rmcp::model::ErrorData::internal_error(error.to_string(), None),
                ),
            }
        }
        mcp_serve::Route::ToolsList => {
            mcp_serve::success(id, serve::tools_list_result(&load_gate()))
        }
        mcp_serve::Route::Ping => mcp_serve::success(id, Value::Object(Default::default())),
        mcp_serve::Route::ToolsCall { name, arguments } => {
            // The arguments are **not** parsed here. The application parses them,
            // because it is the side that owns the tool table. The name is
            // forwarded as sent.
            let arguments = arguments.map(Value::Object).unwrap_or(Value::Null);
            let answer = call_tool(&name, arguments, workspace, instance).await;
            match serde_json::to_value(serve::answer_result(&answer)) {
                Ok(value) => mcp_serve::success(id, value),
                Err(error) => mcp_serve::failure(
                    id,
                    &rmcp::model::ErrorData::internal_error(error.to_string(), None),
                ),
            }
        }
    }
}

/// Forward one tool call to the application, or answer why it could not be.
async fn call_tool(
    tool: &str,
    arguments: Value,
    workspace: Option<&str>,
    hint: Option<&str>,
) -> ToolAnswer {
    if !cfg!(windows) {
        return unsupported();
    }

    // The name is checked here, before any registry is read, so an unknown tool
    // is not answered by "start code-basics and retry" about a tool that will
    // never exist. Only the name: the application still owns the tool table.
    if !tools::is_known(tool) {
        return serve::unknown_tool_answer(tool);
    }

    // A disabled-but-known tool is refused before a registry is read, re-reading
    // the gate every call. See `cb_core::tool_gate`.
    if !load_gate().is_enabled(tool_gate::ServerId::Roslyn, tool) {
        return serve::disabled_tool_answer(tool);
    }

    // The `--workspace` scope *is* the boundary. An unscoped install reaches no
    // session, so it is refused here — before a registry is read — rather than
    // guessing a directory.
    let Some(workspace) = workspace.map(str::trim).filter(|w| !w.is_empty()) else {
        return ToolAnswer::refused(
            RoslynRefusal::NoWorkspace.code(),
            RoslynRefusal::NoWorkspace.sentence(),
        );
    };

    let path = instances::instances_path();
    let file = instances::load(&path);
    let instance = match instances::choose_instance(&file, workspace, hint, &liveness::alive) {
        Ok(instance) => instance.clone(),
        Err(error) => return refusal(&error),
    };

    let request = wire::Request {
        protocol: instances::PROTOCOL_VERSION,
        token: instance.listener.token.clone(),
        workspace: workspace.to_string(),
        tool: tool.to_string(),
        arguments,
    };

    // The pipe a client opens is derived from the verified pid, never the string
    // the registry stated — `choose_instance` already refused a stated name that
    // disagrees ([`InstanceError::PipeNameMismatch`]).
    let pipe = instances::pipe_name(instance.pid);
    match exchange(&pipe, &request).await {
        Ok(answer) => answer,
        Err(failure) => failure.answer(),
    }
}

fn refusal(error: &InstanceError) -> ToolAnswer {
    ToolAnswer::refused(error.code(), error.sentence())
}

/// The answer on a platform with no named pipes — kept so the module compiles
/// everywhere the way the browser server does.
fn unsupported() -> ToolAnswer {
    ToolAnswer::refused(
        "unsupported",
        format!(
            "The Roslyn MCP server reaches the running application over a Windows named pipe; \
             {} has no such facility, so there is nothing to forward to.",
            std::env::consts::OS
        ),
    )
}

/// One request, one reply, under a deadline.
#[cfg(windows)]
async fn exchange(pipe: &str, request: &wire::Request) -> Result<ToolAnswer, PipeFailure> {
    use tokio::io::AsyncWriteExt;

    let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(pipe)
        .map_err(|error| PipeFailure::NotConnected {
            pipe: pipe.to_string(),
            detail: error.to_string(),
        })?;

    let encoded = ndjson::encode(&wire::request_value(1, request));
    let tool = request.tool.clone();
    let exchange = async {
        client
            .write_all(&encoded)
            .await
            .map_err(|_| PipeFailure::Closed { tool: tool.clone() })?;

        let mut decoder = ndjson::LineDecoder::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = client
                .read(&mut chunk)
                .await
                .map_err(|_| PipeFailure::Closed { tool: tool.clone() })?;
            if read == 0 {
                return Err(PipeFailure::Closed { tool: tool.clone() });
            }
            let lines = decoder
                .push(&chunk[..read])
                .map_err(|error| PipeFailure::Malformed {
                    detail: error.to_string(),
                })?;
            // A reply is exactly one frame, so the first complete line ends the
            // exchange. An incomplete read yields no line and the outer loop reads
            // again, which is why this cannot be hoisted out of it.
            if let Some(line) = lines.into_iter().next() {
                return match line {
                    ndjson::Line::Message(message) => wire::parse_answer(&message),
                    ndjson::Line::Malformed { error, .. } => {
                        Err(PipeFailure::Malformed { detail: error })
                    }
                };
            }
        }
    };

    match tokio::time::timeout(TOOL_TIMEOUT, exchange).await {
        Ok(outcome) => outcome,
        Err(_) => Err(PipeFailure::Timeout {
            tool: request.tool.clone(),
            ms: TOOL_TIMEOUT.as_millis() as u64,
        }),
    }
}

/// The pipe does not exist off Windows. Kept so the whole module compiles there —
/// see [`unsupported`], which is what actually answers.
#[cfg(not(windows))]
async fn exchange(pipe: &str, _request: &wire::Request) -> Result<ToolAnswer, PipeFailure> {
    Err(PipeFailure::NotConnected {
        pipe: pipe.to_string(),
        detail: format!(
            "named pipes are a Windows facility; {} cannot reach the application",
            std::env::consts::OS
        ),
    })
}

/// One line to stdout. The **only** thing in this mode that writes there.
fn write_line(response: &Value) {
    use std::io::Write;

    let encoded = ndjson::encode(response);
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&encoded).is_err() || stdout.flush().is_err() {
        eprintln!(
            "code-basics {}: the client closed its input.",
            argv::SUBCOMMAND
        );
    }
}

#[cfg(test)]
#[path = "mcp_roslyn_tests.rs"]
mod tests;
