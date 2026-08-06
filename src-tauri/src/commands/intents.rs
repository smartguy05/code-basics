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

use cb_core::git::attribution::{self, Options};
use cb_core::git::grouping::{self, IntentGroup};
use cb_core::git::{ComparisonMode, Repo};
use cb_core::intents::providers::{self, InstallPlan, InstallScope, ProviderStatus};
use cb_core::intents::{self, LoadOptions, ProviderId};
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
fn lines_for(
    repo: &Repo,
    root: &Path,
    group_id: &str,
    mode: ComparisonMode,
) -> Result<Vec<(String, BTreeSet<u32>)>, String> {
    let diffs = repo.diff_all(mode).map_err(|e| format!("{e:#}"))?;
    let branch = repo.status().ok().and_then(|s| s.branch);
    let intents = intents::load(root, &LoadOptions { branch }).map_err(|e| format!("{e:#}"))?;

    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    let groups = grouping::group(&diffs, &attributions);

    let group = groups
        .into_iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| "that group is no longer in the working tree".to_string())?;

    Ok(group
        .files
        .into_iter()
        .map(|f| (f.path, f.line_indices.into_iter().collect()))
        .collect())
}

/// Stage everything in one group.
///
/// Returns how many files were changed, so the UI can say nothing happened
/// rather than silently appearing to succeed.
#[tauri::command]
pub async fn stage_intent_group(
    state: State<'_, AppState>,
    group: String,
) -> Result<usize, String> {
    let (root, repo) = open(&state)?;
    // Staging compares the working tree against the index, whatever the user
    // happens to be looking at.
    let files = lines_for(&repo, &root, &group, ComparisonMode::WorkingToIndex)?;

    let mut staged = 0;
    for (path, lines) in files {
        if repo
            .stage_lines(&path, &lines)
            .map_err(|e| format!("{e:#}"))?
        {
            staged += 1;
        }
    }
    Ok(staged)
}

/// Revert everything in one group, in the mode the user is looking at.
#[tauri::command]
pub async fn revert_intent_group(
    state: State<'_, AppState>,
    group: String,
    mode: ComparisonMode,
) -> Result<usize, String> {
    let (root, repo) = open(&state)?;
    let files = lines_for(&repo, &root, &group, mode)?;

    let mut reverted = 0;
    for (path, lines) in files {
        if repo
            .revert_lines(&path, mode, &lines)
            .map_err(|e| format!("{e:#}"))?
        {
            reverted += 1;
        }
    }
    Ok(reverted)
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
    let (records, labels) = providers::history(&root);

    let mut base = intents::next_seq(&root);
    for mut record in records {
        record.seq += base;
        intents::append_edit(&root, &record).map_err(|e| format!("{e:#}"))?;
        base = base.max(record.seq);
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
