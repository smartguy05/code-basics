//! The `mcp-sql` mode: this executable as an MCP stdio server.
//!
//! The fourth thing this binary can be, after the application, the intent
//! recorder and the quality gate — and like those two it re-invokes itself
//! rather than shipping a second artifact ([`cb_core::mcp::argv`]).
//!
//! **Everything it decides lives in [`cb_core::mcp`]**, which is unit tested
//! with no database and no client: what a version negotiation answers, which
//! method is which, what a refusal says, and what an agent may see are all
//! there. What is left here is I/O — read stdin, dispatch, open a connection,
//! run one statement, write a line — and it is here precisely because none of
//! it is reachable from a test.
//!
//! # Nothing may reach stdout except MCP bytes
//!
//! stdout **is** the transport. A single stray line on it desynchronises the
//! client, and the failure mode is a server that looks like it is hanging
//! rather than one that looks broken. So this mode installs no `tracing`
//! subscriber (the default one writes to stdout), and every diagnostic here
//! goes to stderr, which the specification reserves for exactly that.
//!
//! # One connection per call, closed after, and the store re-read every time
//!
//! This process is long-lived — a whole agent session — and consent is not.
//! `expose_to_agents` can be revoked in the app at any moment, and revocation
//! has to bite on the *next* call, so [`dispatch`] reloads the connection store
//! on every request and opens a fresh handle for the work. A cached pool would
//! keep answering from a database the user had already withdrawn, which is the
//! one failure this subsystem cannot be allowed to have.
//! `exposed_connections_are_read_per_call_and_never_cached` pins the rule in
//! `cb-core`; it cannot pin this file, so do not "optimise" the reload away.
//!
//! # The deadline is per stage, because a stage is what a refusal names
//!
//! [`cb_core::mcp::answer::ConnectionStatusKind::Timeout`] says *nothing was
//! sent and no statement ran*, so it may only be the answer when the **connect**
//! is what expired. A statement that outlives its deadline is
//! `StatementFailed { stage: Execute }` instead. This is the `within_timeout`
//! lesson from [`crate::commands::sql`] — a deadline around the wrong span
//! names the wrong cause — applied to a path where the cause is the whole of
//! what an agent is told.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_core::lsp::jsonrpc::{self, Incoming, RequestId};
use cb_core::mcp::answer::{self, McpRefusal};
use cb_core::mcp::tools::ToolCall;
use cb_core::mcp::{argv, execute, expose, ndjson, render, serve, tools};
use cb_core::sql::catalog;
use cb_core::sql::driver::{SqlConnection as LiveConnection, SqlDriver};
use cb_core::sql::model::SqlResultSet;
use cb_core::sql::store::{self, SqlConnection as StoredConnection};
use cb_core::tool_gate;
use rmcp::model::CallToolResult;
use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::commands::sql::{
    classify_connect_failure, connection_status_kind, driver_for, resolve_dsn, run_discarding_rows,
    test_outcome,
};

/// How long a connection may take to open before the attempt is abandoned.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long one statement may run. Generous, because a catalog query on a large
/// schema is legitimately slow, and an agent waiting is better than an agent
/// told the wrong thing.
const STATEMENT_TIMEOUT: Duration = Duration::from_secs(120);

/// The per-tool gate, re-read on every use so a tool switched off in the app
/// bites on the next request. See [`cb_core::tool_gate`].
fn load_gate() -> tool_gate::ToolGateFile {
    tool_gate::load(&tool_gate::mcp_tools_path())
}

/// Did the command line ask for the MCP server rather than the application?
pub fn is_mcp_sql_invocation() -> bool {
    argv::is_mcp_sql_invocation(&std::env::args().collect::<Vec<_>>())
}

/// Serve until stdin closes, then exit.
///
/// A **current-thread** runtime: this process serves one client and holds one
/// connection at a time, so the worker pool a multi-thread runtime would start
/// would sit idle for the life of an agent session.
pub fn run() -> ! {
    let args: Vec<String> = std::env::args().collect();
    let Some(invocation) = argv::parse_mcp_sql_args(&args) else {
        // Unreachable through `is_mcp_sql_invocation`, and still not assumed
        // away: exiting with a sentence beats serving a client that asked for
        // something this build did not understand.
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

/// The read loop.
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

    // Re-encoded so the one classifier in this tree reads it. `classify` takes
    // bytes because that is what its other caller has; the cost is a few
    // microseconds per message and the alternative is a second implementation
    // of the request/notification/response distinction, which is the exact
    // thing `jsonrpc`'s docs say must not be got wrong twice.
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
        serve::Route::ToolsList => serve::success(id, serve::tools_list_result(&load_gate())),
        serve::Route::Ping => serve::success(id, Value::Object(Default::default())),
        serve::Route::ToolsCall { name, arguments } => {
            match tools::parse_call(&name, arguments.as_ref()) {
                Err(error) => serve::failure(id, &error),
                Ok(call) => {
                    // A disabled-but-known tool is refused here, re-reading the
                    // gate every call so a tool switched off in the app bites on
                    // the next request. See `cb_core::tool_gate`.
                    let result = if load_gate().is_enabled(tool_gate::ServerId::Sql, &name) {
                        call_tool(call, workspace).await
                    } else {
                        tool_gate::disabled_tool_result(&name)
                    };
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
/// one — see [`cb_core::mcp::tools`].
async fn call_tool(call: ToolCall, workspace: Option<&Path>) -> CallToolResult {
    match dispatch(call, workspace).await {
        Ok(text) => tools::text_result(text),
        Err(refusal) => tools::refusal_result(&refusal),
    }
}

async fn dispatch(call: ToolCall, workspace: Option<&Path>) -> Result<String, McpRefusal> {
    // Every call, without exception. See the module docs.
    let file = store::load(&store::sql_connections_path());

    match call {
        ToolCall::ListConnections => serve::list_connections_answer(&file, workspace),

        ToolCall::ListTables { connection } => {
            let profile = expose::find_exposed(&file, &connection)?;
            let engine = engine_of(profile)?;
            let (_driver, mut live) = open(profile).await?;
            let result = statement(live.as_mut(), catalog::object_catalog_query(engine)).await?;
            Ok(render::tables(&serve::catalog_objects(&result)?))
        }

        ToolCall::DescribeTable {
            connection,
            schema,
            table,
        } => {
            let profile = expose::find_exposed(&file, &connection)?;
            let engine = engine_of(profile)?;
            // Through the same builder the console uses, so an identifier this
            // app will not interpolate is refused here rather than reaching a
            // driver as text.
            let schema_name = schema.as_deref().map(catalog::identifier).transpose();
            let schema_name = schema_name
                .map_err(|refusal| McpRefusal::IdentifierRefused { refusal })?
                .map(str::to_string);
            catalog::identifier(&table)
                .map_err(|refusal| McpRefusal::IdentifierRefused { refusal })?;
            let query = catalog::column_catalog_query(engine, schema_name.as_deref(), &table);

            let (_driver, mut live) = open(profile).await?;
            let result = statement(live.as_mut(), &query).await?;
            Ok(render::columns(&serve::catalog_columns(&result)?))
        }

        ToolCall::Query { connection, sql } => {
            let profile = expose::find_exposed(&file, &connection)?;
            // The guard runs before anything is opened, so a refused statement
            // never becomes a connection.
            let plan = execute::agent_plan(profile, &sql)?;
            let (_driver, mut live) = open(profile).await?;
            let result = statement(live.as_mut(), &plan.sql).await?;
            Ok(render::result(&result))
        }

        ToolCall::ReadOnlyEnforcement { connection } => {
            let profile = expose::find_exposed(&file, &connection)?;
            // Answerable without opening anything: it describes the mechanism
            // this path *would* use, which is a property of the engine.
            Ok(render::enforcement(execute::agent_enforcement(
                profile.engine,
            )))
        }

        ToolCall::ConnectionStatus { connection } => {
            let profile = expose::find_exposed(&file, &connection)?;
            // The console's own probe, with one change: it opens the handle the
            // profile asks for, and this path never opens a writable one. The
            // copy is what keeps rule 2 true even for a test connection.
            let mut probe = profile.clone();
            probe.allow_writes = false;
            let outcome = test_outcome(&probe, CONNECT_TIMEOUT.as_millis() as u64).await;
            Ok(render::status(connection_status_kind(&outcome)))
        }
    }
}

fn engine_of(profile: &StoredConnection) -> Result<cb_core::sql::dsn::SqlEngine, McpRefusal> {
    profile
        .engine
        .ok_or_else(|| McpRefusal::EngineUndetermined {
            connection: profile.id.clone(),
        })
}

/// Open a fresh read-only handle.
///
/// The driver is handed back with the connection so it outlives it; nothing
/// else needs it.
async fn open(
    profile: &StoredConnection,
) -> Result<(Box<dyn SqlDriver>, Box<dyn LiveConnection>), McpRefusal> {
    let engine = engine_of(profile)?;
    let driver = driver_for(engine).ok_or(McpRefusal::EngineUnsupported { engine })?;
    // The store's reason names a file and a key, so it is dropped rather than
    // forwarded: `SecretUnresolved` says only that it could not be read.
    let dsn = resolve_dsn(&profile.secret).map_err(|_| McpRefusal::SecretUnresolved {
        connection: profile.id.clone(),
    })?;
    let spec = execute::agent_connect_spec(dsn);

    let opened = match tokio::time::timeout(CONNECT_TIMEOUT, driver.connect(&spec)).await {
        Err(_elapsed) => {
            eprintln!(
                "code-basics {}: connect exceeded {:?}.",
                argv::SUBCOMMAND,
                CONNECT_TIMEOUT
            );
            return Err(answer::connect_failed(
                answer::ConnectionStatusKind::Timeout,
            ));
        }
        Ok(Ok(live)) => live,
        Ok(Err(error)) => {
            // Classified by the bridge's own rules — the one place a driver's
            // words are read — and only the *kind* crosses.
            let kind = connection_status_kind(&classify_connect_failure(&error.message));
            return Err(answer::connect_failed(kind));
        }
    };
    Ok((driver, opened))
}

/// Run one statement and take its result set.
async fn statement(live: &mut dyn LiveConnection, sql: &str) -> Result<SqlResultSet, McpRefusal> {
    match tokio::time::timeout(STATEMENT_TIMEOUT, run_discarding_rows(live, sql)).await {
        Err(_elapsed) => {
            eprintln!(
                "code-basics {}: statement exceeded {:?}.",
                argv::SUBCOMMAND,
                STATEMENT_TIMEOUT
            );
            // Not a connect timeout: the connection opened and the statement
            // ran. `ConnectionStatusKind::Timeout` says the opposite.
            Err(McpRefusal::StatementFailed {
                stage: cb_core::sql::driver::ErrorStage::Execute,
            })
        }
        Ok(Ok(outcome)) => Ok(outcome.result().clone()),
        Ok(Err(error)) => Err(answer::statement_failed(&error)),
    }
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
