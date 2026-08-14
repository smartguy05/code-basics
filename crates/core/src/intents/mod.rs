//! What a coding agent said it was doing, and where it wrote it down.
//!
//! A diff shows *what* changed. It never shows *why*, and by the time anyone
//! reviews it the reasoning is gone. But the agent that produced the change
//! knew exactly why it touched each region, seconds earlier — so rather than
//! reconstructing intent by static analysis, this module collects it from the
//! agent directly, while it is still cheap to know.
//!
//! # Two files, one shape
//!
//! Agents record through *hooks*: a command the agent runs at fixed points in
//! its own lifecycle. Two events matter, and neither is sufficient alone:
//!
//! * **after every edit** — reliable geometry (which file, which lines), but
//!   the payload carries no reason. Neither Claude Code nor Codex offers any
//!   way to attach one.
//! * **at the end of a turn** — where the reason lives, in the agent's closing
//!   message, but with no indication of which edit it refers to.
//!
//! So the two are written separately and joined afterwards on the turn
//! identifier both carry:
//!
//! ```text
//! .code-basics/intents/edits.jsonl    {turnId, toolUseId, path, oldLines, newLines}
//! .code-basics/intents/labels.jsonl   {turnId, label, paths}
//! ```
//!
//! Because the end-of-turn hook fires once per turn and reports the same id
//! the edits within it carried, that join is exact rather than a guess.
//!
//! # Why the provider difference stops here
//!
//! Claude Code and Codex describe an edit completely differently — one gives
//! before/after strings, the other a patch envelope — and they disagree on
//! what the turn identifier is called. Every one of those differences is
//! resolved at ingest, so [`IntentRecord`] is provider-blind and nothing
//! downstream needs to know which agent wrote the code.
//!
//! Recording is local bookkeeping about work in progress, so
//! `.code-basics/intents/` is gitignored alongside `changelists.json`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

pub mod guard;
pub mod hook;
pub mod patchfmt;
pub mod providers;
pub mod reject;

/// Directory under `.code-basics/` holding recorded intent.
pub const INTENTS_DIR: &str = "intents";
pub const EDITS_FILE: &str = "edits.jsonl";
pub const LABELS_FILE: &str = "labels.jsonl";

/// Which agent recorded an edit.
///
/// Kept on every record so a workspace used with both agents can still be
/// explained, and so a future format change in one of them can be migrated
/// without touching the other's history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProviderId {
    ClaudeCode,
    Codex,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "claudeCode",
            ProviderId::Codex => "codex",
        }
    }
}

/// What the agent did to one file.
///
/// Both agents' edit formats reduce to this. The line vectors hold content
/// only — no line numbers, deliberately: see [`crate::git::attribution`] for
/// why position is discarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IntentEdit {
    /// Lines the edit removed. Empty for a file creation.
    #[serde(default)]
    pub old_lines: Vec<String>,
    /// Lines the edit introduced. Empty for a file deletion.
    #[serde(default)]
    pub new_lines: Vec<String>,
    /// True when this replaced the whole file rather than a region, which
    /// makes it far weaker evidence — a whole-file write "matches" everything.
    #[serde(default)]
    pub whole_file: bool,
}

impl IntentEdit {
    /// An edit that changes nothing carries no evidence and is dropped at
    /// ingest rather than stored.
    pub fn is_empty(&self) -> bool {
        self.old_lines.is_empty() && self.new_lines.is_empty()
    }
}

/// One recorded edit, after provider differences have been normalised away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IntentRecord {
    pub provider: ProviderId,
    /// The agent's turn identifier — `prompt_id` in Claude Code, `turn_id` in
    /// Codex. Labels join to edits on this.
    pub turn_id: String,
    /// The agent's id for this individual tool call, used to deduplicate when
    /// the same edit is seen from more than one source.
    pub tool_use_id: String,
    /// Monotonic within a workspace: the order edits were recorded. Later
    /// edits win when two records claim the same line.
    pub seq: u64,
    /// Workspace-relative path with forward slashes.
    pub path: String,
    pub edit: IntentEdit,
    /// The branch checked out when the edit was made, when the hook could
    /// determine it. Records from another branch are discarded on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// Where a label's words came from.
///
/// The distinction the Changes tab rests on: only a label the agent *offered*
/// as a label may title a card as a stated intent. Anything mined out of prose
/// is a guess about which sentence belongs to which edit, however plausible it
/// reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LabelSource {
    /// An explicit `Intent:` line in the agent's closing message.
    Declared,
    /// The first sentence of prose, mined live or out of session history.
    ///
    /// The default, so records written before this field existed — which came
    /// overwhelmingly from the first-sentence fallback — read as what they are
    /// rather than being promoted to declared.
    #[default]
    Inferred,
}

/// A reason, recorded once per turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IntentLabel {
    pub provider: ProviderId,
    pub turn_id: String,
    /// Short human phrase — "add retry to token refresh".
    pub label: String,
    /// Files this label covers. Empty means every file edited in the turn.
    #[serde(default)]
    pub paths: Vec<String>,
    /// A distinctive line from the edit, disambiguating when one turn made
    /// several unrelated changes to the same file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Whether the agent offered this as a label or it was mined from prose.
    #[serde(default)]
    pub source: LabelSource,
}

/// Everything recorded for a workspace, already filtered and joined.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Intents {
    pub records: Vec<IntentRecord>,
    pub labels: Vec<IntentLabel>,
}

impl Intents {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The label covering a record, preferring one that names its file.
    ///
    /// A turn-wide label (empty `paths`) is a legitimate fallback: an agent
    /// that described its turn once still explains every edit in it.
    pub fn label_for(&self, record: &IntentRecord) -> Option<&IntentLabel> {
        let mut fallback = None;

        for label in &self.labels {
            if label.turn_id != record.turn_id {
                continue;
            }
            if label.paths.iter().any(|p| normalise_path(p) == record.path) {
                return Some(label);
            }
            if label.paths.is_empty() && fallback.is_none() {
                fallback = Some(label);
            }
        }

        fallback
    }

    /// Records touching one file, oldest first.
    pub fn for_path(&self, path: &str) -> Vec<&IntentRecord> {
        let path = normalise_path(path);
        let mut hits: Vec<&IntentRecord> = self.records.iter().filter(|r| r.path == path).collect();
        hits.sort_by_key(|r| r.seq);
        hits
    }
}

/// Separators are normalised so a path compares equal to the one `git status`
/// reports, whichever side wrote it. Mirrors `changelists::normalise`.
pub fn normalise_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Make a path recorded by a hook relative to the workspace.
///
/// Hooks report absolute paths; the diff model uses workspace-relative ones.
/// A path outside the workspace returns `None` and the record is dropped —
/// an agent editing someone else's repository is not this workspace's history.
pub fn relative_to(root: &Path, path: &str) -> Option<String> {
    let normalised = normalise_path(path);
    let root_str = normalise_path(&root.to_string_lossy());

    if let Some(rest) = strip_root(&normalised, &root_str) {
        return Some(rest);
    }

    // Already relative: accept it as-is, but never one that climbs out.
    if !normalised.contains(':') && !normalised.starts_with('/') && !normalised.starts_with("..") {
        return Some(normalised.trim_start_matches("./").to_string());
    }

    None
}

/// Case-insensitively on Windows, where the same path legitimately differs in
/// case between what the agent reports and what the workspace was opened as.
fn strip_root(path: &str, root: &str) -> Option<String> {
    let root = root.trim_end_matches('/');

    let matches = if cfg!(windows) {
        path.len() > root.len()
            && path[..root.len()].eq_ignore_ascii_case(root)
            && path.as_bytes()[root.len()] == b'/'
    } else {
        path.len() > root.len() && path.starts_with(root) && path.as_bytes()[root.len()] == b'/'
    };

    matches.then(|| path[root.len() + 1..].to_string())
}

pub fn intents_dir(root: &Path) -> PathBuf {
    crate::config::config_dir(root).join(INTENTS_DIR)
}

pub fn edits_path(root: &Path) -> PathBuf {
    intents_dir(root).join(EDITS_FILE)
}

pub fn labels_path(root: &Path) -> PathBuf {
    intents_dir(root).join(LABELS_FILE)
}

/// How records are filtered as they load.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Drop records made on a different branch. Their text may match the
    /// current diff by coincidence, and a confident wrong label is the one
    /// outcome this feature must avoid.
    pub branch: Option<String>,
}

/// Read everything recorded for a workspace.
///
/// A missing file means recording was never enabled, which is normal and not
/// an error. A *malformed line* is skipped rather than failing the load: the
/// files are appended to by an external hook, and a half-written final line
/// after a crash must not cost the user the rest of their history.
pub fn load(root: &Path, options: &LoadOptions) -> Result<Intents> {
    let records = load_edits(root, options)?;
    let labels = load_labels(root)?;

    Ok(Intents { records, labels })
}

/// Read the labels, dropping inferred ones that do not read like a reason.
///
/// The gate has to run **here** as well as at the point of recording. Recording
/// only protects future turns, and a workspace that has been capturing for a
/// while already holds hundreds of first sentences written long before any of
/// this existed — those are precisely the ones titling cards today. Judging
/// them on the way out costs one pass over a small file and needs no migration.
///
/// A *declared* label is never second-guessed, in either direction: the agent
/// offered those words as the title.
fn load_labels(root: &Path) -> Result<Vec<IntentLabel>> {
    let mut labels = read_jsonl::<IntentLabel>(&labels_path(root))?;

    labels.retain(|label| match label.source {
        LabelSource::Declared => true,
        LabelSource::Inferred => hook::is_usable_inferred_label(&label.label),
    });

    Ok(labels)
}

fn load_edits(root: &Path, options: &LoadOptions) -> Result<Vec<IntentRecord>> {
    let raw = read_jsonl::<IntentRecord>(&edits_path(root))?;

    let mut seen: HashSet<String> = HashSet::new();
    let mut records = Vec::with_capacity(raw.len());

    for mut record in raw {
        if record.edit.is_empty() {
            continue;
        }

        // A record from another branch describes code that is not in this
        // working tree.
        if let (Some(wanted), Some(actual)) = (&options.branch, &record.branch) {
            if wanted != actual {
                continue;
            }
        }

        record.path = normalise_path(&record.path);
        if let Some(relative) = relative_to(root, &record.path) {
            record.path = relative;
        } else {
            continue;
        }

        // The same edit can arrive from both a hook and a transcript sweep.
        if !record.tool_use_id.is_empty() && !seen.insert(record.tool_use_id.clone()) {
            continue;
        }

        records.push(record);
    }

    records.sort_by_key(|r| r.seq);
    Ok(records)
}

/// Read a JSON-lines file, skipping blank and unparseable lines.
fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<T>(line).ok())
        .collect())
}

/// Append one record, creating the directory and gitignore entry if needed.
///
/// Used by the in-process recorder and by tests; the installed hooks append to
/// the same files themselves.
pub fn append_edit(root: &Path, record: &IntentRecord) -> Result<()> {
    append_line(&edits_path(root), record, root)
}

pub fn append_label(root: &Path, label: &IntentLabel) -> Result<()> {
    append_line(&labels_path(root), label, root)
}

fn append_line<T: Serialize>(path: &Path, value: &T, root: &Path) -> Result<()> {
    use std::io::Write;

    let dir = intents_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    crate::config::ensure_gitignore(&crate::config::config_dir(root))?;

    let json = serde_json::to_string(value).context("failed to serialise an intent record")?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    writeln!(file, "{json}").with_context(|| format!("failed to write {}", path.display()))
}

/// The next unused sequence number for a workspace.
pub fn next_seq(root: &Path) -> u64 {
    read_jsonl::<IntentRecord>(&edits_path(root))
        .map(|records| records.iter().map(|r| r.seq).max().map_or(0, |m| m + 1))
        .unwrap_or(0)
}

/// Lift a batch of imported records above the numbering a workspace already
/// uses, returning the highest sequence number handed out.
///
/// History mined from an agent's own session files is numbered in that
/// agent's terms, starting near zero, and two providers happily reuse the same
/// numbers. Appending them unchanged would interleave imported edits with
/// recorded ones and lose the "later edit wins" ordering attribution depends
/// on, so every imported record is pushed above the workspace's current
/// maximum before it is written.
///
/// Only the ordering is meaningful — the absolute values are not, and the base
/// is carried forward from each record as it is rebased, so a batch's seqs
/// accumulate rather than being shifted by one constant. The caller passes
/// [`next_seq`] and does the writing; this decides nothing about files.
pub fn rebase_seqs(records: &mut [IntentRecord], base: u64) -> u64 {
    let mut next = base;

    for record in records.iter_mut() {
        record.seq = record.seq.saturating_add(next);
        next = next.max(record.seq);
    }

    next
}

/// Forget everything recorded for a workspace.
pub fn clear(root: &Path) -> Result<()> {
    for path in [edits_path(root), labels_path(root)] {
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "intents_tests.rs"]
mod tests;
