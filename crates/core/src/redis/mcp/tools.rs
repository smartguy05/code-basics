//! The Redis tools, their schemas, and how a call is read.
//!
//! Read tools (`list_connections`, `scan_keys`, `get_key`, `connection_status`)
//! are always advertised; write tools are advertised too but **refused at
//! execution** unless the connection has both `expose_to_agents` and
//! `allow_writes` — every write description says so, because a client may show a
//! description without the server instructions. Only two failures are protocol
//! errors, as everywhere in this codebase: an unknown tool name, and arguments
//! that are not the declared shape.

use rmcp::model::{ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{json, Map, Value};

pub const LIST_CONNECTIONS: &str = "redis.list_connections";
pub const SCAN_KEYS: &str = "redis.scan_keys";
pub const GET_KEY: &str = "redis.get_key";
pub const CONNECTION_STATUS: &str = "redis.connection_status";
pub const SET_STRING: &str = "redis.set_string";
pub const HASH_SET: &str = "redis.hash_set";
pub const LIST_PUSH: &str = "redis.list_push";
pub const SET_ADD: &str = "redis.set_add";
pub const ZSET_ADD: &str = "redis.zset_add";
pub const STREAM_ADD: &str = "redis.stream_add";
pub const DELETE_KEY: &str = "redis.delete_key";
pub const EXPIRE: &str = "redis.expire";

/// Every tool this server has. The listing and the dispatch read the same array.
pub const ALL: [&str; 12] = [
    LIST_CONNECTIONS,
    SCAN_KEYS,
    GET_KEY,
    CONNECTION_STATUS,
    SET_STRING,
    HASH_SET,
    LIST_PUSH,
    SET_ADD,
    ZSET_ADD,
    STREAM_ADD,
    DELETE_KEY,
    EXPIRE,
];

const WRITE_SUFFIX: &str = " This is a write. It is refused unless the connection has BOTH \
'expose to agents' and 'allow writes' enabled in the app's Redis panel — two separate consents, \
both off by default; nothing here can grant them.";

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        Tool::new(
            LIST_CONNECTIONS,
            "List the Redis connections the user has exposed to agents. Exposure is per connection \
             and off by default; connections that are not exposed are not listed and cannot be \
             reached."
                .to_string(),
            object(&[], &[]),
        ),
        Tool::new(
            SCAN_KEYS,
            "Scan the keyspace one page at a time, returning each key with its type and TTL. Pass \
             the returned cursor back to continue; a cursor of \"0\" means the scan is complete."
                .to_string(),
            object(
                &[
                    conn_arg(),
                    ("match", json!({ "type": "string", "description": "An optional glob to filter keys, e.g. \"user:*\"." })),
                    ("cursor", json!({ "type": "string", "description": "The cursor to continue from. Omit or \"0\" to start." })),
                    ("count", json!({ "type": "integer", "minimum": 1, "description": "A hint for how many keys to scan per page." })),
                ],
                &["connection"],
            ),
        ),
        Tool::new(
            GET_KEY,
            "Read a key's value, the way its type requires (string, hash, list, set, sorted set or \
             stream). A large value is capped and the cap reported."
                .to_string(),
            object(&[conn_arg(), key_arg()], &["connection", "key"]),
        ),
        Tool::new(
            CONNECTION_STATUS,
            "Report whether a connection opens, as a category. The server's own message is not \
             forwarded: it can embed the connection string."
                .to_string(),
            object(&[conn_arg()], &["connection"]),
        ),
        write_tool(
            SET_STRING,
            "Set a string key to a value, optionally with a TTL in milliseconds.",
            object(
                &[
                    conn_arg(),
                    key_arg(),
                    ("value", json!({ "type": "string", "description": "The string value to set." })),
                    ("ttlMs", json!({ "type": "integer", "minimum": 1, "description": "Optional TTL in milliseconds; omit for no expiry." })),
                ],
                &["connection", "key", "value"],
            ),
        ),
        write_tool(
            HASH_SET,
            "Set one field of a hash key to a value.",
            object(
                &[
                    conn_arg(),
                    key_arg(),
                    ("field", json!({ "type": "string", "description": "The hash field name." })),
                    ("value", json!({ "type": "string", "description": "The field value." })),
                ],
                &["connection", "key", "field", "value"],
            ),
        ),
        write_tool(
            LIST_PUSH,
            "Push a value onto a list, at the front (LPUSH) or the back (RPUSH).",
            object(
                &[
                    conn_arg(),
                    key_arg(),
                    ("value", json!({ "type": "string", "description": "The value to push." })),
                    ("front", json!({ "type": "boolean", "description": "true = push to the front (LPUSH); default false = back (RPUSH)." })),
                ],
                &["connection", "key", "value"],
            ),
        ),
        write_tool(
            SET_ADD,
            "Add a member to a set (SADD).",
            object(
                &[conn_arg(), key_arg(), ("member", json!({ "type": "string", "description": "The member to add." }))],
                &["connection", "key", "member"],
            ),
        ),
        write_tool(
            ZSET_ADD,
            "Add a member with a score to a sorted set (ZADD).",
            object(
                &[
                    conn_arg(),
                    key_arg(),
                    ("member", json!({ "type": "string", "description": "The member to add." })),
                    ("score", json!({ "type": "number", "description": "The member's score." })),
                ],
                &["connection", "key", "member", "score"],
            ),
        ),
        write_tool(
            STREAM_ADD,
            "Append an entry to a stream (XADD). Provide the entry's fields as an object of \
             string-to-string; omit the id to let Redis assign one.",
            object(
                &[
                    conn_arg(),
                    key_arg(),
                    ("id", json!({ "type": "string", "description": "The entry id, or omit for \"*\" (server-assigned)." })),
                    ("fields", json!({ "type": "object", "description": "The entry's fields, an object mapping field name to a string value.", "additionalProperties": { "type": "string" } })),
                ],
                &["connection", "key", "fields"],
            ),
        ),
        write_tool(
            DELETE_KEY,
            "Delete a key (DEL). This cannot be undone.",
            object(&[conn_arg(), key_arg()], &["connection", "key"]),
        ),
        write_tool(
            EXPIRE,
            "Set or clear a key's TTL. Give ttlMs to expire it after that many milliseconds; omit \
             ttlMs to make it persistent (PERSIST).",
            object(
                &[
                    conn_arg(),
                    key_arg(),
                    ("ttlMs", json!({ "type": "integer", "minimum": 1, "description": "TTL in milliseconds; omit to clear the TTL." })),
                ],
                &["connection", "key"],
            ),
        ),
    ]
}

fn write_tool(name: &'static str, description: &str, schema: JsonObject) -> Tool {
    Tool::new(name, format!("{description}{WRITE_SUFFIX}"), schema)
}

fn conn_arg() -> (&'static str, Value) {
    (
        "connection",
        json!({ "type": "string", "description": "The handle or name of an exposed connection (from redis.list_connections)." }),
    )
}

fn key_arg() -> (&'static str, Value) {
    (
        "key",
        json!({ "type": "string", "description": "The Redis key." }),
    )
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
                .map(|n| Value::String((*n).to_string()))
                .collect(),
        ),
    );
    schema
}

/// One call, already read. Matching on this rather than strings makes a
/// forgotten tool a compile error.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCall {
    ListConnections,
    ScanKeys {
        connection: String,
        pattern: Option<String>,
        cursor: Option<String>,
        count: Option<u64>,
    },
    GetKey {
        connection: String,
        key: String,
    },
    ConnectionStatus {
        connection: String,
    },
    SetString {
        connection: String,
        key: String,
        value: String,
        ttl_ms: Option<i64>,
    },
    HashSet {
        connection: String,
        key: String,
        field: String,
        value: String,
    },
    ListPush {
        connection: String,
        key: String,
        value: String,
        front: bool,
    },
    SetAdd {
        connection: String,
        key: String,
        member: String,
    },
    ZSetAdd {
        connection: String,
        key: String,
        member: String,
        score: f64,
    },
    StreamAdd {
        connection: String,
        key: String,
        id: Option<String>,
        fields: Vec<(String, String)>,
    },
    DeleteKey {
        connection: String,
        key: String,
    },
    Expire {
        connection: String,
        key: String,
        ttl_ms: Option<i64>,
    },
}

impl ToolCall {
    /// The connection selector a call names, or `None` for `list_connections`
    /// (which names no connection).
    pub fn connection(&self) -> Option<&str> {
        match self {
            ToolCall::ListConnections => None,
            ToolCall::ScanKeys { connection, .. }
            | ToolCall::GetKey { connection, .. }
            | ToolCall::ConnectionStatus { connection }
            | ToolCall::SetString { connection, .. }
            | ToolCall::HashSet { connection, .. }
            | ToolCall::ListPush { connection, .. }
            | ToolCall::SetAdd { connection, .. }
            | ToolCall::ZSetAdd { connection, .. }
            | ToolCall::StreamAdd { connection, .. }
            | ToolCall::DeleteKey { connection, .. }
            | ToolCall::Expire { connection, .. } => Some(connection),
        }
    }
}

/// Read a call, or the protocol error saying why it could not be read.
pub fn parse_call(name: &str, arguments: Option<&JsonObject>) -> Result<ToolCall, ErrorData> {
    let empty = Map::new();
    let a = arguments.unwrap_or(&empty);
    match name {
        LIST_CONNECTIONS => Ok(ToolCall::ListConnections),
        SCAN_KEYS => Ok(ToolCall::ScanKeys {
            connection: string(a, name, "connection")?,
            pattern: optional_string(a, name, "match")?,
            cursor: optional_string(a, name, "cursor")?,
            count: optional_u64(a, name, "count")?,
        }),
        GET_KEY => Ok(ToolCall::GetKey {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
        }),
        CONNECTION_STATUS => Ok(ToolCall::ConnectionStatus {
            connection: string(a, name, "connection")?,
        }),
        SET_STRING => Ok(ToolCall::SetString {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            value: string(a, name, "value")?,
            ttl_ms: optional_i64(a, name, "ttlMs")?,
        }),
        HASH_SET => Ok(ToolCall::HashSet {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            field: string(a, name, "field")?,
            value: string(a, name, "value")?,
        }),
        LIST_PUSH => Ok(ToolCall::ListPush {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            value: string(a, name, "value")?,
            front: optional_bool(a, name, "front")?.unwrap_or(false),
        }),
        SET_ADD => Ok(ToolCall::SetAdd {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            member: string(a, name, "member")?,
        }),
        ZSET_ADD => Ok(ToolCall::ZSetAdd {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            member: string(a, name, "member")?,
            score: number(a, name, "score")?,
        }),
        STREAM_ADD => Ok(ToolCall::StreamAdd {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            id: optional_string(a, name, "id")?,
            fields: string_map(a, name, "fields")?,
        }),
        DELETE_KEY => Ok(ToolCall::DeleteKey {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
        }),
        EXPIRE => Ok(ToolCall::Expire {
            connection: string(a, name, "connection")?,
            key: string(a, name, "key")?,
            ttl_ms: optional_i64(a, name, "ttlMs")?,
        }),
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            unknown_tool_sentence(unknown),
            None,
        )),
    }
}

/// Whether the name is one this server advertised (so a shim can refuse early).
pub fn is_known(tool: &str) -> bool {
    ALL.contains(&tool)
}

/// The one wording both the parse and any shim use for an unknown tool.
pub fn unknown_tool_sentence(tool: &str) -> String {
    format!(
        "There is no tool called {tool:?}. This server has: {}.",
        ALL.join(", ")
    )
}

/// Which tool names are writes — the dispatch consults this to require consent.
pub fn is_write(tool: &str) -> bool {
    matches!(
        tool,
        SET_STRING | HASH_SET | LIST_PUSH | SET_ADD | ZSET_ADD | STREAM_ADD | DELETE_KEY | EXPIRE
    )
}

fn string(a: &JsonObject, tool: &str, field: &str) -> Result<String, ErrorData> {
    match a.get(field) {
        Some(Value::String(s)) => Ok(s.clone()),
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

fn optional_string(a: &JsonObject, tool: &str, field: &str) -> Result<Option<String>, ErrorData> {
    match a.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a string; it was {}.",
                type_of(other)
            ),
            None,
        )),
    }
}

fn optional_bool(a: &JsonObject, tool: &str, field: &str) -> Result<Option<bool>, ErrorData> {
    match a.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a boolean; it was {}.",
                type_of(other)
            ),
            None,
        )),
    }
}

fn optional_u64(a: &JsonObject, tool: &str, field: &str) -> Result<Option<u64>, ErrorData> {
    match a.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_u64().map(Some).ok_or_else(|| {
            ErrorData::invalid_params(
                format!("{tool} expects {field:?} to be a whole number of zero or more."),
                None,
            )
        }),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a number; it was {}.",
                type_of(other)
            ),
            None,
        )),
    }
}

fn optional_i64(a: &JsonObject, tool: &str, field: &str) -> Result<Option<i64>, ErrorData> {
    match a.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| {
            ErrorData::invalid_params(
                format!("{tool} expects {field:?} to be a whole number."),
                None,
            )
        }),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a number; it was {}.",
                type_of(other)
            ),
            None,
        )),
    }
}

fn number(a: &JsonObject, tool: &str, field: &str) -> Result<f64, ErrorData> {
    match a.get(field) {
        Some(Value::Number(n)) => n.as_f64().ok_or_else(|| {
            ErrorData::invalid_params(format!("{tool} expects {field:?} to be a number."), None)
        }),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a number; it was {}.",
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

/// An object whose values are all strings, as `Vec<(String, String)>`.
fn string_map(a: &JsonObject, tool: &str, field: &str) -> Result<Vec<(String, String)>, ErrorData> {
    match a.get(field) {
        Some(Value::Object(map)) if !map.is_empty() => {
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                match v {
                    Value::String(s) => out.push((k.clone(), s.clone())),
                    other => {
                        return Err(ErrorData::invalid_params(
                            format!(
                            "{tool} expects every value in {field:?} to be a string; {k:?} was {}.",
                            type_of(other)
                        ),
                            None,
                        ))
                    }
                }
            }
            Ok(out)
        }
        Some(Value::Object(_)) => Err(ErrorData::invalid_params(
            format!("{tool} expects {field:?} to have at least one field."),
            None,
        )),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be an object; it was {}.",
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

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
