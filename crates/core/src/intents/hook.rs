//! Turning a hook payload into a record.
//!
//! This runs inside the hook the agent invokes, which shapes every decision
//! here. It executes after **every** edit, so it has to be fast; it runs
//! unattended, so it must never fail loudly; and a user-level hook fires for
//! every repository on the machine, so most invocations are for a workspace
//! that never asked for any of this and must do nothing at all.
//!
//! # The two events
//!
//! `PostToolUse` carries what changed but no reason. `Stop` carries the
//! agent's closing message — the only place either agent exposes a reason —
//! but says nothing about which edit it refers to. Both carry the turn
//! identifier, so writing them separately and joining on it afterwards
//! recovers what neither has alone.
//!
//! # Asking for the label
//!
//! Neither agent lets a model attach a rationale to a tool call, so the label
//! has to be *requested* in `CLAUDE.md` / `AGENTS.md` and parsed back out of
//! the closing message. The requested form is a line like:
//!
//! ```text
//! Intent: add retry to token refresh
//! Intent(src/auth.rs): cache the refreshed token
//! ```
//!
//! When the agent says nothing of the sort, the first sentence of its closing
//! message is used instead. That is a weaker label and is treated as such,
//! but it is still better than an unexplained hunk.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use super::patchfmt;
use super::{
    append_edit, append_label, next_seq, IntentEdit, IntentLabel, IntentRecord, ProviderId,
};

/// Which lifecycle event fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PostToolUse,
    Stop,
}

impl HookEvent {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "PostToolUse" => Some(HookEvent::PostToolUse),
            "Stop" => Some(HookEvent::Stop),
            _ => None,
        }
    }
}

/// Record whatever a hook payload describes.
///
/// Returns how many records were written, which is zero for the many events
/// that are not edits at all.
pub fn ingest(
    root: &Path,
    provider: ProviderId,
    event: HookEvent,
    payload: &Value,
) -> Result<usize> {
    match event {
        HookEvent::PostToolUse => ingest_edit(root, provider, payload),
        HookEvent::Stop => ingest_label(root, provider, payload),
    }
}

/// The turn identifier, under whichever name this agent uses for it.
fn turn_id(payload: &Value) -> String {
    for key in ["turn_id", "prompt_id", "session_id"] {
        if let Some(value) = payload.get(key).and_then(Value::as_str) {
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    "unknown-turn".to_string()
}

fn ingest_edit(root: &Path, provider: ProviderId, payload: &Value) -> Result<usize> {
    let Some(input) = payload.get("tool_input") else {
        return Ok(0);
    };
    let tool = payload
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let edits = extract_edits(tool, input);
    if edits.is_empty() {
        return Ok(0);
    }

    let turn = turn_id(payload);
    let call = payload
        .get("tool_use_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let branch = current_branch(root);
    let mut seq = next_seq(root);
    let mut written = 0;

    for (n, (path, edit)) in edits.into_iter().enumerate() {
        let Some(relative) = super::relative_to(root, &path) else {
            continue;
        };
        if edit.is_empty() {
            continue;
        }

        append_edit(
            root,
            &IntentRecord {
                provider,
                turn_id: turn.clone(),
                tool_use_id: format!("{call}:{n}"),
                seq,
                path: relative,
                edit,
                branch: branch.clone(),
            },
        )?;
        seq += 1;
        written += 1;
    }

    Ok(written)
}

/// Pull every file change out of a tool payload, whatever its shape.
///
/// The three shapes are Claude Code's before/after strings, Claude Code's
/// array of them, and Codex's patch envelope — which may itself arrive as a
/// field, as raw text, or nested inside a shell invocation.
fn extract_edits(tool: &str, input: &Value) -> Vec<(String, IntentEdit)> {
    // A patch envelope names its own files, so it is checked first and
    // regardless of the tool name: Codex routes `apply_patch` through the
    // shell as well as calling it directly.
    if let Some(envelope) = patchfmt::envelope_from_value(input) {
        return patchfmt::parse_envelope(&envelope)
            .into_iter()
            .map(|file| (file.path, file.edit))
            .collect();
    }

    let Some(path) = input.get("file_path").and_then(Value::as_str) else {
        return Vec::new();
    };

    // A whole-file write, under either agent's spelling for the content.
    if tool.eq_ignore_ascii_case("write") {
        if let Some(text) = input
            .get("content")
            .or_else(|| input.get("file_text"))
            .and_then(Value::as_str)
        {
            return vec![(
                path.to_string(),
                IntentEdit {
                    old_lines: Vec::new(),
                    new_lines: lines_of(text),
                    whole_file: true,
                },
            )];
        }
    }

    if let Some(items) = input.get("edits").and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(pair_to_edit)
            .map(|edit| (path.to_string(), edit))
            .collect();
    }

    pair_to_edit(input)
        .map(|edit| vec![(path.to_string(), edit)])
        .unwrap_or_default()
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

fn lines_of(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect()
}

fn ingest_label(root: &Path, provider: ProviderId, payload: &Value) -> Result<usize> {
    let Some(message) = payload
        .get("last_assistant_message")
        .and_then(Value::as_str)
    else {
        return Ok(0);
    };

    let turn = turn_id(payload);
    let labels = parse_labels(message);
    if labels.is_empty() {
        return Ok(0);
    }

    for (paths, text) in &labels {
        append_label(
            root,
            &IntentLabel {
                provider,
                turn_id: turn.clone(),
                label: text.clone(),
                paths: paths.clone(),
                anchor: None,
            },
        )?;
    }

    Ok(labels.len())
}

/// Read the labels an agent declared in its closing message.
///
/// Explicit `Intent:` lines are preferred. Failing those, the first sentence
/// stands in — coarse, but an unexplained change is worse.
pub fn parse_labels(message: &str) -> Vec<(Vec<String>, String)> {
    let mut found = Vec::new();

    for line in message.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '#']).trim();

        let Some(rest) = strip_prefix_ignoring_case(line, "intent") else {
            continue;
        };

        // An optional parenthesised file list scopes the label.
        let (paths, text) = match rest.strip_prefix('(') {
            Some(after) => match after.split_once(')') {
                Some((inside, remainder)) => (
                    inside
                        .split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect(),
                    remainder,
                ),
                None => (Vec::new(), rest),
            },
            None => (Vec::new(), rest),
        };

        let Some(text) = text.trim().strip_prefix(':') else {
            continue;
        };
        let text = text.trim();

        if is_usable_label(text) {
            found.push((paths, text.to_string()));
        }
    }

    if !found.is_empty() {
        return found;
    }

    first_sentence(message)
        .map(|text| vec![(Vec::new(), text)])
        .unwrap_or_default()
}

fn strip_prefix_ignoring_case<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    (line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(prefix))
        .then(|| &line[prefix.len()..])
}

/// A label has to fit on a card and actually say something.
fn is_usable_label(text: &str) -> bool {
    (3..=120).contains(&text.len()) && text.chars().any(char::is_alphanumeric)
}

fn first_sentence(message: &str) -> Option<String> {
    let line = message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("```"))?;

    let sentence = line
        .split_terminator(['.', '!', '?'])
        .next()
        .unwrap_or(line);
    let cleaned = sentence.trim().trim_end_matches(':').trim();

    is_usable_label(cleaned).then(|| cleaned.to_string())
}

/// The branch a workspace is on, so records from elsewhere can be filtered.
///
/// Failure is normal — the hook may run outside a repository — and is not
/// worth reporting.
fn current_branch(root: &Path) -> Option<String> {
    let repo = crate::git::Repo::open(root).ok()?;
    repo.status().ok()?.branch
}

/// Should this invocation do anything at all?
///
/// A user-level hook fires for every repository on the machine. Recording into
/// one that never enabled capture would litter unrelated projects, so the
/// directory has to exist already — created when the user turned capture on.
pub fn is_enabled(root: &Path) -> bool {
    super::intents_dir(root).is_dir()
}

/// Where a hook invocation should record, given what the payload says.
///
/// The workspace named on the command line wins; the payload's `cwd` is the
/// fallback for a hook installed without one.
pub fn resolve_root(explicit: Option<&str>, payload: &Value) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
}

#[cfg(test)]
#[path = "hook_tests.rs"]
mod tests;
