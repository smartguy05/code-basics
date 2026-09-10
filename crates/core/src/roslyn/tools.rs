//! The four Roslyn tools, their schemas, and how a call is read.
//!
//! | tool | takes | answers |
//! | --- | --- | --- |
//! | `find_references` | path + position-or-symbol | every use site of the symbol |
//! | `get_diagnostics` | path | every diagnostic on the file |
//! | `get_type_hierarchy` | path + position-or-symbol | the type's supertypes and subtypes |
//! | `resolve_overloads` | path + position-or-symbol | the callable signatures at a call site |
//!
//! # There is no editor, so a location is a position *or* a symbol
//!
//! A human points a caret; an agent has neither caret nor open file. So the three
//! position-taking tools accept **either** an exact `{line, character}` (1-based
//! line matching the editor gutter, 0-based UTF-16 `character`) **or** a
//! `{symbol: "Name"}` the application resolves to the identifier anchor via its
//! own document-symbol index. `get_diagnostics` is whole-file — pull diagnostics
//! are document-scoped — so it takes **only** `path`; demanding a position it
//! would ignore would be a lie in the schema.
//!
//! # The parse decides only what it can see
//!
//! [`parse_call`] resolves the *shape*: a complete `{line, character}` **wins**
//! when both a position and a symbol are given, an incomplete position (one of
//! the two) is a shape error, and neither a position nor a symbol is a shape
//! error naming both ways in. What it does **not** decide is whether a symbol
//! name is ambiguous — that needs the document-symbol index, so it is the
//! application's answer (refuse-and-list, never pick), not this pure layer's.

use rmcp::model::{ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{json, Map, Value};

pub const FIND_REFERENCES: &str = "find_references";
pub const GET_DIAGNOSTICS: &str = "get_diagnostics";
pub const GET_TYPE_HIERARCHY: &str = "get_type_hierarchy";
pub const RESOLVE_OVERLOADS: &str = "resolve_overloads";

/// Every tool this server has. The listing and the dispatch read the same array,
/// so a tool cannot be advertised and unreachable, or reachable and unadvertised.
pub const ALL: [&str; 4] = [
    FIND_REFERENCES,
    GET_DIAGNOSTICS,
    GET_TYPE_HIERARCHY,
    RESOLVE_OVERLOADS,
];

/// The sentence every tool description ends with — this is the semantic model,
/// not a text search, and it can abstain.
const ABSTAIN_SUFFIX: &str = " This is answered from the language server the application already \
     keeps warm for this workspace, not from a text search. If that server is still loading, does \
     not support this question for this file's language, or is not running, the answer says so \
     rather than guessing.";

/// The sentence the position-taking tools' descriptions carry.
const LOCATION_SUFFIX: &str = " Give a location as either an exact position (line, 1-based, and \
     character, 0-based UTF-16 code units) or a symbol name; when a symbol name matches more than \
     one declaration it is listed rather than guessed at, so pass a position to disambiguate.";

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        located(
            FIND_REFERENCES,
            "Every place a symbol is used across the workspace, from the language server — the \
             same answer the editor's \"find usages\" gives, not a text match. Point it at a \
             declaration or a use of the symbol.",
        ),
        Tool::new(
            GET_DIAGNOSTICS,
            format!(
                "Every diagnostic (error, warning, hint) the language server reports on one file, \
                 pulled on request rather than read from a stale cache.{ABSTAIN_SUFFIX}"
            ),
            location_free_schema(),
        ),
        located(
            GET_TYPE_HIERARCHY,
            "The inheritance around a type: the types it derives from and the types that derive \
             from it, from the language server. Point it at a type name or a position on one.",
        ),
        located(
            RESOLVE_OVERLOADS,
            "The callable signatures available at a call site, with the one the server considers \
             active — the editor's signature help. Point it at a position inside a call, or at \
             the called method's name.",
        ),
    ]
}

/// A tool that takes `path` plus a position-or-symbol.
fn located(name: &'static str, description: &str) -> Tool {
    Tool::new(
        name,
        format!("{description}{LOCATION_SUFFIX}{ABSTAIN_SUFFIX}"),
        location_schema(),
    )
}

/// The schema for a whole-file tool: just `path`.
fn location_free_schema() -> JsonObject {
    let mut props = Map::new();
    props.insert("path".to_string(), path_property());
    object(props, &["path"])
}

/// The schema for a position-or-symbol tool.
fn location_schema() -> JsonObject {
    let mut props = Map::new();
    props.insert("path".to_string(), path_property());
    props.insert(
        "line".to_string(),
        json!({
            "type": "integer",
            "minimum": 1,
            "description": "1-based line, matching the editor gutter. Give this with \"character\" \
                            for an exact position, or omit both and give \"symbol\"."
        }),
    );
    props.insert(
        "character".to_string(),
        json!({
            "type": "integer",
            "minimum": 0,
            "description": "0-based column in UTF-16 code units. Give this with \"line\"."
        }),
    );
    props.insert(
        "symbol".to_string(),
        json!({
            "type": "string",
            "description": "A symbol name to resolve to its declaration, as an alternative to a \
                            position. If it matches more than one declaration the matches are \
                            listed rather than guessed at; pass a position to choose one."
        }),
    );
    // Only `path` is required: a location is either a position or a symbol, so
    // neither line/character nor symbol can be marked required on its own.
    object(props, &["path"])
}

fn path_property() -> Value {
    json!({
        "type": "string",
        "description": "A workspace-relative path, forward slashes, to the file the location is \
                        in — e.g. \"src/App.cs\"."
    })
}

fn object(properties: Map<String, Value>, required: &[&str]) -> JsonObject {
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
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

/// A location inside a file: an exact position, or a symbol name to resolve.
///
/// The application resolves a [`Target::Symbol`] to a position via its own
/// document-symbol index; this layer never does — it has no file to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// **1-based** line, **0-based UTF-16** character. See the module docs.
    Position { line: u32, character: u32 },
    /// A symbol name, resolved by the application (refuse-and-list on ambiguity).
    Symbol { name: String },
}

/// One call, already read. The dispatch matches on this rather than on strings,
/// so a tool that is added to [`ALL`] and forgotten is a compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoslynToolCall {
    FindReferences {
        path: String,
        target: Target,
    },
    /// Whole-file: pull diagnostics are document-scoped, so no position.
    GetDiagnostics {
        path: String,
    },
    GetTypeHierarchy {
        path: String,
        target: Target,
    },
    ResolveOverloads {
        path: String,
        target: Target,
    },
}

impl RoslynToolCall {
    /// The file this call is about — every call names one.
    pub fn path(&self) -> &str {
        match self {
            Self::FindReferences { path, .. }
            | Self::GetDiagnostics { path }
            | Self::GetTypeHierarchy { path, .. }
            | Self::ResolveOverloads { path, .. } => path,
        }
    }

    /// The tool name this call was parsed from.
    pub fn name(&self) -> &'static str {
        match self {
            Self::FindReferences { .. } => FIND_REFERENCES,
            Self::GetDiagnostics { .. } => GET_DIAGNOSTICS,
            Self::GetTypeHierarchy { .. } => GET_TYPE_HIERARCHY,
            Self::ResolveOverloads { .. } => RESOLVE_OVERLOADS,
        }
    }
}

/// Read a call, or produce the protocol error saying why it could not be read.
///
/// Only two things are protocol errors, the same two as [`crate::mcp::tools`]: a
/// tool this server does not have, and arguments that are not the declared shape.
/// Everything else — a still-loading server, a language that does not support the
/// question, an ambiguous symbol — is a tool *refusal* the application answers.
pub fn parse_call(name: &str, arguments: Option<&JsonObject>) -> Result<RoslynToolCall, ErrorData> {
    let empty = Map::new();
    let args = arguments.unwrap_or(&empty);
    match name {
        GET_DIAGNOSTICS => Ok(RoslynToolCall::GetDiagnostics {
            path: path(args, name)?,
        }),
        FIND_REFERENCES => Ok(RoslynToolCall::FindReferences {
            path: path(args, name)?,
            target: target(args, name)?,
        }),
        GET_TYPE_HIERARCHY => Ok(RoslynToolCall::GetTypeHierarchy {
            path: path(args, name)?,
            target: target(args, name)?,
        }),
        RESOLVE_OVERLOADS => Ok(RoslynToolCall::ResolveOverloads {
            path: path(args, name)?,
            target: target(args, name)?,
        }),
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            unknown_tool_sentence(unknown),
            None,
        )),
    }
}

fn path(args: &JsonObject, tool: &str) -> Result<String, ErrorData> {
    match args.get("path") {
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(text.clone()),
        Some(Value::String(_)) => Err(ErrorData::invalid_params(
            format!("{tool} requires \"path\" to name a file; it was empty."),
            None,
        )),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects \"path\" to be a string; it was {}.",
                type_of(other)
            ),
            None,
        )),
        None => Err(ErrorData::invalid_params(
            format!("{tool} requires \"path\", a workspace-relative path to the file."),
            None,
        )),
    }
}

/// Resolve the location shape: a complete position **wins** when both are given,
/// an incomplete position is an error, and neither is an error naming both ways.
fn target(args: &JsonObject, tool: &str) -> Result<Target, ErrorData> {
    let line = whole(args, tool, "line")?;
    let character = whole(args, tool, "character")?;
    let symbol = match args.get("symbol") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
        Some(Value::String(_)) => None, // an empty symbol is no symbol
        Some(other) => {
            return Err(ErrorData::invalid_params(
                format!(
                    "{tool} expects \"symbol\" to be a string; it was {}.",
                    type_of(other)
                ),
                None,
            ))
        }
    };

    match (line, character) {
        // A complete position wins even if a symbol was also sent.
        (Some(line), Some(character)) => {
            if line < 1 {
                return Err(ErrorData::invalid_params(
                    format!("{tool} expects \"line\" to be 1-based, so 1 or greater; it was 0."),
                    None,
                ));
            }
            Ok(Target::Position {
                line: line as u32,
                character: character as u32,
            })
        }
        // An incomplete position is a mistake worth reporting, even alongside a
        // symbol: the caller clearly meant a position and got half of it.
        (Some(_), None) | (None, Some(_)) => Err(ErrorData::invalid_params(
            format!(
                "{tool} needs both \"line\" and \"character\" for a position; give both, or give \
                 \"symbol\" instead."
            ),
            None,
        )),
        (None, None) => match symbol {
            Some(name) => Ok(Target::Symbol { name }),
            None => Err(ErrorData::invalid_params(
                format!(
                    "{tool} needs a location: either a position (\"line\" 1-based and \
                     \"character\" 0-based) or a \"symbol\" name."
                ),
                None,
            )),
        },
    }
}

/// A whole, non-negative number, or a shape error. Absent is `None`.
fn whole(args: &JsonObject, tool: &str, field: &str) -> Result<Option<u64>, ErrorData> {
    match args.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or_else(|| {
            ErrorData::invalid_params(
                format!(
                    "{tool} expects {field:?} to be a whole number of zero or more; it was \
                     {number}."
                ),
                None,
            )
        }),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{tool} expects {field:?} to be a whole number; it was {}.",
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

/// Is this a tool this server advertised?
///
/// Exists so the **shim** can refuse a name it does not have without first
/// looking for a running application.
pub fn is_known(tool: &str) -> bool {
    ALL.contains(&tool)
}

/// Why a tool name is refused, in the one wording both sides use.
pub fn unknown_tool_sentence(tool: &str) -> String {
    format!(
        "There is no tool called {tool:?}. This server has: {}.",
        ALL.join(", ")
    )
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
