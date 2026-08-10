//! Claude Code: hooks in `settings.json`, history in per-project transcripts.
//!
//! History lives one directory per project under `~/.claude/projects/`, named
//! by mangling the workspace's absolute path — every `:`, `\`, `/`, `.` and
//! `_` becomes `-`. That rule matched all fifteen projects on the machine this
//! was written against, but it is not a documented interface, so it is used
//! only as a fast path: the `cwd` recorded inside the file is what actually
//! decides whether a session belongs to a workspace. If the naming scheme ever
//! changes, the scan falls back to reading candidates and still finds them.
//!
//! # Why transcript labels are weaker than hook labels
//!
//! An assistant message in these files contains *either* prose *or* tool
//! calls, never both — measured across a real session: 420 messages carrying
//! tool calls, 127 carrying text, and no overlap. So there is no message that
//! says "here is what I am about to do" attached to the edit that does it. The
//! best available label is the nearest preceding prose, which typically covers
//! around three tool calls and may describe only the first.
//!
//! That is why these labels are marked as coming from history rather than
//! from the agent's own end-of-turn summary, and why installing the hooks is
//! worth doing even though history works with no setup at all.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use super::{
    home_dir, hooks_json, instructions, InstallPlan, InstallScope, PlannedWrite, Provider,
    ProviderStatus,
};
use crate::intents::{normalise_path, IntentEdit, IntentLabel, IntentRecord, ProviderId};

#[derive(Debug, Clone, Default)]
pub struct ClaudeCode {
    /// An explicit `~/.claude` to read instead of the real one.
    ///
    /// Production always leaves this `None` and resolves the home directory
    /// itself; it exists so the transcript scan can be pointed at a fixture
    /// tree without an environment variable, which would leak between tests
    /// running in the same process.
    home: Option<PathBuf>,
}

impl ClaudeCode {
    pub fn new() -> Self {
        Self { home: None }
    }

    /// Read a specific directory as if it were `~/.claude`.
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
        }
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(claude_home)
    }
}

fn claude_home() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude"))
}

fn project_settings_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json")
}

fn user_settings_path(home: &Path) -> PathBuf {
    home.join("settings.json")
}

/// The directory name Claude Code derives from a workspace path.
///
/// Verified against every project on the development machine; treated as a
/// hint rather than a contract, because it is not documented anywhere.
pub fn encode_project_dir(root: &Path) -> String {
    root.to_string_lossy()
        .chars()
        .map(|c| match c {
            ':' | '\\' | '/' | '.' | '_' => '-',
            other => other,
        })
        .collect()
}

impl Provider for ClaudeCode {
    fn id(&self) -> ProviderId {
        ProviderId::ClaudeCode
    }

    fn detected(&self) -> bool {
        self.home().is_some_and(|h| h.is_dir())
    }

    fn status(&self, root: &Path) -> ProviderStatus {
        let Some(home) = self.home().filter(|h| h.is_dir()) else {
            return ProviderStatus::absent(ProviderId::ClaudeCode);
        };

        let user = user_settings_path(&home);
        let mut capture = if hooks_json::is_installed(&project_settings_path(root)) {
            Some(InstallScope::Project)
        } else if hooks_json::is_installed(&user) {
            Some(InstallScope::User)
        } else {
            None
        };

        let mut caveats = Vec::new();
        if capture == Some(InstallScope::User) {
            if let Some(pinned) = hooks_json::pinned_elsewhere(&user, root) {
                capture = None;
                caveats.push(hooks_json::pinned_caveat(&pinned));
            }
        }

        ProviderStatus {
            provider: ProviderId::ClaudeCode,
            detected: true,
            capture,
            sessions: find_sessions(&home, root).len(),
            caveats,
        }
    }

    fn install_plan(&self, root: &Path, scope: InstallScope) -> Result<InstallPlan> {
        let path = match scope {
            InstallScope::Project => project_settings_path(root),
            InstallScope::User => self.home().map(|h| user_settings_path(&h)).ok_or_else(|| {
                anyhow::anyhow!("could not locate the Claude Code home directory")
            })?,
        };

        // A user-scope hook fires everywhere, so it names no workspace and
        // lets the recorder resolve one from the payload.
        let pin = (scope == InstallScope::Project).then_some(root);
        let (content, merges_existing) = hooks_json::plan_merge(&path, pin)?;

        let mut caveats = Vec::new();
        if scope == InstallScope::Project {
            caveats.push(
                "This file is normally committed, so the hook would be shared with \
                 anyone who clones the repository."
                    .to_string(),
            );
        } else {
            caveats.push(
                "A user-level hook runs for every repository. It does nothing in \
                 workspaces that have not enabled intent capture."
                    .to_string(),
            );
        }
        if merges_existing {
            caveats.push(format!(
                "{} already exists; its current settings are kept and a backup is written.",
                path.display()
            ));
        }

        let mut writes = vec![PlannedWrite {
            path,
            content,
            merges_existing,
        }];

        // Asking for a real label. Without it the Stop hook falls back to the
        // first sentence of the closing message, which is written for a human
        // reading a chat rather than as a card title.
        if let Some(write) = instructions::planned_write(ProviderId::ClaudeCode, root) {
            caveats.push(format!(
                "A short section in {} asks the agent to state its intent; it is \
                 added or brought up to date. Capture works without it, but the \
                 labels are coarser.",
                write.path.display()
            ));
            writes.push(write);
        }

        Ok(InstallPlan {
            provider: ProviderId::ClaudeCode,
            scope,
            writes,
            caveats,
        })
    }

    fn history(&self, root: &Path) -> Result<(Vec<IntentRecord>, Vec<IntentLabel>)> {
        let mut records = Vec::new();
        let mut labels = Vec::new();
        let mut seq = 0u64;

        if let Some(home) = self.home() {
            for session in find_sessions(&home, root) {
                read_transcript(&session, root, &mut seq, &mut records, &mut labels);
            }
        }

        Ok((records, labels))
    }
}

/// Transcript files belonging to this workspace.
fn find_sessions(home: &Path, root: &Path) -> Vec<PathBuf> {
    let projects = home.join("projects");
    if !projects.is_dir() {
        return Vec::new();
    }

    // The encoded name is the fast path; anything it misses is still caught by
    // checking the `cwd` recorded inside each candidate.
    let expected = projects.join(encode_project_dir(root));
    let wanted = normalise_path(&root.to_string_lossy()).to_lowercase();

    let mut dirs: Vec<PathBuf> = Vec::new();
    if expected.is_dir() {
        dirs.push(expected.clone());
    } else {
        for entry in std::fs::read_dir(&projects).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
    }

    let mut found = Vec::new();
    for dir in dirs {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if transcript_cwd(&path).is_some_and(|cwd| cwd.to_lowercase() == wanted) {
                found.push(path);
            }
        }
    }

    found.sort();
    found
}

fn transcript_cwd(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok).take(40) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
            return Some(normalise_path(cwd));
        }
    }
    None
}

/// Read one transcript into records, attaching the nearest preceding prose as
/// a best-effort label.
fn read_transcript(
    path: &Path,
    root: &Path,
    seq: &mut u64,
    records: &mut Vec<IntentRecord>,
    labels: &mut Vec<IntentLabel>,
) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };

    // The most recent prose, which becomes the label for edits that follow.
    let mut recent_text: Option<String> = None;

    // Everything the agent did after one piece of prose is one intent.
    //
    // Keying on the individual tool call instead would make every edit its own
    // group, which defeats the point: measured against this repository's own
    // history that produced 127 groups from 145 hunks. Grouping by the prose
    // that preceded them is what turns a wall of hunks into a few decisions,
    // and it matches how the label was described in the first place — the
    // nearest preceding sentence, covering the handful of edits that followed.
    let session = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut block = 0usize;
    let mut labelled_block: Option<usize> = None;

    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        // Sidechain entries are a subagent's work, not the main session's.
        if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }

        let branch = value
            .get("gitBranch")
            .and_then(Value::as_str)
            .filter(|b| !b.is_empty())
            .map(str::to_string);

        let Some(blocks) = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        for content_block in blocks {
            match content_block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = content_block.get("text").and_then(Value::as_str) {
                        if let Some(summary) = summarise(text) {
                            // A new piece of prose starts a new intent.
                            recent_text = Some(summary);
                            block += 1;
                        }
                    }
                }
                Some("tool_use") => {
                    let turn = format!("claude-history-{session}-{block}");

                    let before = records.len();
                    read_tool_use(content_block, root, &branch, &turn, seq, records);

                    // One label per block of prose, however many edits followed
                    // it. Emitting one per edit would defeat the grouping.
                    if records.len() > before && labelled_block != Some(block) {
                        if let Some(label) = &recent_text {
                            labelled_block = Some(block);
                            labels.push(IntentLabel {
                                provider: ProviderId::ClaudeCode,
                                turn_id: turn,
                                label: label.clone(),
                                paths: Vec::new(),
                                anchor: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Reduce a prose block to something that fits on a card.
///
/// Only the first sentence is kept, and only if it is short enough to read at
/// a glance — a paragraph of explanation is worse than no label, because it
/// makes the card unreadable while looking authoritative.
fn summarise(text: &str) -> Option<String> {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("```"))?;

    let sentence = first
        .split_terminator(['.', '!', '?'])
        .next()
        .unwrap_or(first);
    let cleaned = sentence.trim().trim_end_matches(':').trim();

    (cleaned.len() >= 8 && cleaned.len() <= 120).then(|| cleaned.to_string())
}

/// Turn one `Edit` or `Write` tool call into records.
fn read_tool_use(
    block: &Value,
    root: &Path,
    branch: &Option<String>,
    turn: &str,
    seq: &mut u64,
    out: &mut Vec<IntentRecord>,
) {
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(input) = block.get("input") else {
        return;
    };
    let Some(path) = input.get("file_path").and_then(Value::as_str) else {
        return;
    };
    let Some(relative) = crate::intents::relative_to(root, path) else {
        return;
    };
    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();

    let edits: Vec<IntentEdit> = match name {
        "Write" => input
            .get("content")
            .or_else(|| input.get("file_text"))
            .and_then(Value::as_str)
            .map(|text| {
                vec![IntentEdit {
                    old_lines: Vec::new(),
                    new_lines: lines_of(text),
                    whole_file: true,
                }]
            })
            .unwrap_or_default(),

        "Edit" | "MultiEdit" => single_or_many(input),

        _ => Vec::new(),
    };

    for (n, edit) in edits.into_iter().enumerate() {
        if edit.is_empty() {
            continue;
        }
        *seq += 1;
        out.push(IntentRecord {
            provider: ProviderId::ClaudeCode,
            turn_id: turn.to_string(),
            tool_use_id: format!("{id}:{n}"),
            seq: *seq,
            path: relative.clone(),
            edit,
            branch: branch.clone(),
        });
    }
}

/// An edit arrives either as one before/after pair or as an array of them,
/// depending on the version — both spellings are accepted.
fn single_or_many(input: &Value) -> Vec<IntentEdit> {
    if let Some(items) = input.get("edits").and_then(Value::as_array) {
        return items.iter().filter_map(pair_to_edit).collect();
    }
    pair_to_edit(input).into_iter().collect()
}

fn pair_to_edit(value: &Value) -> Option<IntentEdit> {
    let old = value
        .get("old_string")
        .or_else(|| value.get("old_text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new = value
        .get("new_string")
        .or_else(|| value.get("new_text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if old.is_empty() && new.is_empty() {
        return None;
    }

    Some(IntentEdit {
        old_lines: lines_of(old),
        new_lines: lines_of(new),
        whole_file: false,
    })
}

#[cfg(test)]
#[path = "claude_code_tests.rs"]
mod claude_code_tests;

fn lines_of(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect()
}
