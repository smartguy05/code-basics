//! Applying one tool call to the task store.
//!
//! This is the Tasks server's decision core, and the counterpart of the SQL
//! server's [`crate::mcp::execute`] — except there is nothing to *forbid* here.
//! The SQL version's whole job is forcing read-only; this one has no such gate,
//! because a write is the point (see the module docs). What it does instead is
//! turn a [`ToolCall`] into either a read or a mutation of a [`TasksFile`],
//! abstaining with an [`McpRefusal`] when a named task is not there.
//!
//! It is pure and headlessly tested: the clock and any new id are **arguments**,
//! not calls to `now()` or a uuid generator, so a test pins exact timestamps and
//! ids. The runner in the app crate injects the real clock and a fresh uuid, and
//! is the only thing that touches the disk — this returns [`Outcome::mutated`]
//! to tell it whether a save is owed.

use crate::tasks::{Task, TaskOwner, TaskStatus, TasksFile};

use super::answer::McpRefusal;
use super::tools::ToolCall;

/// What a successfully applied call produced.
///
/// Rendering lives in [`super::render`]; this only carries the data. A variant
/// per verb rather than one `Task` field, so the renderer can word "created"
/// differently from "assigned" and a reader is told what actually happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    List(Vec<Task>),
    One(Task),
    Created(Task),
    Updated(Task),
    Completed(Task),
    Deleted { id: String },
    Assigned(Task),
}

impl Outcome {
    /// Whether the store must be written back. The two reads
    /// ([`Outcome::List`], [`Outcome::One`]) change nothing; everything else
    /// mutated the file and the caller owes it a save.
    pub fn mutated(&self) -> bool {
        !matches!(self, Outcome::List(_) | Outcome::One(_))
    }
}

/// Apply `call` to `file`, mutating it in place when the call is a write.
///
/// `now` stamps any created/changed task; `new_id` is the id a `create` gives
/// the new task. Both are the caller's, so this stays pure.
pub fn apply(
    file: &mut TasksFile,
    call: ToolCall,
    now: u64,
    new_id: &str,
) -> Result<Outcome, McpRefusal> {
    match call {
        ToolCall::ListTasks => Ok(Outcome::List(file.tasks.clone())),

        ToolCall::GetTask { id } => find(file, &id).map(Outcome::One),

        ToolCall::CreateTask { title, body } => {
            let task = file
                .create(new_id, title, body.unwrap_or_default(), TaskOwner::Me, now)
                .clone();
            Ok(Outcome::Created(task))
        }

        ToolCall::UpdateTask { id, title, body } => {
            // An omitted field keeps the current value; only a present field
            // (including an empty string) changes it. So the existing task is
            // read first, and the missing halves are filled from it.
            let current = find(file, &id)?;
            let new_title = title.unwrap_or_else(|| current.title.clone());
            let new_body = body.unwrap_or_else(|| current.body.clone());
            file.update(&id, new_title, new_body, now);
            Ok(Outcome::Updated(find(file, &id)?))
        }

        ToolCall::CompleteTask { id } => {
            if !file.set_status(&id, TaskStatus::Done, now) {
                return Err(McpRefusal::TaskNotFound { id });
            }
            Ok(Outcome::Completed(find(file, &id)?))
        }

        ToolCall::DeleteTask { id } => {
            if !file.remove(&id) {
                return Err(McpRefusal::TaskNotFound { id });
            }
            Ok(Outcome::Deleted { id })
        }

        ToolCall::AssignTask { id, owner } => {
            if !file.set_owner(&id, owner, now) {
                return Err(McpRefusal::TaskNotFound { id });
            }
            Ok(Outcome::Assigned(find(file, &id)?))
        }
    }
}

/// The task with `id`, cloned, or the not-found refusal.
fn find(file: &TasksFile, id: &str) -> Result<Task, McpRefusal> {
    file.tasks
        .iter()
        .find(|task| task.id == id)
        .cloned()
        .ok_or_else(|| McpRefusal::TaskNotFound { id: id.to_string() })
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;
