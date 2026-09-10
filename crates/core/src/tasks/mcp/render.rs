//! Turning an outcome into the words an agent reads.
//!
//! The reader will act on this text without seeing the panel it came from, so —
//! as in [`crate::mcp::render`] — an empty list is stated as *the complete
//! answer* rather than left to look like something withheld, and every task
//! carries its id so the next call can name it exactly.

use crate::tasks::{Task, TaskOwner, TaskStatus};

use super::execute::Outcome;

/// The whole answer for one applied call.
pub fn outcome(outcome: &Outcome) -> String {
    match outcome {
        Outcome::List(tasks) => list(tasks),
        Outcome::One(task) => detail(task),
        Outcome::Created(task) => format!("Created task {}.\n{}", task.id, detail(task)),
        Outcome::Updated(task) => format!("Updated task {}.\n{}", task.id, detail(task)),
        Outcome::Completed(task) => format!("Marked task {} done.\n{}", task.id, detail(task)),
        Outcome::Assigned(task) => format!(
            "Assigned task {} to {}.\n{}",
            task.id,
            owner_word(task.owner),
            detail(task)
        ),
        Outcome::Deleted { id } => format!("Deleted task {id}."),
    }
}

/// The list, one task per line, or the explicit empty answer.
pub fn list(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return "This workspace has no tasks yet. This is the complete answer, not a truncated \
                one; use create_task to add one."
            .to_string();
    }
    tasks.iter().map(line).collect::<Vec<_>>().join("\n")
}

/// One task as a single line: id, status, owner, title.
pub fn line(task: &Task) -> String {
    format!(
        "{}  [{}]  owner:{}  {}",
        task.id,
        status_word(task.status),
        owner_word(task.owner),
        task.title
    )
}

/// One task in full, including its body.
pub fn detail(task: &Task) -> String {
    let mut out = line(task);
    if !task.body.is_empty() {
        out.push('\n');
        out.push_str(&task.body);
    }
    out
}

fn status_word(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Open => "open",
        TaskStatus::Done => "done",
    }
}

fn owner_word(owner: TaskOwner) -> &'static str {
    match owner {
        TaskOwner::Me => "me",
        TaskOwner::Ai => "ai",
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
