//! Per-workspace record of which run-once prompts have already been run.
//!
//! A prompt marked `once: true` (see [`super::PromptInfo::once`]) should run a
//! single time per repository — a knowledge-graph build, an initial setup. This
//! module is the memory of that: a small JSON map under `.code-basics/` in the
//! opened workspace, keyed by prompt id, recording when each was last run to a
//! successful finish. The menu reads it to badge an already-run prompt and to
//! confirm before running it again.
//!
//! Pure filesystem/serde work with no Tauri dependency, tested headlessly. The
//! clock is passed in (never read here) so a record is reproducible in tests,
//! matching [`crate::intents::providers::reject::iso_date`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One prompt's last successful run in this workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptRun {
    /// When the run finished, in milliseconds since the Unix epoch. A number so
    /// the frontend can format an absolute or relative time as it likes.
    pub last_run_at_ms: u64,
}

/// The run record keyed by prompt id.
pub type PromptRuns = BTreeMap<String, PromptRun>;

/// The record's file name inside `.code-basics/`. Listed in
/// [`crate::config::ensure_gitignore`] so it is never committed.
pub const RUNS_FILE: &str = "agent-runs.json";

/// Where the record lives for a workspace: `<root>/.code-basics/agent-runs.json`.
pub fn runs_path(root: &Path) -> PathBuf {
    root.join(crate::config::CONFIG_DIR).join(RUNS_FILE)
}

/// Read the record for `root`. A missing or unparseable file yields an empty map
/// rather than an error — the record is a convenience, and a corrupt one must
/// not block running a prompt.
pub fn read_runs(root: &Path) -> PromptRuns {
    let Ok(text) = std::fs::read_to_string(runs_path(root)) else {
        return PromptRuns::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Record a successful run of `prompt_id` at `at`, creating the file (and its
/// `.code-basics` directory) if absent. A re-run overwrites the previous stamp.
pub fn record_run(root: &Path, prompt_id: &str, at: SystemTime) -> Result<()> {
    let mut runs = read_runs(root);
    runs.insert(
        prompt_id.to_string(),
        PromptRun {
            last_run_at_ms: epoch_millis(at),
        },
    );

    let path = runs_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&runs)?;
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn epoch_millis(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
