//! Codex: hooks in `hooks.json`, history in dated rollout files.
//!
//! Codex's hook system is a close relative of Claude Code's — same handler
//! shape, same events, same JSON on stdin — so most of what differs is where
//! the files live and how an edit is described.
//!
//! Two Codex-specific conditions have to be reported rather than assumed
//! away, because in both cases the configuration looks correct while quietly
//! doing nothing:
//!
//! * A `.codex/` directory inside a repository is ignored unless that
//!   repository is **trusted**, recorded as
//!   `[projects."<path>"] trust_level = "trusted"` in `~/.codex/config.toml`.
//! * Codex asks the user to review a command hook before it will run one.
//!
//! History lives in dated rollout files rather than one file per project:
//! `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`. Each
//! begins with a `session_meta` line naming the `cwd`, so matching a session
//! to a workspace costs one line per file. Cold sessions are Zstd-compressed
//! to `.jsonl.zst`; those are counted and skipped rather than decompressed.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};

use super::{
    home_dir, hooks_json, instructions, InstallPlan, InstallScope, PlannedWrite, Provider,
    ProviderStatus,
};
use crate::intents::patchfmt;
use crate::intents::{normalise_path, IntentEdit, IntentLabel, IntentRecord, ProviderId};

pub struct Codex;

impl Default for Codex {
    fn default() -> Self {
        Self::new()
    }
}

impl Codex {
    pub fn new() -> Self {
        Self
    }
}

/// Codex's home, honouring the `CODEX_HOME` override before falling back.
pub fn codex_home() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("CODEX_HOME") {
        if !explicit.is_empty() {
            return Some(PathBuf::from(explicit));
        }
    }
    home_dir().map(|h| h.join(".codex"))
}

fn user_hooks_path(home: Option<&Path>) -> Option<PathBuf> {
    home.map(|h| h.join("hooks.json"))
}

fn project_hooks_path(root: &Path) -> PathBuf {
    root.join(".codex").join("hooks.json")
}

/// Everything below takes the Codex home as an argument rather than resolving
/// it, so a test can point the whole provider at a directory it created. The
/// trait methods perform the real resolution once and pass it down.
impl Codex {
    pub(crate) fn detected_in(&self, home: Option<&Path>) -> bool {
        home.is_some_and(|h| h.is_dir())
    }

    pub(crate) fn status_in(&self, home: Option<&Path>, root: &Path) -> ProviderStatus {
        if !self.detected_in(home) {
            return ProviderStatus::absent(ProviderId::Codex);
        }

        let project = project_hooks_path(root);
        let user = user_hooks_path(home);

        let capture = if hooks_json::is_installed(&project) {
            Some(InstallScope::Project)
        } else if user.as_deref().is_some_and(hooks_json::is_installed) {
            Some(InstallScope::User)
        } else {
            None
        };

        let sessions = find_sessions_in(home, root).map(|s| s.len()).unwrap_or(0);

        let mut caveats = Vec::new();
        if capture == Some(InstallScope::Project) && !is_trusted_in(home, root) {
            caveats.push(
                "Codex ignores this repository's .codex/ directory until the project is \
                 trusted. Open it in Codex once and accept the trust prompt."
                    .to_string(),
            );
        }
        if capture.is_some() {
            caveats.push(
                "Codex asks you to review a new command hook the first time it runs.".to_string(),
            );
        }
        if let Some(skipped) = compressed_session_count(home) {
            if skipped > 0 {
                caveats.push(format!(
                    "{skipped} older session(s) are compressed and were not read."
                ));
            }
        }

        ProviderStatus {
            provider: ProviderId::Codex,
            detected: true,
            capture,
            sessions,
            caveats,
        }
    }

    pub(crate) fn install_plan_in(
        &self,
        home: Option<&Path>,
        root: &Path,
        scope: InstallScope,
    ) -> Result<InstallPlan> {
        let path = match scope {
            InstallScope::Project => project_hooks_path(root),
            InstallScope::User => user_hooks_path(home)
                .ok_or_else(|| anyhow::anyhow!("could not locate the Codex home directory"))?,
        };

        let (content, merges_existing) = hooks_json::plan_merge(&path, root)?;

        let mut caveats = Vec::new();
        if scope == InstallScope::Project && !is_trusted_in(home, root) {
            caveats.push(
                "This repository is not yet trusted by Codex, so it will ignore .codex/ \
                 until you accept the trust prompt."
                    .to_string(),
            );
        }
        if scope == InstallScope::User {
            caveats.push(
                "A user-level hook runs for every repository. It does nothing in \
                 workspaces that have not enabled intent capture."
                    .to_string(),
            );
        }
        if merges_existing {
            caveats.push(format!(
                "{} already exists; its current entries are kept and a backup is written.",
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
        if let Some(write) = instructions::planned_write(ProviderId::Codex, root) {
            caveats.push(format!(
                "A short section is appended to {} asking the agent to state its \
                 intent. Capture works without it, but the labels are coarser.",
                write.path.display()
            ));
            writes.push(write);
        }

        Ok(InstallPlan {
            provider: ProviderId::Codex,
            scope,
            writes,
            caveats,
        })
    }

    pub(crate) fn history_in(
        &self,
        home: Option<&Path>,
        root: &Path,
    ) -> Result<(Vec<IntentRecord>, Vec<IntentLabel>)> {
        let mut records = Vec::new();
        let labels = Vec::new();
        let mut seq = 0u64;

        for session in find_sessions_in(home, root)? {
            read_rollout(&session, root, &mut seq, &mut records);
        }

        Ok((records, labels))
    }
}

impl Provider for Codex {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn detected(&self) -> bool {
        self.detected_in(codex_home().as_deref())
    }

    fn status(&self, root: &Path) -> ProviderStatus {
        self.status_in(codex_home().as_deref(), root)
    }

    fn install_plan(&self, root: &Path, scope: InstallScope) -> Result<InstallPlan> {
        self.install_plan_in(codex_home().as_deref(), root, scope)
    }

    fn history(&self, root: &Path) -> Result<(Vec<IntentRecord>, Vec<IntentLabel>)> {
        self.history_in(codex_home().as_deref(), root)
    }
}

/// Whether `~/.codex/config.toml` records this path as trusted.
///
/// Read as text rather than parsed: the key is a quoted absolute path whose
/// separators and case vary, and a full TOML parse would still need the same
/// fuzzy comparison afterwards.
fn is_trusted_in(home: Option<&Path>, root: &Path) -> bool {
    let Some(home) = home else {
        return false;
    };
    let Ok(config) = std::fs::read_to_string(home.join("config.toml")) else {
        return false;
    };

    let wanted = normalise_path(&root.to_string_lossy()).to_lowercase();

    let mut in_project = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with("[projects.") {
            let key = normalise_path(line).to_lowercase();
            in_project = key.contains(&wanted);
            continue;
        }
        if line.starts_with('[') {
            in_project = false;
            continue;
        }
        if in_project && line.replace(' ', "").contains("trust_level=\"trusted\"") {
            return true;
        }
    }

    false
}

fn sessions_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    vec![home.join("sessions"), home.join("archived_sessions")]
}

/// Rollout files whose opening `session_meta` names this workspace.
fn find_sessions_in(home: Option<&Path>, root: &Path) -> Result<Vec<PathBuf>> {
    let wanted = normalise_path(&root.to_string_lossy()).to_lowercase();
    let mut found = Vec::new();

    for base in sessions_roots(home) {
        if !base.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&base)
            .max_depth(5)
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if session_cwd(path).is_some_and(|cwd| cwd.to_lowercase() == wanted) {
                found.push(path.to_path_buf());
            }
        }
    }

    found.sort();
    Ok(found)
}

/// Compressed rollouts we deliberately do not read, so the count can be shown.
fn compressed_session_count(home: Option<&Path>) -> Option<usize> {
    let mut count = 0;
    for base in sessions_roots(home) {
        if !base.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&base)
            .max_depth(5)
            .into_iter()
            .flatten()
        {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("zst") {
                count += 1;
            }
        }
    }
    Some(count)
}

/// The `cwd` from a rollout's first line.
///
/// `session_meta` is always first, so this reads one line rather than the
/// whole file — the difference between a scan that is instant and one that is
/// not, across a year of dated directories.
fn session_cwd(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;

    let line: Value = serde_json::from_str(&first).ok()?;
    if line.get("type")?.as_str()? != "session_meta" {
        return None;
    }

    Some(normalise_path(line.get("payload")?.get("cwd")?.as_str()?))
}

fn session_branch(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;

    let line: Value = serde_json::from_str(&first).ok()?;
    line.get("payload")?
        .get("git")?
        .get("branch")?
        .as_str()
        .map(str::to_string)
}

/// Read every edit a rollout file recorded.
///
/// Codex persists an edit in two shapes and which one appears depends on the
/// session's history mode, so both are handled. Deduplication downstream keys
/// on the call id, which both shapes carry, so a session recording both forms
/// contributes one record rather than two.
fn read_rollout(path: &Path, root: &Path, seq: &mut u64, out: &mut Vec<IntentRecord>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let branch = session_branch(path);

    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(payload) = value.get("payload") else {
            continue;
        };

        match value.get("type").and_then(Value::as_str) {
            Some("response_item") => {
                read_custom_tool_call(payload, root, &branch, seq, out);
            }
            Some("event_msg") => {
                read_patch_apply_end(payload, root, &branch, seq, out);
            }
            _ => {}
        }
    }
}

/// The raw-envelope shape: `custom_tool_call` with the patch as its input.
fn read_custom_tool_call(
    payload: &Value,
    root: &Path,
    branch: &Option<String>,
    seq: &mut u64,
    out: &mut Vec<IntentRecord>,
) {
    if payload.get("type").and_then(Value::as_str) != Some("custom_tool_call") {
        return;
    }
    if payload.get("name").and_then(Value::as_str) != Some("apply_patch") {
        return;
    }

    let call_id = payload
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(input) = payload.get("input").and_then(Value::as_str) else {
        return;
    };

    // A raw tool call carries no turn of its own, so every file the one patch
    // named shares an intent — which is exactly what a single patch is.
    for file in patchfmt::parse_envelope(input) {
        push_record(
            out, root, branch, seq, &file.path, file.edit, call_id, call_id,
        );
    }
}

/// The structured shape: `patch_apply_end` with a map of per-file changes.
fn read_patch_apply_end(
    payload: &Value,
    root: &Path,
    branch: &Option<String>,
    seq: &mut u64,
    out: &mut Vec<IntentRecord>,
) {
    if payload.get("type").and_then(Value::as_str) != Some("patch_apply_end") {
        return;
    }
    // A failed or declined patch never reached the working tree.
    if payload.get("success").and_then(Value::as_bool) == Some(false) {
        return;
    }

    let call_id = payload
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // Everything Codex did in one turn is one intent. Keying on the individual
    // call instead would make every edit its own card, which is the opposite
    // of what the grouping is for.
    let turn = payload
        .get("turn_id")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .unwrap_or(call_id);

    let Some(changes) = payload.get("changes").and_then(Value::as_object) else {
        return;
    };

    for (path, change) in changes {
        let Some(edit) = file_change_to_edit(change) else {
            continue;
        };
        push_record(out, root, branch, seq, path, edit, call_id, turn);
    }
}

/// Codex's `FileChange` enum, in its serialised form.
fn file_change_to_edit(change: &Value) -> Option<IntentEdit> {
    match change.get("type").and_then(Value::as_str)? {
        "add" => Some(IntentEdit {
            old_lines: Vec::new(),
            new_lines: split_lines(change.get("content")?.as_str()?),
            whole_file: true,
        }),
        "delete" => Some(IntentEdit {
            old_lines: split_lines(change.get("content")?.as_str()?),
            new_lines: Vec::new(),
            whole_file: true,
        }),
        "update" => {
            let diff = change.get("unified_diff")?.as_str()?;
            Some(patchfmt::parse_unified_diff(diff))
        }
        _ => None,
    }
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn push_record(
    out: &mut Vec<IntentRecord>,
    root: &Path,
    branch: &Option<String>,
    seq: &mut u64,
    path: &str,
    edit: IntentEdit,
    call_id: &str,
    turn: &str,
) {
    if edit.is_empty() {
        return;
    }
    let Some(relative) = crate::intents::relative_to(root, path) else {
        return;
    };

    out.push(IntentRecord {
        provider: ProviderId::Codex,
        turn_id: format!("codex-history-{turn}"),
        tool_use_id: format!("{call_id}:{relative}"),
        seq: {
            *seq += 1;
            *seq
        },
        path: relative,
        edit,
        branch: branch.clone(),
    });
}

/// The hook entries Codex should run for us.
pub(super) fn hook_commands(root: &Path) -> Value {
    hooks_json::commands_for(root, "codex")
}

/// Exposed for the plan renderer, which shows what will be added.
pub fn planned_entries(root: &Path) -> Value {
    json!({ "hooks": hook_commands(root) })
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod codex_tests;
