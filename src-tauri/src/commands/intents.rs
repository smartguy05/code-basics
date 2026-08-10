//! Agent-intent commands.
//!
//! Grouping is recomputed on every call rather than cached. It is cheap —
//! short-string hashing over a diff libgit2 has already produced — and the
//! alternative is a cache that has to be invalidated by every edit, stage,
//! revert and branch switch. A stale group is worse than a recomputed one:
//! it would offer to stage lines that have moved.
//!
//! Nothing here persists line indices, because they are only meaningful
//! within one `ComparisonMode`. A group's lines are re-derived for whichever
//! mode the requested operation needs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cb_core::git::attribution::{self, Options};
use cb_core::git::grouping::{self, GroupFile, IntentGroup};
use cb_core::git::{ComparisonMode, Repo};
use cb_core::intents::providers::{self, InstallPlan, InstallScope, ProviderStatus};
use cb_core::intents::reject::{self, RejectSummary};
use cb_core::intents::{self, guard, LoadOptions, ProviderId};
use tauri::State;

use crate::state::AppState;

fn open(state: &State<'_, AppState>) -> Result<(PathBuf, Repo), String> {
    let root = state.workspace_root()?;
    let repo = Repo::open(&root).map_err(|e| format!("{e:#}"))?;
    Ok((root, repo))
}

/// Build the cards for the whole working tree.
#[tauri::command]
pub async fn intent_groups(
    state: State<'_, AppState>,
    mode: ComparisonMode,
) -> Result<Vec<IntentGroup>, String> {
    let (root, repo) = open(&state)?;

    let diffs = repo.diff_all(mode).map_err(|e| format!("{e:#}"))?;

    // Records made on another branch describe code that is not in this tree.
    let branch = repo.status().ok().and_then(|s| s.branch);
    let intents = intents::load(&root, &LoadOptions { branch }).map_err(|e| format!("{e:#}"))?;

    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    Ok(grouping::group(&diffs, &attributions))
}

/// The line indices a group covers, recomputed for one comparison mode.
///
/// Indices from the mode the cards were built in do not carry over: staging
/// works against the index, reverting against whatever the user is looking at.
/// Rather than trusting the frontend to keep them straight, every action
/// re-derives them here from a fresh diff.
/// Returns the group's own [`GroupFile`]s rather than just line numbers,
/// because rejecting also needs the hunk indices: a note is placed per hunk,
/// and line indices alone cannot say where a hunk began.
fn lines_for(
    repo: &Repo,
    root: &Path,
    group_id: &str,
    mode: ComparisonMode,
) -> Result<Vec<GroupFile>, String> {
    let diffs = repo.diff_all(mode).map_err(|e| format!("{e:#}"))?;
    let branch = repo.status().ok().and_then(|s| s.branch);
    let intents = intents::load(root, &LoadOptions { branch }).map_err(|e| format!("{e:#}"))?;

    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    let groups = grouping::group(&diffs, &attributions);

    let group = groups
        .into_iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| "that group is no longer in the working tree".to_string())?;

    Ok(group.files)
}

fn selected(file: &GroupFile) -> BTreeSet<u32> {
    file.line_indices.iter().copied().collect()
}

/// Stage everything in one group — or, with `path`, just that file's share.
///
/// Returns how many files were changed, so the UI can say nothing happened
/// rather than silently appearing to succeed.
#[tauri::command]
pub async fn stage_intent_group(
    state: State<'_, AppState>,
    group: String,
    path: Option<String>,
) -> Result<usize, String> {
    let (root, repo) = open(&state)?;
    // Staging compares the working tree against the index, whatever the user
    // happens to be looking at.
    let mut files = lines_for(&repo, &root, &group, ComparisonMode::WorkingToIndex)?;
    if let Some(path) = path {
        files.retain(|f| f.path == path);
    }

    let mut staged = 0;
    for file in files {
        if repo
            .stage_lines(&file.path, &selected(&file))
            .map_err(|e| format!("{e:#}"))?
        {
            staged += 1;
        }
    }
    Ok(staged)
}

/// Revert one group, in the mode the user is looking at — or, with `path`,
/// just that file's share of it.
#[tauri::command]
pub async fn revert_intent_group(
    state: State<'_, AppState>,
    group: String,
    mode: ComparisonMode,
    path: Option<String>,
) -> Result<usize, String> {
    let (root, repo) = open(&state)?;
    let mut files = lines_for(&repo, &root, &group, mode)?;
    if let Some(path) = path {
        files.retain(|f| f.path == path);
    }

    let mut reverted = 0;
    for file in files {
        if repo
            .revert_lines(&file.path, mode, &selected(&file))
            .map_err(|e| format!("{e:#}"))?
        {
            reverted += 1;
        }
    }
    Ok(reverted)
}

/// Reject one group: revert it, and leave the reason where the code was.
///
/// Only the working-tree views can be rejected. In the staged view a revert
/// changes the index, so a note written into the working tree would explain a
/// change the reviewer is not looking at — and would itself be unstaged.
#[tauri::command]
pub async fn reject_intent_group(
    state: State<'_, AppState>,
    group: String,
    mode: ComparisonMode,
    path: Option<String>,
    reason: String,
) -> Result<RejectSummary, String> {
    if matches!(mode, ComparisonMode::IndexToHead) {
        return Err(
            "rejecting works on the working tree — switch out of the staged view first".into(),
        );
    }

    let reason = reject::sanitise_reason(&reason);
    if reason.is_empty() {
        return Err("a rejection needs a reason: it is the whole point of it".into());
    }

    let (root, repo) = open(&state)?;
    let mut files = lines_for(&repo, &root, &group, mode)?;
    if let Some(path) = path {
        files.retain(|f| f.path == path);
    }

    // One clock reading for the whole group, so every note it writes carries
    // the same date.
    let at = SystemTime::now();

    let mut summary = RejectSummary::default();
    for file in files {
        let outcome = reject::reject_file(
            &repo,
            &file.path,
            mode,
            &selected(&file),
            &file.hunks,
            &reason,
            at,
        )
        .map_err(|e| format!("{e:#}"))?;

        summary.record(outcome);
    }

    Ok(summary)
}

/// What each agent can currently do for this workspace.
#[tauri::command]
pub async fn intent_capture_status(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, String> {
    let root = state.workspace_root()?;
    Ok(providers::statuses(&root))
}

/// Exactly what enabling capture would write. Touches nothing.
#[tauri::command]
pub async fn intent_install_plan(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;

    providers::all()
        .iter()
        .find(|p| p.id() == provider)
        .ok_or_else(|| "that agent is not supported".to_string())?
        .install_plan(&root, scope)
        .map_err(|e| format!("{e:#}"))
}

/// Perform an install the user has confirmed.
#[tauri::command]
pub async fn enable_intent_capture(
    state: State<'_, AppState>,
    provider: ProviderId,
    scope: InstallScope,
) -> Result<Vec<ProviderStatus>, String> {
    let root = state.workspace_root()?;

    let plan = providers::all()
        .iter()
        .find(|p| p.id() == provider)
        .ok_or_else(|| "that agent is not supported".to_string())?
        .install_plan(&root, scope)
        .map_err(|e| format!("{e:#}"))?;

    providers::apply_plan(&plan).map_err(|e| format!("{e:#}"))?;

    // The commit guard is a shell script, and on unix git ignores one it cannot
    // execute. The mode is not part of what the user previewed, so it is set
    // here rather than carried through the plan.
    if let Some(hook) = guard::hook_path(&root) {
        if guard::is_installed(&hook) {
            guard::ensure_executable(&hook).map_err(|e| format!("{e:#}"))?;
        }
    }

    // The hook refuses to record into a workspace that never opted in, so the
    // directory has to exist before the next edit lands.
    std::fs::create_dir_all(intents::intents_dir(&root))
        .map_err(|e| format!("failed to prepare the intent directory: {e}"))?;

    Ok(providers::statuses(&root))
}

/// Read whatever the agents already recorded, with no setup at all.
///
/// Returns how many records were imported. Existing records are left alone:
/// deduplication happens on load, keyed by the agent's own call id.
#[tauri::command]
pub async fn import_intent_history(state: State<'_, AppState>) -> Result<usize, String> {
    let root = state.workspace_root()?;
    let (mut records, labels) = providers::history(&root);

    intents::rebase_seqs(&mut records, intents::next_seq(&root));
    for record in &records {
        intents::append_edit(&root, record).map_err(|e| format!("{e:#}"))?;
    }
    for label in &labels {
        intents::append_label(&root, label).map_err(|e| format!("{e:#}"))?;
    }

    Ok(intents::load(&root, &LoadOptions::default())
        .map(|i| i.records.len())
        .unwrap_or(0))
}

/// Forget everything recorded for this workspace.
#[tauri::command]
pub async fn clear_intent_history(state: State<'_, AppState>) -> Result<(), String> {
    let root = state.workspace_root()?;
    intents::clear(&root).map_err(|e| format!("{e:#}"))
}
