//! The seven tools, their schemas, and how a call is read.
//!
//! | Tool | Does |
//! | --- | --- |
//! | `list_tasks` | list every task in this workspace |
//! | `get_task` | read one task in full, including its body |
//! | `create_task` | add a new open task, owned by the user |
//! | `update_task` | change a task's title and/or body |
//! | `complete_task` | mark a task done |
//! | `delete_task` | remove a task |
//! | `assign_task` | set a task's owner to `me` or `ai` |
//!
//! Unlike [`crate::mcp::tools`] there is **no read-only suffix** on these
//! descriptions and no read-only enforcement anywhere: five of the seven tools
//! write, on purpose (see the module docs).
//!
//! # Which failures are protocol errors
//!
//! Two, and only two, exactly as in the SQL server: a tool name this server
//! does not have, and arguments that are not the shape the schema declared —
//! which includes an `owner` outside the `{me, ai}` set the schema names.
//! Neither is something a model can fix by reasoning about the task list.
//! Everything else — every [`McpRefusal`] — is a **tool execution error**, so
//! the client hands it to the model to self-correct.

use rmcp::model::{CallToolResult, ContentBlock, ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{json, Map, Value};

use super::answer::McpRefusal;
use crate::tasks::TaskOwner;

pub const LIST_TASKS: &str = "list_tasks";
pub const GET_TASK: &str = "get_task";
pub const CREATE_TASK: &str = "create_task";
pub const UPDATE_TASK: &str = "update_task";
pub const COMPLETE_TASK: &str = "complete_task";
pub const DELETE_TASK: &str = "delete_task";
pub const ASSIGN_TASK: &str = "assign_task";

/// Every tool this server has. The listing and the dispatch read the same
/// array, so a tool cannot be advertised and unreachable, or reachable and
/// unadvertised.
pub const ALL: [&str; 7] = [
    LIST_TASKS,
    GET_TASK,
    CREATE_TASK,
    UPDATE_TASK,
    COMPLETE_TASK,
    DELETE_TASK,
    ASSIGN_TASK,
];

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        tool(
            LIST_TASKS,
            "List every task in this workspace's task list, with each task's id, status (open or \
             done) and owner (me or ai).",
            object(&[], &[]),
        ),
        tool(
            GET_TASK,
            "Read one task in full, including its longer body.",
            object(&[id_arg()], &["id"]),
        ),
        tool(
            CREATE_TASK,
            "Add a new task. It starts open and owned by the user (me); use assign_task to hand it \
             to the ai.",
            object(
                &[
                    (
                        "title",
                        json!({ "type": "string", "description": "The short one-line title." }),
                    ),
                    (
                        "body",
                        json!({
                            "type": "string",
                            "description": "The longer description. Optional; omit it for an \
                                            empty body."
                        }),
                    ),
                ],
                &["title"],
            ),
        ),
        tool(
            UPDATE_TASK,
            "Change a task's title and/or body. A field that is omitted is left as it is; sending \
             an empty string sets it to empty.",
            object(
                &[
                    id_arg(),
                    (
                        "title",
                        json!({ "type": "string", "description": "The new title. Omit to keep the \
                                current one." }),
                    ),
                    (
                        "body",
                        json!({ "type": "string", "description": "The new body. Omit to keep the \
                                current one." }),
                    ),
                ],
                &["id"],
            ),
        ),
        tool(
            COMPLETE_TASK,
            "Mark a task done.",
            object(&[id_arg()], &["id"]),
        ),
        tool(
            DELETE_TASK,
            "Remove a task from the list. This cannot be undone from here.",
            object(&[id_arg()], &["id"]),
        ),
        tool(
            ASSIGN_TASK,
            "Set a task's owner. Assigning to the user (me) hands it back to them; assigning to \
             the ai marks it as the agent's to do. This server only records the owner — it does \
             not itself launch anything.",
            object(
                &[
                    id_arg(),
                    (
                        "owner",
                        json!({
                            "type": "string",
                            "enum": ["me", "ai"],
                            "description": "Who the task is assigned to: \"me\" for the user, \
                                            \"ai\" for the agent."
                        }),
                    ),
                ],
                &["id", "owner"],
            ),
        ),
    ]
}

fn id_arg() -> (&'static str, Value) {
    (
        "id",
        json!({
            "type": "string",
            "description": "The id of a task, as returned by list_tasks or create_task."
        }),
    )
}

fn tool(name: &'static str, description: &str, schema: JsonObject) -> Tool {
    Tool::new(name, description.to_string(), schema)
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
    ListTasks,
    GetTask {
        id: String,
    },
    CreateTask {
        title: String,
        /// [`None`] means the client sent no body; the task gets an empty one.
        body: Option<String>,
    },
    UpdateTask {
        id: String,
        /// [`None`] leaves the existing title unchanged; `Some("")` clears it.
        title: Option<String>,
        /// [`None`] leaves the existing body unchanged; `Some("")` clears it.
        body: Option<String>,
    },
    CompleteTask {
        id: String,
    },
    DeleteTask {
        id: String,
    },
    AssignTask {
        id: String,
        owner: TaskOwner,
    },
}

/// Read a call, or produce the protocol error that says why it could not be
/// read. See the module docs for why only these are protocol errors.
pub fn parse_call(name: &str, arguments: Option<&JsonObject>) -> Result<ToolCall, ErrorData> {
    let empty = Map::new();
    let args = arguments.unwrap_or(&empty);
    match name {
        LIST_TASKS => Ok(ToolCall::ListTasks),
        GET_TASK => Ok(ToolCall::GetTask {
            id: string(args, name, "id")?,
        }),
        CREATE_TASK => Ok(ToolCall::CreateTask {
            title: string(args, name, "title")?,
            body: optional_string(args, name, "body")?,
        }),
        UPDATE_TASK => Ok(ToolCall::UpdateTask {
            id: string(args, name, "id")?,
            title: optional_string(args, name, "title")?,
            body: optional_string(args, name, "body")?,
        }),
        COMPLETE_TASK => Ok(ToolCall::CompleteTask {
            id: string(args, name, "id")?,
        }),
        DELETE_TASK => Ok(ToolCall::DeleteTask {
            id: string(args, name, "id")?,
        }),
        ASSIGN_TASK => Ok(ToolCall::AssignTask {
            id: string(args, name, "id")?,
            owner: owner(args, name)?,
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

/// The `owner` argument, mapped onto [`TaskOwner`]. A value outside the schema's
/// declared `{me, ai}` set is a **schema violation** and so an invalid-params
/// protocol error, not a tool refusal.
fn owner(args: &JsonObject, tool: &str) -> Result<TaskOwner, ErrorData> {
    match string(args, tool, "owner")?.as_str() {
        "me" => Ok(TaskOwner::Me),
        "ai" => Ok(TaskOwner::Ai),
        other => Err(ErrorData::invalid_params(
            format!("{tool} expects \"owner\" to be \"me\" or \"ai\"; it was {other:?}."),
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
