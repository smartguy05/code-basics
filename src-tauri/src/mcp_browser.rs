//! The `mcp-browser` mode: this executable as an MCP stdio server that talks to
//! a *running* application over a named pipe.
//!
//! The fifth thing this binary can be, after the application, the intent
//! recorder, the quality gate and the SQL MCP server — and like those it
//! re-invokes itself rather than shipping a second artifact
//! ([`cb_core::browser::argv`]).
//!
//! **Everything it decides lives in [`cb_core::browser`]**: which application to
//! talk to and the five reasons there may be none (`instances`), what a request
//! and a reply look like and the four ways a call can fail to happen (`wire`),
//! the thirteen tools and their schemas (`tools`), the handshake (`serve`). What
//! is left here is I/O — read stdin, connect a pipe, write a line, wait — and it
//! is here precisely because none of it is reachable from a test.
//!
//! # It is not the SQL server's twin, and the difference is the point
//!
//! `mcp_sql` *is* the thing that answers: it opens a database and runs the
//! statement itself. This server answers nothing. The state it would need — what
//! page is open, whether the user granted permission — lives in a window in
//! another process, so every call is forwarded and the reply is the
//! application's own words ([`cb_core::browser::wire`] explains why the pipe
//! carries prose rather than state).
//!
//! # Nothing may reach stdout except MCP bytes
//!
//! stdout **is** the transport. A single stray line desynchronises the client,
//! and the failure looks like a server that hangs rather than one that is
//! broken. So this mode installs no `tracing` subscriber (the default one writes
//! to stdout) and every diagnostic goes to stderr, which the specification
//! reserves for it.
//!
//! # A connection per call, not per session
//!
//! The application's pipe exists only while a browser panel is open, so a
//! connection held for the life of an agent session would break the moment the
//! user closed the panel and would have to be rebuilt anyway. Connecting per
//! call also means the registry is re-read per call, so a panel that opened
//! after this server started is found — and one that closed is reported as
//! closed rather than answered from a stale handle.
//!
//! # Every request carries a deadline, and a timeout is its own answer
//!
//! [`cb_core::browser::wire::PipeFailure::Timeout`] is deliberately not folded
//! into "the browser is closed": the application is running and did not reply,
//! which usually means the page itself is wedged, and an agent told the browser
//! was closed would ask the user to open it.

use std::time::Duration;

use cb_core::browser::instances::{self, InstanceError};
use cb_core::browser::wire::{self, PipeFailure, ToolAnswer};
use cb_core::browser::{argv, liveness, serve, tools};
use cb_core::lsp::jsonrpc::{self, Incoming, RequestId};
use cb_core::mcp::{ndjson, serve as mcp_serve};
use cb_core::tool_gate;
use serde_json::Value;
use tokio::io::AsyncReadExt;

/// How long the application has to answer one tool call.
///
/// Generous, because a `browser_page_text` on a large document is legitimately
/// slow and an agent waiting is better than an agent told the wrong thing —
/// the `STATEMENT_TIMEOUT` reasoning from [`crate::mcp_sql`].
const TOOL_TIMEOUT: Duration = Duration::from_secs(20);

/// The per-tool gate, re-read on every use. See [`cb_core::tool_gate`].
fn load_gate() -> tool_gate::ToolGateFile {
    tool_gate::load(&tool_gate::mcp_tools_path())
}

/// Did the command line ask for the browser MCP server rather than the
/// application?
pub fn is_mcp_browser_invocation() -> bool {
    argv::is_mcp_browser_invocation(&std::env::args().collect::<Vec<_>>())
}

/// Serve until stdin closes, then exit.
///
/// A **current-thread** runtime: this process serves one client and holds one
/// pipe connection at a time, so a multi-thread runtime's worker pool would sit
/// idle for the life of an agent session.
pub fn run() -> ! {
    let args: Vec<String> = std::env::args().collect();
    let Some(invocation) = argv::parse_mcp_browser_args(&args) else {
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

    runtime.block_on(serve_stdio(invocation.instance.as_deref()));
    // The specification asks a server to exit promptly once its input closes.
    std::process::exit(0)
}

/// The read loop.
async fn serve_stdio(hint: Option<&str>) {
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
            if let Some(response) = answer_line(line, hint).await {
                write_line(&response);
            }
        }
    }
}

/// The response one line calls for, or [`None`] when it calls for none.
async fn answer_line(line: ndjson::Line, hint: Option<&str>) -> Option<Value> {
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
            Some(answer_request(&id, &method, params.as_ref(), hint).await)
        }
    }
}

async fn answer_request(
    id: &RequestId,
    method: &str,
    params: Option<&Value>,
    hint: Option<&str>,
) -> Value {
    let route = match mcp_serve::route(method, params) {
        Ok(route) => route,
        Err(error) => return mcp_serve::failure(id, &error),
    };

    match route {
        // Answers whether or not an application is running: a server that
        // fails to initialize is dropped silently by its host.
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
            // The arguments are **not** parsed here. The application parses
            // them, because it is the side that owns the tool table — and a
            // second parse here would be a second place a tool could be
            // reachable-but-unadvertised. The name is forwarded as sent.
            let arguments = arguments.map(Value::Object).unwrap_or(Value::Null);
            let answer = call_tool(&name, arguments, hint).await;
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
async fn call_tool(tool: &str, arguments: Value, hint: Option<&str>) -> ToolAnswer {
    if !cfg!(windows) {
        return serve::unsupported_answer(std::env::consts::OS);
    }

    // The name is checked here, before any registry is read, because otherwise
    // an unknown tool is answered by whatever the *application* lookup says —
    // and with no application running that is "start code-basics and retry",
    // about a tool that will never exist. Only the name: the application still
    // owns the tool table and parses the arguments.
    if !tools::is_known(tool) {
        return serve::unknown_tool_answer(tool);
    }

    // A disabled-but-known tool is refused before the pipe is opened, re-reading
    // the gate every call. See `cb_core::tool_gate`.
    if !load_gate().is_enabled(tool_gate::ServerId::Browser, tool) {
        return serve::disabled_tool_answer(tool);
    }

    let path = instances::instances_path();
    let file = instances::load(&path);
    let instance = match instances::choose_instance(&file, hint, &liveness::alive) {
        Ok(instance) => instance.clone(),
        Err(error) => return refusal(&error),
    };
    let Some(listener) = instance.listener.clone() else {
        // Unreachable through `choose_instance`, which only returns an instance
        // with a listener — and not assumed away, because the alternative to
        // this line is an `unwrap` on somebody else's invariant.
        return refusal(&InstanceError::PanelClosed { pid: instance.pid });
    };

    let request = wire::Request {
        protocol: instances::PROTOCOL_VERSION,
        token: listener.token.clone(),
        tool: tool.to_string(),
        arguments,
    };

    match exchange(&listener.pipe, &request).await {
        Ok(answer) => answer,
        Err(failure) => failure.answer(),
    }
}

fn refusal(error: &InstanceError) -> ToolAnswer {
    ToolAnswer::refused(error.code(), error.sentence())
}

/// One request, one reply, under a deadline.
#[cfg(windows)]
async fn exchange(pipe: &str, request: &wire::Request) -> Result<ToolAnswer, PipeFailure> {
    use cb_core::browser::framing;
    use tokio::io::AsyncWriteExt;

    let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(pipe)
        .map_err(|error| PipeFailure::NotConnected {
            pipe: pipe.to_string(),
            detail: error.to_string(),
        })?;

    let encoded = framing::encode(&wire::request_value(1, request));
    let tool = request.tool.clone();
    let exchange = async {
        client
            .write_all(&encoded)
            .await
            .map_err(|_| PipeFailure::Closed { tool: tool.clone() })?;

        let mut decoder = framing::pipe_decoder();
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
            // A reply is exactly one frame, so the first complete line ends
            // the exchange — this is `next`, not a loop, and nothing is being
            // dropped: no further frame is read because none is expected. An
            // incomplete read yields no line at all and the outer loop reads
            // again, which is why this cannot be hoisted out of it.
            if let Some(line) = lines.into_iter().next() {
                return match line {
                    framing::Line::Message(message) => wire::parse_answer(&message),
                    framing::Line::Malformed { error, .. } => {
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

/// The pipe does not exist off Windows, and neither does the panel. Kept so the
/// whole module compiles there — see [`serve::unsupported_answer`], which is
/// what actually answers.
#[cfg(not(windows))]
async fn exchange(pipe: &str, request: &wire::Request) -> Result<ToolAnswer, PipeFailure> {
    Err(PipeFailure::NotConnected {
        pipe: pipe.to_string(),
        detail: format!(
            "named pipes are a Windows facility; {} has no browser panel to reach",
            std::env::consts::OS
        ),
    })
}

/// One line to stdout. The **only** thing in this mode that writes there.
fn write_line(response: &Value) {
    use std::io::Write;

    let encoded = cb_core::browser::framing::encode(response);
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&encoded).is_err() || stdout.flush().is_err() {
        // The client is gone. Reported to stderr and nowhere else.
        eprintln!(
            "code-basics {}: the client closed its input.",
            argv::SUBCOMMAND
        );
    }
}

#[cfg(test)]
#[path = "mcp_browser_tests.rs"]
mod tests;
