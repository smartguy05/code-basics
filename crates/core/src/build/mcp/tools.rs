//! The four Build tools, their schemas, and how a call is read.
//!
//! | tool | takes | answers |
//! | --- | --- | --- |
//! | `build_solution` | optional `configuration` | runs a build and reports the outcome |
//! | `get_errors` | nothing | the errors from the most recent build |
//! | `get_warnings` | nothing | the warnings from the most recent build |
//! | `get_build_status` | nothing | the state of the most recent build |
//!
//! # These tools take no location
//!
//! Unlike the Roslyn tools, a build is workspace-wide: there is no file, position
//! or symbol to name. `build_solution` accepts one **optional** `configuration`
//! (e.g. `Debug`/`Release`); the three readers take nothing at all, because they
//! report the cached result of the last build and demanding an argument they would
//! ignore would be a lie in the schema.
//!
//! # The parse decides only the shape
//!
//! [`parse_call`] resolves the *shape*: a tool name it does not have, or a
//! `configuration` that is present but not a string. Everything else — that no
//! build has run, that a build could not start — is a tool *refusal* the
//! application answers ([`super::answer::BuildRefusal`]), not this pure layer's.

use rmcp::model::{ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{json, Map, Value};

pub const BUILD_SOLUTION: &str = "build_solution";
pub const GET_ERRORS: &str = "get_errors";
pub const GET_WARNINGS: &str = "get_warnings";
pub const GET_BUILD_STATUS: &str = "get_build_status";

/// Every tool this server has. The listing and the dispatch read the same array,
/// so a tool cannot be advertised and unreachable, or reachable and unadvertised.
pub const ALL: [&str; 4] = [BUILD_SOLUTION, GET_ERRORS, GET_WARNINGS, GET_BUILD_STATUS];

/// The sentence every tool description ends with — this is a real build, parsed
/// from what the compiler emitted, and its answers abstain rather than guess.
const ABSTAIN_SUFFIX: &str =
    " The diagnostics are parsed from what MSBuild wrote, not scraped from \
     console colour. If no build has run yet, or the build process could not start, the answer \
     says exactly that rather than reporting an empty success.";

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        Tool::new(
            BUILD_SOLUTION,
            format!(
                "Build the projects in this workspace and report the outcome: whether it \
                 succeeded, succeeded with warnings, or failed, and how many errors and warnings \
                 it produced. Runs the same build the editor's Build button runs.{ABSTAIN_SUFFIX}"
            ),
            build_solution_schema(),
        ),
        Tool::new(
            GET_ERRORS,
            format!(
                "The errors from the most recent build of this workspace, each with its file, \
                 line, column, code and message — structured, so you can act on them \
                 directly.{ABSTAIN_SUFFIX}"
            ),
            no_arg_schema(),
        ),
        Tool::new(
            GET_WARNINGS,
            format!(
                "The warnings from the most recent build of this workspace, each with its file, \
                 line, column, code and message.{ABSTAIN_SUFFIX}"
            ),
            no_arg_schema(),
        ),
        Tool::new(
            GET_BUILD_STATUS,
            format!(
                "The state of the most recent build of this workspace: never built, building, \
                 succeeded clean, succeeded with warnings, failed, or could-not-start — six \
                 distinct answers.{ABSTAIN_SUFFIX}"
            ),
            no_arg_schema(),
        ),
    ]
}

/// The schema for `build_solution`: an optional `configuration`.
fn build_solution_schema() -> JsonObject {
    let mut props = Map::new();
    props.insert(
        "configuration".to_string(),
        json!({
            "type": "string",
            "description": "Optional build configuration to use, e.g. \"Debug\" or \"Release\". \
                            Omit to use the project's default."
        }),
    );
    object(props, &[])
}

/// The schema for a tool that takes nothing.
fn no_arg_schema() -> JsonObject {
    object(Map::new(), &[])
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

/// One call, already read. The dispatch matches on this rather than on strings, so
/// a tool that is added to [`ALL`] and forgotten is a compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildToolCall {
    /// Run a build. `configuration` is the optional build configuration override.
    BuildSolution {
        configuration: Option<String>,
    },
    GetErrors,
    GetWarnings,
    GetBuildStatus,
}

impl BuildToolCall {
    /// The tool name this call was parsed from.
    pub fn name(&self) -> &'static str {
        match self {
            Self::BuildSolution { .. } => BUILD_SOLUTION,
            Self::GetErrors => GET_ERRORS,
            Self::GetWarnings => GET_WARNINGS,
            Self::GetBuildStatus => GET_BUILD_STATUS,
        }
    }
}

/// Read a call, or produce the protocol error saying why it could not be read.
///
/// Only two things are protocol errors: a tool this server does not have, and a
/// `configuration` that is present but not a string. Everything else is a tool
/// *refusal* the application answers.
pub fn parse_call(name: &str, arguments: Option<&JsonObject>) -> Result<BuildToolCall, ErrorData> {
    let empty = Map::new();
    let args = arguments.unwrap_or(&empty);
    match name {
        BUILD_SOLUTION => Ok(BuildToolCall::BuildSolution {
            configuration: configuration(args)?,
        }),
        GET_ERRORS => Ok(BuildToolCall::GetErrors),
        GET_WARNINGS => Ok(BuildToolCall::GetWarnings),
        GET_BUILD_STATUS => Ok(BuildToolCall::GetBuildStatus),
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            unknown_tool_sentence(unknown),
            None,
        )),
    }
}

/// The optional `configuration`. Absent or explicitly null is `None`; an empty
/// string is `None` (an empty configuration is no configuration); a present
/// non-string is a shape error.
fn configuration(args: &JsonObject) -> Result<Option<String>, ErrorData> {
    match args.get("configuration") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(Some(text.trim().to_string())),
        Some(Value::String(_)) => Ok(None),
        Some(other) => Err(ErrorData::invalid_params(
            format!(
                "{BUILD_SOLUTION} expects \"configuration\" to be a string; it was {}.",
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
/// Exists so the **shim** can refuse a name it does not have without first looking
/// for a running application.
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
