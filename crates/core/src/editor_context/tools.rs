//! The four editor-context tools, their schemas, and how a call is read.
//!
//! | tool | takes | answers |
//! | --- | --- | --- |
//! | `get_active_file` | nothing | the active tab's path, cursor, viewport, dirty/pinned |
//! | `get_selection` | nothing | the current selection's range and text (capped) |
//! | `get_open_files` | nothing | every open tab: path, active, dirty, pinned |
//! | `get_recent_files` | nothing | recently edited files, most-recent first |
//!
//! # There are no arguments — the workspace is the boundary and it is baked in
//!
//! Unlike the Roslyn tools (which take a path plus a position-or-symbol), every
//! tool here reads the **live editor state** for the one workspace this server
//! was installed for. There is nothing to point at: the tools report what the
//! user is *already* looking at. So the schemas declare no properties, and
//! [`parse_call`] validates only the tool name — a client that sends spurious
//! arguments is not refused for them, because inventing a shape error over
//! arguments a tool ignores would be a lie about what the tool needs.
//!
//! The only two things that are protocol errors here, as in [`crate::mcp::tools`]
//! and [`super::super::roslyn::tools`], are a tool this server does not have.
//! Everything else — the feature switched off, no file active, an empty
//! selection — is a tool *refusal* or a genuine empty the application answers via
//! [`super::render`], never a shape error.

use rmcp::model::{ErrorCode, ErrorData, JsonObject, Tool};
use serde_json::{Map, Value};

pub const GET_ACTIVE_FILE: &str = "get_active_file";
pub const GET_SELECTION: &str = "get_selection";
pub const GET_OPEN_FILES: &str = "get_open_files";
pub const GET_RECENT_FILES: &str = "get_recent_files";

/// Every tool this server has. The listing and the dispatch read the same array,
/// so a tool cannot be advertised and unreachable, or reachable and unadvertised.
pub const ALL: [&str; 4] = [
    GET_ACTIVE_FILE,
    GET_SELECTION,
    GET_OPEN_FILES,
    GET_RECENT_FILES,
];

/// The sentence every tool description ends with — this is live editor state and
/// it can abstain honestly rather than fabricate a path or a position.
const ABSTAIN_SUFFIX: &str =
    " This reads the live editor state of the one workspace this server is \
     installed for. If no file is open, nothing is selected, or the feature is switched off, the \
     answer says so plainly rather than guessing a path or position.";

/// The tools, as the protocol describes them.
pub fn descriptors() -> Vec<Tool> {
    vec![
        no_args(
            GET_ACTIVE_FILE,
            "The file the user is currently focused on in the editor: its workspace-relative path, \
             the cursor position (line, 1-based; character, 0-based UTF-16 code units), the visible \
             line range, and whether the tab has unsaved changes or is pinned.",
        ),
        no_args(
            GET_SELECTION,
            "The user's current text selection: its start and end (line, 1-based; character, \
             0-based UTF-16 code units) and the selected text itself, which is length-capped.",
        ),
        no_args(
            GET_OPEN_FILES,
            "Every editor tab the user has open, each with its workspace-relative path and whether \
             it is the active tab, has unsaved changes, or is pinned.",
        ),
        no_args(
            GET_RECENT_FILES,
            "The files the user has edited most recently, in edit-recency order (most recent \
             first), by workspace-relative path.",
        ),
    ]
}

/// A tool that takes no arguments.
fn no_args(name: &'static str, description: &str) -> Tool {
    Tool::new(
        name,
        format!("{description}{ABSTAIN_SUFFIX}"),
        empty_schema(),
    )
}

/// The schema for a tool that takes no arguments: an object with no properties.
fn empty_schema() -> JsonObject {
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(Map::new()));
    schema.insert("required".to_string(), Value::Array(Vec::new()));
    schema
}

/// One call, already read. The dispatch matches on this rather than on strings,
/// so a tool that is added to [`ALL`] and forgotten is a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorToolCall {
    GetActiveFile,
    GetSelection,
    GetOpenFiles,
    GetRecentFiles,
}

impl EditorToolCall {
    /// The tool name this call was parsed from.
    pub fn name(self) -> &'static str {
        match self {
            Self::GetActiveFile => GET_ACTIVE_FILE,
            Self::GetSelection => GET_SELECTION,
            Self::GetOpenFiles => GET_OPEN_FILES,
            Self::GetRecentFiles => GET_RECENT_FILES,
        }
    }
}

/// Read a call, or produce the protocol error saying why it could not be read.
///
/// The only protocol error is a tool this server does not have. Arguments are
/// ignored: the tools take none, so a client sending some is not a shape error —
/// see the module docs.
pub fn parse_call(
    name: &str,
    _arguments: Option<&JsonObject>,
) -> Result<EditorToolCall, ErrorData> {
    match name {
        GET_ACTIVE_FILE => Ok(EditorToolCall::GetActiveFile),
        GET_SELECTION => Ok(EditorToolCall::GetSelection),
        GET_OPEN_FILES => Ok(EditorToolCall::GetOpenFiles),
        GET_RECENT_FILES => Ok(EditorToolCall::GetRecentFiles),
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            unknown_tool_sentence(unknown),
            None,
        )),
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
