//! Git commands.
//!
//! A repository handle is opened per call rather than held in state: libgit2's
//! `Repository` is not `Sync`, and opening is cheap next to any operation
//! performed on it.

use std::collections::BTreeSet;

use cb_core::git::repo::{resolve_network, MergeReport};
use cb_core::git::{Branch, Commit, ComparisonMode, FileContents, FileDiff, Repo, WorkingStatus};
use cb_core::process::ProcessEvent;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::commands::files::reindex_saved_file;
use crate::state::AppState;

fn open(state: &State<'_, AppState>) -> Result<Repo, String> {
    let root = state.workspace_root()?;
    Repo::open(&root).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_status(state: State<'_, AppState>) -> Result<WorkingStatus, String> {
    open(&state)?.status().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_file_diff(
    state: State<'_, AppState>,
    path: String,
    mode: ComparisonMode,
) -> Result<FileDiff, String> {
    open(&state)?
        .file_diff(&path, mode)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_file_contents(
    state: State<'_, AppState>,
    path: String,
    mode: ComparisonMode,
) -> Result<FileContents, String> {
    let repo = open(&state)?;

    Ok(FileContents {
        baseline: repo
            .baseline_content(&path, mode)
            .map_err(|e| format!("{e:#}"))?,
        working: repo.working_content(&path).map_err(|e| format!("{e:#}"))?,
    })
}

/// Save an edit made inside the diff view.
///
/// Re-indexes the file afterwards for the same reason `fs_write_file` does —
/// see `reindex_saved_file`. This is the editor most likely to rename a symbol,
/// since it is the one a review is done in.
///
/// `path` is relative to the repository, not to the workspace, and the two are
/// only the same when the user opened the repository root. `Repo::open`
/// discovers the repository at or above the opened directory, so opening
/// `C:/repo/src/Api` leaves every path in this tab prefixed with `src/Api/`.
/// Resolving it against the working directory — the root it is actually
/// relative to — is what `write_working_file` does one line above, and handing
/// the re-index the same absolute path is what stops the two disagreeing.
#[tauri::command]
pub async fn git_write_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), String> {
    let repo = open(&state)?;
    repo.write_working_file(&path, &content)
        .map_err(|e| format!("{e:#}"))?;
    reindex_saved_file(&state, &repo.workdir().join(&path));
    Ok(())
}

#[tauri::command]
pub async fn git_stage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    open(&state)?
        .stage_file(&path)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_unstage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    open(&state)?
        .unstage_file(&path)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_stage_lines(
    state: State<'_, AppState>,
    path: String,
    lines: Vec<u32>,
) -> Result<bool, String> {
    open(&state)?
        .stage_lines(&path, &lines.into_iter().collect::<BTreeSet<_>>())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_unstage_lines(
    state: State<'_, AppState>,
    path: String,
    lines: Vec<u32>,
) -> Result<bool, String> {
    open(&state)?
        .unstage_lines(&path, &lines.into_iter().collect::<BTreeSet<_>>())
        .map_err(|e| format!("{e:#}"))
}

/// Undo selected lines, returning them to whatever the mode's baseline is.
#[tauri::command]
pub async fn git_revert_lines(
    state: State<'_, AppState>,
    path: String,
    mode: ComparisonMode,
    lines: Vec<u32>,
) -> Result<bool, String> {
    open(&state)?
        .revert_lines(&path, mode, &lines.into_iter().collect::<BTreeSet<_>>())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_discard_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    open(&state)?
        .discard_file(&path)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_commit(
    state: State<'_, AppState>,
    message: String,
    amend: bool,
) -> Result<String, String> {
    open(&state)?
        .commit(&message, amend)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_branches(state: State<'_, AppState>) -> Result<Vec<Branch>, String> {
    open(&state)?.branches().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_create_branch(
    state: State<'_, AppState>,
    name: String,
    checkout: bool,
    // Revision to branch from. Absent means HEAD.
    from: Option<String>,
) -> Result<(), String> {
    let repo = open(&state)?;
    match from {
        Some(start) => repo.create_branch_from(&name, &start, checkout),
        None => repo.create_branch(&name, checkout),
    }
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_checkout_branch(state: State<'_, AppState>, name: String) -> Result<(), String> {
    open(&state)?
        .checkout_branch(&name)
        .map_err(|e| format!("{e:#}"))
}

/// Check out a remote-tracking branch like `git switch`: create the local
/// branch with its upstream set (or reuse an existing one), then switch.
#[tauri::command]
pub async fn git_checkout_remote_branch(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    open(&state)?
        .checkout_remote_branch(&name)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_delete_branch(state: State<'_, AppState>, name: String) -> Result<(), String> {
    open(&state)?
        .delete_branch(&name)
        .map_err(|e| format!("{e:#}"))
}

/// Merge a branch into the current one.
///
/// A conflicted merge is reported rather than aborted, and left in progress so
/// the conflicts can be resolved in the Changes tab — `git_abort_merge` backs
/// out instead.
#[tauri::command]
pub async fn git_merge_branch(
    state: State<'_, AppState>,
    name: String,
) -> Result<MergeReport, String> {
    open(&state)?
        .merge_branch(&name)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_abort_merge(state: State<'_, AppState>) -> Result<(), String> {
    open(&state)?.abort_merge().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_history(state: State<'_, AppState>, limit: u32) -> Result<Vec<Commit>, String> {
    open(&state)?
        .history(limit as usize)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_commit_diff(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<FileDiff>, String> {
    open(&state)?.commit_diff(&id).map_err(|e| format!("{e:#}"))
}

/// Both sides of one file as a commit changed it, for the History tab's diff
/// viewer. `git_commit_diff` above says which files a commit touched; this says
/// what they looked like either side of it.
#[tauri::command]
pub async fn git_commit_file_contents(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<FileContents, String> {
    open(&state)?
        .commit_file_contents(&id, &path)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_stash_save(state: State<'_, AppState>, message: String) -> Result<(), String> {
    open(&state)?
        .stash_save(&message)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_stash_pop(state: State<'_, AppState>) -> Result<(), String> {
    open(&state)?.stash_pop().map_err(|e| format!("{e:#}"))
}

/// Which network operation to perform.
///
/// Re-exported rather than declared here so the IPC deserialisation surface is
/// unchanged while the mapping it feeds lives in `cb-core`.
pub use cb_core::git::repo::NetworkKind;

/// Run fetch, pull or push through the system `git`, streaming its progress.
///
/// Deliberately not libgit2: the system `git` already has the user's SSH
/// agent, keychain and credential helpers configured, so authentication just
/// works without this app ever handling a credential.
#[tauri::command]
pub async fn git_network(
    state: State<'_, AppState>,
    kind: NetworkKind,
    channel: Channel<ProcessEvent>,
) -> Result<Option<i32>, String> {
    let invocation = {
        let repo = open(&state)?;
        // Only the upstream-setting push needs the branch, and status() is not
        // free, so it stays behind that arm.
        let current_branch = match kind {
            NetworkKind::PushSetUpstream => repo.status().map_err(|e| format!("{e:#}"))?.branch,
            _ => None,
        };
        repo.network_command(resolve_network(kind, current_branch)?)
    };

    let (tx, rx) = mpsc::channel(256);
    let forwarder = channel.clone();
    tokio::spawn(async move {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            if forwarder.send(event).is_err() {
                break;
            }
        }
    });

    state
        .supervisor
        .run("git:network", &invocation, tx)
        .await
        .map_err(|e| format!("{e:#}"))
}
