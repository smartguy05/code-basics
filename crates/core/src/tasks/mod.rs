//! The per-workspace task store, `.code-basics/tasks.json`.
//!
//! Unlike [`crate::notes`], which is user-global, tasks are **per-repository and
//! personal**: the file lives under the opened workspace's `.code-basics/`
//! directory and is **gitignored** (see the `IGNORED` list in [`crate::config`]),
//! so one developer's task list never reaches their team through the repository.
//!
//! Pure filesystem/serde work with no Tauri dependency, tested headlessly. The
//! path is resolved by [`tasks_path`] and passed into [`load`]/[`save`], so tests
//! drive the store against a temp file without touching a real workspace (and
//! without racing on a shared environment variable). The CRUD helpers take the
//! clock and any new id as arguments rather than calling `now()` themselves, so a
//! test pins exact timestamps.

pub mod mcp;
pub mod store;

pub use store::{load, save, tasks_path, TASKS_FILE};

use serde::{Deserialize, Serialize};
use specta::Type;

/// Whether a task is still to do or finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    /// Still to do.
    Open,
    /// Finished.
    Done,
}

/// Who a task is currently assigned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TaskOwner {
    /// The human user owns it.
    Me,
    /// Assigned to the coding agent.
    Ai,
}

/// One task in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Stable id: the React key and the target of update/status/owner/delete.
    pub id: String,
    /// The short one-line title.
    pub title: String,
    /// The longer description / prompt body.
    pub body: String,
    /// Whether the task is open or done.
    pub status: TaskStatus,
    /// Who the task is assigned to.
    pub owner: TaskOwner,
    /// When the task was created, milliseconds since the Unix epoch. A number so
    /// the frontend can format it however it likes; the clock is the caller's.
    pub created_at_ms: u64,
    /// When the task was last changed, milliseconds since the Unix epoch.
    pub updated_at_ms: u64,
}

/// The whole tasks file: a schema version and the ordered tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TasksFile {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    /// The tasks, in the order the panel shows them.
    #[serde(default)]
    pub tasks: Vec<Task>,
}

pub(crate) fn default_version() -> u32 {
    1
}

impl Default for TasksFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            tasks: Vec::new(),
        }
    }
}

impl TasksFile {
    /// Add a new open task owned by `owner`, created and updated at `now`. The new
    /// task is appended, so the list keeps insertion order. Returns a reference to
    /// the created task.
    pub fn create(
        &mut self,
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        owner: TaskOwner,
        now: u64,
    ) -> &Task {
        self.tasks.push(Task {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            status: TaskStatus::Open,
            owner,
            created_at_ms: now,
            updated_at_ms: now,
        });
        self.tasks.last().expect("just pushed")
    }

    /// Overwrite the title and body of the task with `id`, stamping `updated_at_ms`
    /// to `now`. Returns `true` if a task was found and changed.
    pub fn update(
        &mut self,
        id: &str,
        title: impl Into<String>,
        body: impl Into<String>,
        now: u64,
    ) -> bool {
        match self.tasks.iter_mut().find(|t| t.id == id) {
            Some(task) => {
                task.title = title.into();
                task.body = body.into();
                task.updated_at_ms = now;
                true
            }
            None => false,
        }
    }

    /// Set the status of the task with `id`, stamping `updated_at_ms` to `now`.
    /// Returns `true` if a task was found.
    pub fn set_status(&mut self, id: &str, status: TaskStatus, now: u64) -> bool {
        match self.tasks.iter_mut().find(|t| t.id == id) {
            Some(task) => {
                task.status = status;
                task.updated_at_ms = now;
                true
            }
            None => false,
        }
    }

    /// Set the owner of the task with `id`, stamping `updated_at_ms` to `now`.
    /// Returns `true` if a task was found.
    pub fn set_owner(&mut self, id: &str, owner: TaskOwner, now: u64) -> bool {
        match self.tasks.iter_mut().find(|t| t.id == id) {
            Some(task) => {
                task.owner = owner;
                task.updated_at_ms = now;
                true
            }
            None => false,
        }
    }

    /// Remove the task with `id`. Returns `true` if a task was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        self.tasks.len() != before
    }
}

#[cfg(test)]
#[path = "tasks_tests.rs"]
mod tests;
