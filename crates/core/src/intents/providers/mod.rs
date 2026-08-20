//! Per-agent knowledge: where it keeps its history, and how to ask it to
//! record intent.
//!
//! Claude Code and Codex converged on nearly the same hook design — the same
//! `{type, command, timeout}` handler shape, the same event names, the same
//! JSON-on-stdin contract. Codex even accepts `Edit` and `Write` as aliases
//! for its own `apply_patch` tool so that a Claude Code configuration works
//! unchanged. So this abstraction stays deliberately thin: it answers where
//! files live and how payloads are shaped, and nothing more.
//!
//! Each provider offers two independent things, and a workspace may have
//! either, both, or neither:
//!
//! * **capture** — hooks the agent runs as it works, giving labelled intent
//!   from the moment they are installed.
//! * **history** — session files the agent already wrote, giving unlabelled
//!   (or coarsely labelled) intent retroactively, with no setup at all.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{IntentLabel, IntentPrompt, IntentRecord, ProviderId};

pub mod claude_code;
pub mod codex;
pub mod hooks_json;
pub mod instructions;
pub mod settings_merge;

/// One matcher covering both agents' edit tools.
///
/// Codex registers `apply_patch` with `Write` and `Edit` as aliases precisely
/// so a Claude Code configuration keeps working, which means a single string
/// is correct for both rather than a per-provider special case.
pub const EDIT_TOOL_MATCHER: &str = "apply_patch|Edit|Write";

/// Where a provider's hook configuration should be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum InstallScope {
    /// Inside the repository, shared with anyone who clones it.
    Project,
    /// In the user's home directory, covering every repository they open.
    User,
}

/// What a provider can currently do for a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: ProviderId,
    /// The agent appears to be installed on this machine.
    pub detected: bool,
    /// Our hooks are already configured, and where.
    pub capture: Option<InstallScope>,
    /// How many past sessions were found for this workspace.
    pub sessions: usize,
    /// Anything the user has to do before capture will actually produce
    /// records — an untrusted project, an unreviewed hook, a skipped
    /// compressed session. Shown rather than swallowed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
}

impl ProviderStatus {
    pub fn absent(provider: ProviderId) -> Self {
        Self {
            provider,
            detected: false,
            capture: None,
            sessions: 0,
            caveats: Vec::new(),
        }
    }
}

/// One file an install would create or change.
///
/// Rendered for confirmation before anything is written: these paths reach
/// outside the workspace and into files the user shares with their team, so
/// the change is always shown first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedWrite {
    pub path: PathBuf,
    /// The file's full contents after the change.
    pub content: String,
    /// True when the file already exists and is being merged into rather than
    /// created, which is the case the user most needs to see.
    pub merges_existing: bool,
}

/// Everything an install would do, computed without touching disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub provider: ProviderId,
    pub scope: InstallScope,
    pub writes: Vec<PlannedWrite>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
}

/// A provider's session file, already matched to this workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// What each agent has to know how to do.
pub trait Provider {
    fn id(&self) -> ProviderId;

    /// Whether the agent is installed on this machine at all.
    fn detected(&self) -> bool;

    /// Report what is configured for a workspace, and what is in the way.
    fn status(&self, root: &Path) -> ProviderStatus;

    /// Compute the install without performing it.
    fn install_plan(&self, root: &Path, scope: InstallScope) -> Result<InstallPlan>;

    /// Read past sessions for this workspace into records.
    ///
    /// Labels and the user's prompts come back too where the agent's own
    /// history makes them recoverable. Labels are necessarily coarser than
    /// hook-captured ones; prompts are keyed to the same synthesised turn id as
    /// the records from that turn, so they join.
    fn history(&self, root: &Path) -> Result<HistoryMined>;
}

/// What a session sweep recovered: records, coarse labels, and user prompts —
/// each keyed to the same synthesised turn id so they join.
#[derive(Debug, Default)]
pub struct HistoryMined {
    pub records: Vec<IntentRecord>,
    pub labels: Vec<IntentLabel>,
    pub prompts: Vec<IntentPrompt>,
}

/// Perform a plan, writing every file it names.
///
/// Existing files are backed up beside themselves first. Nothing here is
/// clever: the plan already computed the exact final contents, precisely so
/// that what the user approved is what gets written.
pub fn apply_plan(plan: &InstallPlan) -> Result<()> {
    apply_writes(&plan.writes)
}

/// Perform a set of planned writes, backing up any file being merged into first.
///
/// Split out from [`apply_plan`] so callers that assemble writes without a full
/// [`InstallPlan`] — the instruction-template library — reuse the same
/// backup-then-write behaviour rather than reimplementing it.
pub fn apply_writes(writes: &[PlannedWrite]) -> Result<()> {
    use anyhow::Context;

    for write in writes {
        if let Some(parent) = write.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        if write.merges_existing && write.path.exists() {
            let backup = write.path.with_extension(backup_extension(&write.path));
            std::fs::copy(&write.path, &backup).with_context(|| {
                format!(
                    "failed to back up {} before changing it",
                    write.path.display()
                )
            })?;
        }

        std::fs::write(&write.path, &write.content)
            .with_context(|| format!("failed to write {}", write.path.display()))?;
    }

    Ok(())
}

/// `hooks.json` becomes `hooks.json.bak`, keeping the original extension so
/// the backup is obviously a copy of that file and not a different one.
fn backup_extension(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(existing) => format!("{existing}.bak"),
        None => "bak".to_string(),
    }
}

/// Every provider, whether or not it is installed.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(claude_code::ClaudeCode::new()),
        Box::new(codex::Codex::new()),
    ]
}

/// Status for every provider, for the capture-settings UI.
pub fn statuses(root: &Path) -> Vec<ProviderStatus> {
    all().iter().map(|p| p.status(root)).collect()
}

/// Records from every provider's existing history, merged.
///
/// One provider failing — an unreadable home directory, a format that moved —
/// must not cost the other's history, so failures are dropped rather than
/// propagated.
pub fn history(root: &Path) -> HistoryMined {
    let mut merged = HistoryMined::default();

    for provider in all() {
        if let Ok(mined) = provider.history(root) {
            merged.records.extend(mined.records);
            merged.labels.extend(mined.labels);
            merged.prompts.extend(mined.prompts);
        }
    }

    merged.records.sort_by_key(|r| r.seq);
    merged
}

/// The commit guard, as an extra write on an install plan.
///
/// Repository-level rather than per-agent: the note a rejection leaves behind
/// is the same note whichever agent wrote the code. Both providers append this,
/// and installing both is harmless — the second plan finds the block already
/// current and contributes nothing.
pub(crate) fn guard_write(root: &Path, caveats: &mut Vec<String>) -> Option<PlannedWrite> {
    let write = super::guard::planned_write(root)?;

    caveats.push(format!(
        "A guard is added to {} so a commit that still carries a rejection note \
         is refused. Commit with CB_ALLOW_REJECTED=1 to override it.",
        write.path.display()
    ));

    Some(write)
}

/// The durable-why post-commit hook, as an extra write on an install plan.
///
/// Repository-level like the guard, and for the same reason: a commit made from
/// the command line — including by an agent — goes through the system `git`,
/// which the in-app commit's note-writing never reaches. Installing from both
/// providers is harmless — the second plan finds the block already current.
pub(crate) fn whyhook_write(root: &Path, caveats: &mut Vec<String>) -> Option<PlannedWrite> {
    let write = super::whyhook::planned_write(root)?;

    caveats.push(format!(
        "A post-commit hook is added to {} so commits made from the command line \
         also persist the intent behind each line into a durable git note.",
        write.path.display()
    ));

    Some(write)
}

/// The user's home directory, for locating an agent's configuration.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
