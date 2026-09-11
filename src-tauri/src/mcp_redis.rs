//! The `mcp-redis` mode: this executable as the Redis MCP stdio server.
//!
//! The seventh self-dispatch mode. Like the SQL and Tasks servers it answers in
//! process (it opens the Redis connection itself), and like the Tasks server it
//! **writes** — but every write is gated twice: `expose_to_agents` decides
//! reachability ([`cb_core::redis::mcp::expose::find_exposed`]) and `allow_writes`
//! decides writing ([`cb_core::redis::mcp::execute::plan`]). Both are re-read from
//! the store on every call, so revoking either bites the next request.
//!
//! Everything it decides lives in [`cb_core::redis`]; this is I/O — read stdin,
//! resolve a connection, open a handle, run one command, write a line. stdout is
//! the transport, so every diagnostic goes to stderr.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use cb_core::lsp::jsonrpc::{self, Incoming, RequestId};
use cb_core::mcp::{ndjson, serve as mcp_serve};
use cb_core::redis::driver::{RedisClient, DEFAULT_CONNECT_TIMEOUT};
use cb_core::redis::mcp::answer::{self, McpRefusal};
use cb_core::redis::mcp::execute::Planned;
use cb_core::redis::mcp::tools::ToolCall;
use cb_core::redis::mcp::{argv, execute, expose, render, serve, tools};
use cb_core::redis::store;
use cb_core::tool_gate;
use rmcp::model::CallToolResult;
use serde_json::Value;
use tokio::io::AsyncReadExt;

/// How long one command may run before it is abandoned.
const OP_TIMEOUT: Duration = Duration::from_secs(20);

fn load_gate() -> tool_gate::ToolGateFile {
    tool_gate::load(&tool_gate::mcp_tools_path())
}

pub fn is_mcp_redis_invocation() -> bool {
    argv::is_mcp_redis_invocation(&std::env::args().collect::<Vec<_>>())
}

pub fn run() -> ! {
    let args: Vec<String> = std::env::args().collect();
    let Some(invocation) = argv::parse_mcp_redis_args(&args) else {
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
    std::process::exit(0)
}

async fn serve_stdio(workspace: Option<&std::path::Path>) {
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
            if let Some(response) = answer_line(line, workspace).await {
                write_line(&response);
            }
        }
    }
}

async fn answer_line(line: ndjson::Line, workspace: Option<&std::path::Path>) -> Option<Value> {
    let message = match line {
        ndjson::Line::Malformed { error, .. } => return Some(mcp_serve::parse_failure(&error)),
        ndjson::Line::Message(message) => message,
    };
    let bytes = serde_json::to_vec(&message).ok()?;
    match jsonrpc::classify(&bytes) {
        Err(error) => Some(mcp_serve::parse_failure(&error.to_string())),
        Ok(Incoming::Notification { .. }) => None,
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
    workspace: Option<&std::path::Path>,
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
            match tools::parse_call(&name, arguments.as_ref()) {
                Err(error) => mcp_serve::failure(id, &error),
                Ok(call) => {
                    let result = call_tool(call, workspace).await;
                    match serde_json::to_value(result) {
                        Ok(value) => mcp_serve::success(id, value),
                        Err(error) => mcp_serve::failure(
                            id,
                            &rmcp::model::ErrorData::internal_error(error.to_string(), None),
                        ),
                    }
                }
            }
        }
    }
}

async fn call_tool(call: ToolCall, workspace: Option<&std::path::Path>) -> CallToolResult {
    match dispatch(call, workspace).await {
        Ok(text) => answer::text_result(text),
        Err(refusal) => answer::refusal_result(&refusal),
    }
}

/// Load the store, resolve the connection, apply consent, run one command. The
/// store is re-read on every call so revoking exposure or writes bites the next.
async fn dispatch(
    call: ToolCall,
    workspace: Option<&std::path::Path>,
) -> Result<String, McpRefusal> {
    let file = store::load(&store::redis_connections_path());

    if matches!(call, ToolCall::ListConnections) {
        return serve::list_connections_answer(&file, workspace);
    }

    let selector = call.connection().unwrap_or("").to_string();
    let profile = expose::find_exposed(&file, &selector)?;
    let planned = execute::plan(profile.allow_writes, call)?;
    let target = execute::resolve_target(profile)?;

    match planned {
        Planned::ConnectionStatus => {
            let kind = match connect(&target).await {
                Ok(mut client) => match client.ping().await {
                    Ok(()) => cb_core::redis::model::RedisStatusKind::Ok,
                    Err(e) => e.kind,
                },
                Err(e) => e.kind,
            };
            Ok(render::status(kind))
        }
        Planned::Read(op) => {
            let mut client = connect(&target).await.map_err(connect_failed)?;
            let result = run_op(client.read(op)).await.map_err(op_failed)?;
            Ok(render_read(result))
        }
        Planned::Write(plan) => {
            let confirmation = render::write_ok(plan.op());
            let mut client = connect(&target).await.map_err(connect_failed)?;
            run_op(client.write(plan)).await.map_err(op_failed)?;
            Ok(confirmation)
        }
    }
}

async fn connect(
    target: &cb_core::redis::dsn::RedisTarget,
) -> Result<RedisClient, cb_core::redis::driver::DriverError> {
    RedisClient::connect(target, DEFAULT_CONNECT_TIMEOUT).await
}

/// Run one command under the op deadline. A timeout is its own failure kind.
async fn run_op<T>(
    fut: impl std::future::Future<Output = Result<T, cb_core::redis::driver::DriverError>>,
) -> Result<T, cb_core::redis::driver::DriverError> {
    match tokio::time::timeout(OP_TIMEOUT, fut).await {
        Ok(result) => result,
        Err(_elapsed) => Err(cb_core::redis::driver::DriverError {
            kind: cb_core::redis::model::RedisStatusKind::Timeout,
            message: "the command did not complete within its deadline".to_string(),
        }),
    }
}

fn connect_failed(e: cb_core::redis::driver::DriverError) -> McpRefusal {
    McpRefusal::ConnectFailed { kind: e.kind }
}

fn op_failed(e: cb_core::redis::driver::DriverError) -> McpRefusal {
    McpRefusal::OperationFailed { kind: e.kind }
}

fn render_read(result: cb_core::redis::driver::ReadResult) -> String {
    use cb_core::redis::driver::ReadResult;
    match result {
        ReadResult::Scan(page) => render::scan_page(&page),
        ReadResult::KeyInfo(info) => {
            let ttl = info
                .ttl_ms
                .map(|ms| format!("ttl {ms}ms"))
                .unwrap_or_else(|| "no ttl".to_string());
            format!("{}  {ttl}", info.key)
        }
        ReadResult::Value(value) => render::value(&value),
    }
}

fn write_line(message: &Value) {
    let bytes = ndjson::encode(message);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    if let Err(error) = handle.write_all(&bytes).and_then(|()| handle.flush()) {
        eprintln!("code-basics {}: stdout: {error}", argv::SUBCOMMAND);
    }
}
