//! Git commands.
//!
//! A repository handle is opened per call rather than held in state: libgit2's
//! `Repository` is not `Sync`, and opening is cheap next to any operation
//! performed on it.

use std::collections::BTreeSet;

use cb_core::git::repo::NetworkOperation;
use cb_core::git::{Branch, Commit, ComparisonMode, FileDiff, Repo, WorkingStatus};
use cb_core::process::ProcessEvent;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

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

/// Both sides of a file, for the diff editor.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContents {
    /// What the file is being compared against. `None` for a new file.
    pub baseline: Option<String>,
    /// The file as it currently exists on disk. `None` if it was deleted.
    pub working: Option<String>,
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
#[tauri::command]
pub async fn git_write_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), String> {
    open(&state)?
        .write_working_file(&path, &content)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_stage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    open(&state)?.stage_file(&path).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn git_unstage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    open(&state)?.unstage_file(&path).map_err(|e| format!("{e:#}"))
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
    open(&state)?.discard_file(&path).map_err(|e| format!("{e:#}"))
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
    open(&state)?.delete_branch(&name).map_err(|e| format!("{e:#}"))
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
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkKind {
    Fetch,
    Pull,
    Push,
    /// Push a branch that has no upstream yet, establishing one.
    PushSetUpstream,
}

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
        let operation = match kind {
            NetworkKind::Fetch => NetworkOperation::Fetch,
            NetworkKind::Pull => NetworkOperation::Pull,
            NetworkKind::Push => NetworkOperation::Push,
            NetworkKind::PushSetUpstream => {
                let branch = repo
                    .status()
                    .map_err(|e| format!("{e:#}"))?
                    .branch
                    .ok_or_else(|| "cannot push a detached HEAD".to_string())?;
                NetworkOperation::PushSetUpstream(branch)
            }
        };
        repo.network_command(operation)
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
