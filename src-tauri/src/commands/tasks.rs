//! Commands behind the per-workspace Tasks panel and its installable MCP server.
//!
//! Two families of command, both thin:
//!
//! - **CRUD** — [`read_tasks`] plus the granular writes. The store — where the
//!   file lives, how a missing or corrupt one is tolerated, how a task is
//!   created or changed — is all in [`cb_core::tasks`]. These commands take an
//!   explicit `root` rather than reading the active workspace: tasks are
//!   per-repository and several codebases can be open at once, so the panel that
//!   issued the call names its own workspace. The clock is stamped **here at the
//!   command edge**, never in `cb-core`, so the pure helpers stay testable with a
//!   fixed time.
//! - **The MCP install bridge** — the same five-command shape as
//!   [`crate::commands::mcp`] and [`crate::commands::browser_mcp`], over
//!   [`cb_core::tasks::mcp::install`], which reuses `mcp::install`'s merge
//!   wholesale. Writes go through [`providers::apply_writes_atomically`] for the
//!   same reason as there: `~/.claude.json` is 122 KB and rewritten continuously
//!   by a running Claude Code.
//!
//! Per the `commands/lsp.rs::status_for` rule, every body that *decides*
//! anything delegates to a plain free function tested below; a command body only
//! resolves its arguments, stamps the clock, and awaits.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cb_core::intents::providers::{self, InstallPlan, InstallScope};
use cb_core::intents::ProviderId;
use cb_core::tasks::mcp::install as tasks_install;
use cb_core::tasks::{self, TaskOwner, TaskStatus, TasksFile};

/// What a write refuses with when the named task is not in the store.
const TASK_NOT_FOUND: &str = "no task with that id";

// ---------------------------------------------------------------------------
// CRUD commands
// ---------------------------------------------------------------------------

/// Read this workspace's task list. A missing or unreadable file is an empty
/// list, not an error — [`cb_core::tasks::load`] tolerates both.
#[tauri::command]
pub async fn read_tasks(root: PathBuf) -> Result<TasksFile, String> {
    Ok(tasks::load(&tasks::tasks_path(&root)))
}

/// Create a new open task owned by the user, returning the updated list.
#[tauri::command]
pub async fn create_task(root: PathBuf, title: String, body: String) -> Result<TasksFile, String> {
    create_at(&tasks::tasks_path(&root), &new_id(), title, body, now_ms())
}

/// Overwrite a task's title and body, returning the updated list.
#[tauri::command]
pub async fn update_task(
    root: PathBuf,
    id: String,
    title: String,
    body: String,
) -> Result<TasksFile, String> {
    update_at(&tasks::tasks_path(&root), &id, title, body, now_ms())
}

/// Assign a task to the user or the agent, returning the updated list. Launching
/// the agent when the owner becomes the AI is the frontend's job; this only
/// records the owner.
#[tauri::command]
pub async fn assign_task(root: PathBuf, id: String, owner: TaskOwner) -> Result<TasksFile, String> {
    set_owner_at(&tasks::tasks_path(&root), &id, owner, now_ms())
}

/// Set a task's status (done or back to open), returning the updated list.
#[tauri::command]
pub async fn complete_task(
    root: PathBuf,
    id: String,
    status: TaskStatus,
) -> Result<TasksFile, String> {
    set_status_at(&tasks::tasks_path(&root), &id, status, now_ms())
}

/// Remove a task, returning the updated list.
#[tauri::command]
pub async fn delete_task(root: PathBuf, id: String) -> Result<TasksFile, String> {
    remove_at(&tasks::tasks_path(&root), &id)
}

// ---------------------------------------------------------------------------
// The store-mutation free functions — where the decisions live, tested below
// ---------------------------------------------------------------------------

/// Load the store, append a new task, and persist. Create cannot fail to find a
/// task, so it has no not-found path.
fn create_at(
    path: &Path,
    id: &str,
    title: String,
    body: String,
    now: u64,
) -> Result<TasksFile, String> {
    let mut file = tasks::load(path);
    file.create(id, title, body, TaskOwner::Me, now);
    persist(path, file)
}

/// Load, update the named task, and persist. A missing id is refused rather than
/// saving an unchanged file and reporting success.
fn update_at(
    path: &Path,
    id: &str,
    title: String,
    body: String,
    now: u64,
) -> Result<TasksFile, String> {
    apply_at(path, |file| file.update(id, title, body, now))
}

/// Load, set the named task's owner, and persist.
fn set_owner_at(path: &Path, id: &str, owner: TaskOwner, now: u64) -> Result<TasksFile, String> {
    apply_at(path, |file| file.set_owner(id, owner, now))
}

/// Load, set the named task's status, and persist.
fn set_status_at(path: &Path, id: &str, status: TaskStatus, now: u64) -> Result<TasksFile, String> {
    apply_at(path, |file| file.set_status(id, status, now))
}

/// Load, remove the named task, and persist.
fn remove_at(path: &Path, id: &str) -> Result<TasksFile, String> {
    apply_at(path, |file| file.remove(id))
}

/// The shared load → mutate → persist path for every write that targets an
/// existing task. `change` returns whether it found and changed a task; when it
/// did not, the store is left untouched and the call refuses — a write that
/// silently no-ops would report success for a task that is not there.
fn apply_at(path: &Path, change: impl FnOnce(&mut TasksFile) -> bool) -> Result<TasksFile, String> {
    let mut file = tasks::load(path);
    if !change(&mut file) {
        return Err(TASK_NOT_FOUND.to_string());
    }
    persist(path, file)
}

/// Write the store back and hand the saved list to the caller. The filesystem's
/// own error is surfaced (unlike the MCP path, which drops it) because this is a
/// direct user action and the panel can show what went wrong.
fn persist(path: &Path, file: TasksFile) -> Result<TasksFile, String> {
    tasks::save(path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(file)
}

/// The wall clock, in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A fresh task id. A uuid so two writers (the panel and an agent) creating a
/// task in the same millisecond cannot collide.
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ---------------------------------------------------------------------------
// The MCP install bridge — plan, confirm, apply, re-read the status from disk
// ---------------------------------------------------------------------------

/// Where the Tasks MCP server is installed for this workspace and `provider`, if
/// anywhere. Project scope wins over user scope, as it does for the hooks.
#[tauri::command]
pub async fn tasks_mcp_status(
    root: PathBuf,
    provider: ProviderId,
) -> Result<Option<InstallScope>, String> {
    Ok(tasks_install::status_for(provider, &root, None))
}

/// Exactly what installing the server for `provider` at `scope` would write.
/// Touches nothing.
#[tauri::command]
pub async fn tasks_mcp_install_plan(
    root: PathBuf,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    tasks_install::install_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform an install the user has confirmed, returning the new status.
#[tauri::command]
pub async fn install_tasks_mcp_server(
    root: PathBuf,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let plan = tasks_install::install_plan_for(provider, &root, scope, None)
        .map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(tasks_install::status_for(provider, &root, None))
}

/// Exactly what removing the server for `provider` at `scope` would rewrite.
/// Touches nothing. A zero-write plan means there was nothing of ours there.
#[tauri::command]
pub async fn tasks_mcp_uninstall_plan(
    root: PathBuf,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    tasks_install::uninstall_plan_for(provider, &root, scope, None).map_err(|e| format!("{e:#}"))
}

/// Perform a removal the user has confirmed, returning the new status.
#[tauri::command]
pub async fn uninstall_tasks_mcp_server(
    root: PathBuf,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Option<InstallScope>, String> {
    let plan = tasks_install::uninstall_plan_for(provider, &root, scope, None)
        .map_err(|e| format!("{e:#}"))?;
    providers::apply_writes_atomically(&plan.writes).map_err(|e| format!("{e:#}"))?;
    Ok(tasks_install::status_for(provider, &root, None))
}

// A `#[tauri::command]` body cannot be reached from a test (it needs a
// `tauri::App` to build its `State`), so the CRUD bodies keep no decision the
// free functions above do not already make; those are what the tests exercise.
// The MCP bridge commands hold no decision at all — they resolve, plan, apply
// and re-read — so they are covered by `cb_core::tasks::mcp::install`'s own
// tests, exactly as `commands/mcp.rs` is by `cb_core::mcp::install`.
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store_path(dir: &TempDir) -> PathBuf {
        dir.path().join("tasks.json")
    }

    #[test]
    fn create_appends_and_persists_a_user_owned_open_task() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let file = create_at(&path, "id-1", "Title".into(), "Body".into(), 1_000).unwrap();
        assert_eq!(file.tasks.len(), 1);
        let task = &file.tasks[0];
        assert_eq!(task.id, "id-1");
        assert_eq!(task.title, "Title");
        assert_eq!(task.owner, TaskOwner::Me);
        assert_eq!(task.status, TaskStatus::Open);
        assert_eq!(task.created_at_ms, 1_000);
        assert_eq!(task.updated_at_ms, 1_000);

        // It was written, not merely returned: a fresh load sees it.
        let reloaded = tasks::load(&path);
        assert_eq!(reloaded, file);
    }

    #[test]
    fn create_preserves_existing_tasks_and_their_order() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        create_at(&path, "a", "First".into(), String::new(), 1).unwrap();
        let file = create_at(&path, "b", "Second".into(), String::new(), 2).unwrap();
        assert_eq!(
            file.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn update_changes_the_named_task_and_stamps_the_clock() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        create_at(&path, "id-1", "Old".into(), "Old body".into(), 1_000).unwrap();
        let file = update_at(&path, "id-1", "New".into(), "New body".into(), 2_000).unwrap();
        let task = &file.tasks[0];
        assert_eq!(task.title, "New");
        assert_eq!(task.body, "New body");
        assert_eq!(task.created_at_ms, 1_000, "create time is preserved");
        assert_eq!(task.updated_at_ms, 2_000, "update time is stamped");
    }

    #[test]
    fn a_write_to_a_missing_task_is_refused_and_leaves_the_store_untouched() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        create_at(&path, "id-1", "Keep".into(), String::new(), 1_000).unwrap();

        for result in [
            update_at(&path, "nope", "x".into(), "y".into(), 2_000),
            set_owner_at(&path, "nope", TaskOwner::Ai, 2_000),
            set_status_at(&path, "nope", TaskStatus::Done, 2_000),
            remove_at(&path, "nope"),
        ] {
            assert_eq!(result.unwrap_err(), TASK_NOT_FOUND);
        }

        // The one real task is exactly as it was — no field was touched.
        let reloaded = tasks::load(&path);
        assert_eq!(reloaded.tasks.len(), 1);
        assert_eq!(reloaded.tasks[0].updated_at_ms, 1_000);
    }

    #[test]
    fn assign_flips_the_owner_both_ways() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        create_at(&path, "id-1", "T".into(), String::new(), 1_000).unwrap();

        let to_ai = set_owner_at(&path, "id-1", TaskOwner::Ai, 2_000).unwrap();
        assert_eq!(to_ai.tasks[0].owner, TaskOwner::Ai);
        assert_eq!(to_ai.tasks[0].updated_at_ms, 2_000);

        let back = set_owner_at(&path, "id-1", TaskOwner::Me, 3_000).unwrap();
        assert_eq!(back.tasks[0].owner, TaskOwner::Me);
    }

    #[test]
    fn status_can_complete_and_reopen() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        create_at(&path, "id-1", "T".into(), String::new(), 1_000).unwrap();

        let done = set_status_at(&path, "id-1", TaskStatus::Done, 2_000).unwrap();
        assert_eq!(done.tasks[0].status, TaskStatus::Done);

        let reopened = set_status_at(&path, "id-1", TaskStatus::Open, 3_000).unwrap();
        assert_eq!(reopened.tasks[0].status, TaskStatus::Open);
    }

    #[test]
    fn delete_removes_only_the_named_task() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        create_at(&path, "a", "A".into(), String::new(), 1).unwrap();
        create_at(&path, "b", "B".into(), String::new(), 2).unwrap();

        let file = remove_at(&path, "a").unwrap();
        assert_eq!(
            file.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
        assert_eq!(tasks::load(&path).tasks.len(), 1);
    }
}
