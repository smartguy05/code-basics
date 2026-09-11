//! Every answer this server can give that is not data — and they are all
//! different answers.
//!
//! The same abstain-rather-than-guess rule the SQL server applies
//! ([`crate::mcp::answer`]), for the same reason: a model reads the sentence,
//! believes it, and acts, so the distinctions a human would have recovered from
//! unaided are the ones kept apart here.
//!
//! * *no workspace scope* is not *the task is missing*. The first says this
//!   server was started without a `--workspace`, so there is no store to reach
//!   at all; the second says the store was read and did not contain that task.
//! * a write that could not be persisted crosses as
//!   [`McpRefusal::StoreWriteFailed`] with **no filesystem detail** — the same
//!   rule the SQL server applies to driver text, because the party on the other
//!   end of this transport pastes what it is told into a transcript and a
//!   provider's logs.

/// Why a tool call produced no data.
///
/// Every variant is rendered as an MCP **tool execution error**, so the model
/// sees it and can correct itself. Protocol errors are reserved for the two
/// things a model cannot fix by reasoning: an unknown tool name, and arguments
/// that are not the shape the schema declared (see [`super::tools`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRefusal {
    /// This server was started without a `--workspace`, so there is no
    /// per-workspace task store to read or write. Distinct from
    /// [`McpRefusal::TaskNotFound`]: the fix is to install the server scoped to
    /// a repository, not to name a different task.
    NoWorkspace,
    /// The store was read and contains no task with this id. Not a silent
    /// no-op: an update or delete against a vanished task is reported so the
    /// agent knows nothing changed.
    TaskNotFound { id: String },
    /// A mutation was applied in memory but could not be written back to disk.
    /// The filesystem's own words are deliberately **not** forwarded — a path
    /// or an OS error routinely names a home directory and a layout this
    /// interface has no business leaking. The person who owns the workspace can
    /// see the real error in the app.
    StoreWriteFailed,
}

impl McpRefusal {
    /// A short machine-matchable name, so a model can branch without parsing
    /// prose.
    pub fn code(&self) -> &'static str {
        match self {
            McpRefusal::NoWorkspace => "noWorkspace",
            McpRefusal::TaskNotFound { .. } => "taskNotFound",
            McpRefusal::StoreWriteFailed => "storeWriteFailed",
        }
    }

    /// The sentence the model reads.
    pub fn sentence(&self) -> String {
        match self {
            McpRefusal::NoWorkspace => "This task server was started without a --workspace, so \
                 there is no task list to read or write. The server has to be installed scoped to \
                 a repository; a task list is per-workspace and personal."
                .to_string(),
            McpRefusal::TaskNotFound { id } => {
                format!("No task has the id {id:?}. Use list_tasks to see the tasks that exist.")
            }
            McpRefusal::StoreWriteFailed => "The change was made but could not be saved to the \
                 task file, so it has not taken effect. The filesystem's own error is deliberately \
                 not forwarded here; the person who owns this workspace can see it in the app."
                .to_string(),
        }
    }
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
