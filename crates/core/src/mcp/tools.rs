//! The six tools, their schemas, and how a call is read.
//!
//! # What is here
//!
//! | Tool | Answers |
//! | --- | --- |
//! | `sql.list_connections` | id, name, engine (`null` when undetermined) |
//! | `sql.list_tables` | schema and name pairs |
//! | `sql.describe_table` | column rows |
//! | `sql.query` | capped rows, with the cap reported first |
//! | `sql.read_only_enforcement` | what is really stopping a write, and what is not |
//! | `sql.connection_status` | whether the connection opens, as a variant with no message |
//!
//! `sql.read_only_enforcement` is a tool of its own rather than a line on every
//! other tool's output, and it is the reason this surface can be honest: for
//! SQL Server a write **executes and is rolled back**, and an agent that reads
//! "read-only" as "cannot write" is being told something untrue. See
//! [`crate::mcp::render`].
//!
//! # What is deliberately not here
//!
//! **No explain tool.** The hazard an explain tool would open is already closed
//! — [`crate::sql::guard`]'s `explain_effect` makes `EXPLAIN ANALYZE INSERT` a
//! write and an unrecognised option a refusal — so the objection is not safety.
//! It is that **SQL Server has no `EXPLAIN`**: its equivalent is a session
//! statement (`SET SHOWPLAN_ALL`) that the guard refuses, so the tool would be
//! unsupported for one of the three shipped engines while buying nothing at
//! all, because `sql.query` already accepts `EXPLAIN SELECT ...` as an ordinary
//! read.
//!
//! **No `sql.discover`.** [`crate::sql::discover`] reads `appsettings.json`,
//! .NET user secrets and `.env` files hunting for connection strings. Exposing
//! it here would be a filesystem scraper aimed at exactly the values this
//! subsystem exists to keep out of an agent's hands, and it would do it under a
//! name that sounds helpful.
//!
//! # Which failures are protocol errors
//!
//! Two, and only two: a tool name this server does not have, and arguments that
//! are not the shape the schema declared. Neither is something a model can fix
//! by reasoning about the database. Everything else — every
//! [`McpRefusal`] — is a **tool execution error**, because the specification
//! says a client SHOULD hand those to the model so it can correct itself, and
//! self-correction is exactly what a refusal here invites.

use rmcp::model::{CallToolResult, ContentBlock, ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{json, Map, Value};

use super::answer::McpRefusal;

pub const LIST_CONNECTIONS: &str = "sql.list_connections";
pub const LIST_TABLES: &str = "sql.list_tables";
pub const DESCRIBE_TABLE: &str = "sql.describe_table";
pub const QUERY: &str = "sql.query";
pub const READ_ONLY_ENFORCEMENT: &str = "sql.read_only_enforcement";
pub const CONNECTION_STATUS: &str = "sql.connection_status";

/// Every tool this server has. The listing and the dispatch read the same
/// array, so a tool cannot be advertised and unreachable, or reachable and
/// unadvertised.
pub const ALL: [&str; 6] = [
    LIST_CONNECTIONS,
    LIST_TABLES,
    DESCRIBE_TABLE,
    QUERY,
    READ_ONLY_ENFORCEMENT,
    CONNECTION_STATUS,
];

/// The sentence every tool description ends with.
///
/// Repeated on each tool rather than stated once in the server instructions,
/// because a client may show a tool's description without them, and a model
/// that has read only the description would otherwise believe writes are
/// possible with the right argument.
const READ_ONLY_SUFFIX: &str =
    " This interface is read-only and there is no argument that lifts that.";

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        tool(
            LIST_CONNECTIONS,
            "List the database connections the user has explicitly exposed to agents. Exposure \
             is off by default and is granted per connection in the app; connections that are \
             not exposed are not listed and cannot be reached.",
            object(&[], &[]),
        ),
        tool(
            LIST_TABLES,
            "List the tables in a connection's catalog.",
            object(&[connection_arg()], &["connection"]),
        ),
        tool(
            DESCRIBE_TABLE,
            "List a table's columns, with each column's type and the facts the engine reported \
             about it. A fact the engine did not report is said to be unreported rather than \
             assumed.",
            object(
                &[
                    connection_arg(),
                    (
                        "table",
                        json!({ "type": "string", "description": "The table name, unqualified." }),
                    ),
                    (
                        "schema",
                        json!({
                            "type": "string",
                            "description": "The schema the table is in. Omit it to search every \
                                            schema; an empty string is not the same as omitting it \
                                            and is refused."
                        }),
                    ),
                ],
                &["connection", "table"],
            ),
        ),
        tool(
            QUERY,
            "Run one read-only statement and return its rows. A result that hit a row or byte \
             ceiling says so before the rows, because a capped result is an incomplete answer \
             rather than a short one.",
            object(
                &[
                    connection_arg(),
                    (
                        "sql",
                        json!({
                            "type": "string",
                            "description": "One SQL statement. It is classified by a read-only \
                                            guard first; anything not positively recognised as a \
                                            read is refused, including statements the guard \
                                            simply does not know."
                        }),
                    ),
                ],
                &["connection", "sql"],
            ),
        ),
        tool(
            READ_ONLY_ENFORCEMENT,
            "Say what is actually stopping a write on a connection, and what is not. Read this \
             before trusting the word read-only: for SQL Server a write executes inside a \
             transaction that is rolled back, so it runs and is undone rather than being \
             refused, and anything it does outside that transaction survives.",
            object(&[connection_arg()], &["connection"]),
        ),
        tool(
            CONNECTION_STATUS,
            "Report whether a connection opens, as a category. The database driver's own message \
             is deliberately not forwarded: it routinely embeds the connection string.",
            object(&[connection_arg()], &["connection"]),
        ),
    ]
}

fn connection_arg() -> (&'static str, Value) {
    (
        "connection",
        json!({
            "type": "string",
            "description": "The id or the name of an exposed connection. A name that matches \
                            several is refused rather than resolved; use the id."
        }),
    )
}

fn tool(name: &'static str, description: &str, schema: JsonObject) -> Tool {
    Tool::new(name, format!("{description}{READ_ONLY_SUFFIX}"), schema)
}

fn object(properties: &[(&str, Value)], required: &[&str]) -> JsonObject {
    let mut props = Map::new();
    for (name, schema) in properties {
        props.insert((*name).to_string(), schema.clone());
    }
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(props));
    schema.insert(
        "required".to_string(),
        Value::Array(
            required
                .iter()
                .map(|name| Value::String((*name).to_string()))
                .collect(),
        ),
    );
    schema
}

/// One call, already read. The dispatch matches on this rather than on strings,
/// so a tool that is added to [`ALL`] and forgotten here is a compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCall {
    ListConnections,
    ListTables {
        connection: String,
    },
    DescribeTable {
        connection: String,
        /// [`None`] means *every schema*. `Some("")` is not that, and is
        /// refused downstream by [`crate::sql::catalog::identifier`].
        schema: Option<String>,
        table: String,
    },
    Query {
        connection: String,
        sql: String,
    },
    ReadOnlyEnforcement {
        connection: String,
    },
    ConnectionStatus {
        connection: String,
    },
}

/// Read a call, or produce the protocol error that says why it could not be
/// read. See the module docs for why only these two are protocol errors.
pub fn parse_call(name: &str, arguments: Option<&JsonObject>) -> Result<ToolCall, ErrorData> {
    let empty = Map::new();
    let args = arguments.unwrap_or(&empty);
    match name {
        LIST_CONNECTIONS => Ok(ToolCall::ListConnections),
        LIST_TABLES => Ok(ToolCall::ListTables {
            connection: string(args, name, "connection")?,
        }),
        DESCRIBE_TABLE => Ok(ToolCall::DescribeTable {
            connection: string(args, name, "connection")?,
            table: string(args, name, "table")?,
            schema: optional_string(args, name, "schema")?,
        }),
        QUERY => Ok(ToolCall::Query {
            connection: string(args, name, "connection")?,
            sql: string(args, name, "sql")?,
        }),
        READ_ONLY_ENFORCEMENT => Ok(ToolCall::ReadOnlyEnforcement {
            connection: string(args, name, "connection")?,
        }),
        CONNECTION_STATUS => Ok(ToolCall::ConnectionStatus {
            connection: string(args, name, "connection")?,
        }),
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            format!(
                "There is no tool called {unknown:?}. This server has: {}.",
                ALL.join(", ")
            ),
            None,
        )),
    }
}

fn string(args: &JsonObject, tool: &str, field: &str) -> Result<String, ErrorData> {
    match args.get(field) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a string; it was {}.",
                type_of(other)
            ),
            None,
        )),
        None => Err(ErrorData::invalid_params(
            format!("{tool} requires {field:?}."),
            None,
        )),
    }
}

fn optional_string(
    args: &JsonObject,
    tool: &str,
    field: &str,
) -> Result<Option<String>, ErrorData> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a string when it is given; it was {}.",
                type_of(other)
            ),
            None,
        )),
    }
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// A successful answer.
pub fn text_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

/// A refusal, as a **tool execution error** — never a protocol error.
///
/// The machine-matchable code leads so a model can branch without parsing
/// prose, and the sentence follows so it can explain itself to its user.
pub fn refusal_result(refusal: &McpRefusal) -> CallToolResult {
    let mut result = CallToolResult::error(vec![ContentBlock::text(format!(
        "{}: {}",
        refusal.code(),
        refusal.sentence()
    ))]);
    result.structured_content = Some(json!({ "code": refusal.code() }));
    result
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
