//! Intent that survives the commit.
//!
//! Recorded intent lives in gitignored JSONL and dies at commit. This module
//! persists it: at commit time it maps each attributed line to the turn and
//! reason behind it, keyed by **content** (via [`crate::git::attribution::anchor_key`]),
//! and stores that mapping in a git note under [`WHY_NOTES_REF`]. The History
//! tab then answers "why is this line like this?" months later.
//!
//! # Content-keyed, like attribution
//!
//! The key is a line's normalised skeleton, gated by the same anchor rule the
//! matcher uses, so a bare `}` never keys anything and a line found in a
//! historical blob resolves even after it was reformatted or rebased. Position
//! is never stored.
//!
//! # A wrong "why" is worse than no "why"
//!
//! A line resolves only when its content key maps to exactly one turn. A key
//! that two different turns produced is dropped at build time; a blob line that
//! matches no key resolves to nothing. The History tab shows the empty state
//! rather than a guessed reason.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::attribution::{anchor_key, Confidence, FileAttribution};
use crate::git::patch::{FileDiff, LineOrigin};
use crate::intents::LabelSource;

/// The notes ref durable intent is written to.
pub const WHY_NOTES_REF: &str = "refs/notes/code-basics-intents";

/// One content-keyed intent entry for a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WhyEntry {
    /// A stable hash of the line's anchor key. Never the source text or a line
    /// number.
    pub content_hash: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_source: Option<LabelSource>,
    /// The user's prompt for this turn, when one was recorded. Persisted onto
    /// the note at commit so old notes (written before prompts existed)
    /// deserialise unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub seq: u64,
    pub confidence: Confidence,
}

/// The durable intent for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileWhy {
    pub path: String,
    pub entries: Vec<WhyEntry>,
}

/// Everything persisted for one commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DurableWhy {
    pub files: Vec<FileWhy>,
}

impl DurableWhy {
    pub fn is_empty(&self) -> bool {
        self.files.iter().all(|f| f.entries.is_empty())
    }
}

/// The reason behind one line of a historical file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LineIntent {
    /// 1-based line number in the committed file.
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_source: Option<LabelSource>,
    pub turn_id: String,
    pub confidence: Confidence,
    /// The user prompt that caused it, when captured. Currently always absent —
    /// prompt capture is a separate turn-keyed source not yet wired — so the
    /// UI shows the reason and turn without fabricating a prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// A stable content hash (FNV-1a, 64-bit) — deterministic across platforms and
/// releases, unlike `DefaultHasher`, because it is persisted.
pub fn content_hash(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Build the durable mapping for a just-made commit.
///
/// `diffs` are the commit's own changes and `attributions` are their
/// attribution — same scan, same order. Only *added* anchor lines key (the
/// committed blob contains added content), and a key two different turns
/// produced is dropped rather than attributed to either.
pub fn build_for_commit(
    diffs: &[FileDiff],
    attributions: &[FileAttribution],
    prompts: &[crate::intents::IntentPrompt],
) -> DurableWhy {
    let mut files = Vec::new();

    for (index, diff) in diffs.iter().enumerate() {
        let Some(attribution) = attributions.get(index) else {
            continue;
        };

        // content_hash -> (distinct turns seen, best entry by seq).
        let mut candidates: BTreeMap<String, Candidate> = BTreeMap::new();

        for hunk_attr in &attribution.hunks {
            let Some(hunk) = diff.hunks.get(hunk_attr.hunk) else {
                continue;
            };
            for span in &hunk_attr.spans {
                for &line_index in &span.line_indices {
                    let Some(line) = hunk.lines.iter().find(|l| l.index == line_index) else {
                        continue;
                    };
                    // Only added lines survive into the commit's blob.
                    if line.origin != LineOrigin::Addition {
                        continue;
                    }
                    let Some(key) = anchor_key(&line.content) else {
                        continue;
                    };
                    let hash = content_hash(&key);
                    let entry = WhyEntry {
                        content_hash: hash.clone(),
                        turn_id: span.turn_id.clone(),
                        label: span.label.clone(),
                        label_source: span.label_source,
                        prompt: crate::intents::prompt_for(prompts, &span.turn_id)
                            .map(str::to_string),
                        seq: span.seq,
                        confidence: span.confidence,
                    };
                    candidates.entry(hash).or_default().observe(entry);
                }
            }
        }

        let entries: Vec<WhyEntry> = candidates
            .into_values()
            .filter_map(Candidate::resolve)
            .collect();

        if !entries.is_empty() {
            files.push(FileWhy {
                path: diff.path.clone(),
                entries,
            });
        }
    }

    DurableWhy { files }
}

#[derive(Default)]
struct Candidate {
    turns: std::collections::BTreeSet<String>,
    best: Option<WhyEntry>,
}

impl Candidate {
    fn observe(&mut self, entry: WhyEntry) {
        self.turns.insert(entry.turn_id.clone());
        match &self.best {
            Some(current) if current.seq >= entry.seq => {}
            _ => self.best = Some(entry),
        }
    }

    /// Keep the entry only when a single turn produced this key; a key two
    /// turns share is ambiguous and abstained on.
    fn resolve(self) -> Option<WhyEntry> {
        (self.turns.len() == 1).then_some(self.best).flatten()
    }
}

/// Resolve every line of a committed file to its intent, from the stored note.
///
/// A line resolves only when its content key matches a stored entry; every
/// other line yields nothing.
pub fn resolve_lines(file: &FileWhy, blob: &str) -> Vec<LineIntent> {
    let by_hash: HashMap<&str, &WhyEntry> = file
        .entries
        .iter()
        .map(|e| (e.content_hash.as_str(), e))
        .collect();

    let mut out = Vec::new();
    for (i, line) in blob.lines().enumerate() {
        let Some(key) = anchor_key(line) else {
            continue;
        };
        if let Some(entry) = by_hash.get(content_hash(&key).as_str()) {
            out.push(LineIntent {
                line: (i + 1) as u32,
                label: entry.label.clone(),
                label_source: entry.label_source,
                turn_id: entry.turn_id.clone(),
                confidence: entry.confidence,
                prompt: entry.prompt.clone(),
            });
        }
    }
    out
}

/// The durable intent for one file within a whole-commit mapping.
pub fn file_in<'a>(why: &'a DurableWhy, path: &str) -> Option<&'a FileWhy> {
    why.files.iter().find(|f| f.path == path)
}

/// The full pipeline: build the durable mapping for a commit from the recorded
/// intent and write it to the commit's note.
///
/// The single point both commit paths funnel through — the in-app `git_commit`
/// command and the `post-commit` hook for external commits — so the two cannot
/// drift. An empty mapping writes nothing (see [`Repo::write_why_note`]).
pub fn record_note(
    repo: &crate::git::repo::Repo,
    root: &std::path::Path,
    commit: &str,
) -> anyhow::Result<()> {
    use crate::git::attribution::{attribute, Options};
    use crate::intents::{self, LoadOptions};

    let diffs = repo.commit_diff(commit)?;
    // Records made on another branch describe code that is not in this commit.
    let branch = repo.status().ok().and_then(|s| s.branch);
    let intents = intents::load(root, &LoadOptions { branch })?;
    // Prompts are kept out of `Intents`, so load them alongside.
    let prompts = intents::load_prompts(root)?;
    let attributions = attribute(&diffs, &intents, Options::default());
    let mapping = build_for_commit(&diffs, &attributions, &prompts);
    repo.write_why_note(commit, &mapping)
}

#[cfg(test)]
#[path = "why_tests.rs"]
mod tests;
