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
    append_edit, append_label, intents_dir, next_seq, IntentEdit, IntentLabel, IntentRecord,
    LabelSource, ProviderId,
};

/// Did the command line ask for recording rather than for the application?
///
/// The installed hook line carries both the subcommand and the marker flag,
/// and either alone is accepted: it lives in a config file the user shares
/// with their team, so a hand-edited line keeping only one must still record.
/// The marker is read back through the same constant that writes it
/// ([`super::providers::hooks_json::MARKER`]) so the two cannot drift.
pub fn is_record_invocation(args: &[String]) -> bool {
    let marker = format!("--{}", super::providers::hooks_json::MARKER);
    args.iter()
        .any(|arg| arg == "record-intent" || *arg == marker)
}

/// What a `record-intent` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderInvocation {
    /// Which agent installed the hook that fired.
    pub provider: ProviderId,
    /// Which lifecycle event fired.
    pub event: HookEvent,
    /// The workspace the hook was installed for, if it named one. `None`
    /// leaves the root to the payload — see [`resolve_root`].
    pub workspace: Option<String>,
}

/// Read a `record-intent` command line.
///
/// `None` means there is nothing to do and nothing wrong: either this is an
/// ordinary application launch, or some other lifecycle event fired. The
/// recorder must be silent in both cases, so neither is an error.
pub fn parse_recorder_args(args: &[String]) -> Option<RecorderInvocation> {
    if !is_record_invocation(args) {
        return None;
    }

    let event = HookEvent::parse(&flag(args, "--event")?)?;

    // Claude Code is the default rather than a rejected unknown: a hook line
    // written before the flag existed must keep recording.
    let provider = match flag(args, "--provider").as_deref() {
        Some("codex") => ProviderId::Codex,
        _ => ProviderId::ClaudeCode,
    };

    Some(RecorderInvocation {
        provider,
        event,
        workspace: flag(args, "--workspace").filter(|w| !w.is_empty()),
    })
}

/// Read `--name value` from the command line.
///
/// The first occurrence wins, and a flag with nothing after it has no value
/// rather than swallowing whatever follows.
fn flag(args: &[String], name: &str) -> Option<String> {
    let position = args.iter().position(|a| a == name)?;
    args.get(position + 1).cloned()
}

/// Which lifecycle event fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PostToolUse,
    Stop,
    /// A subagent finished. It carries the same closing message a [`Stop`](Self::Stop)
    /// does, so its label is recorded the same way — but it must **never** go
    /// through [`ask_for_intent`]: blocking a subagent could hang it, and
    /// nothing establishes the platform honours a refusal here.
    SubagentStop,
    /// A git `post-commit` hook fired. Unlike the agent events it carries no
    /// stdin payload — the workspace is always named on the command line — and
    /// its job is to persist the durable-why note for the new commit, not to
    /// ingest an edit.
    PostCommit,
}

impl HookEvent {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "PostToolUse" => Some(HookEvent::PostToolUse),
            "Stop" => Some(HookEvent::Stop),
            "SubagentStop" => Some(HookEvent::SubagentStop),
            "PostCommit" => Some(HookEvent::PostCommit),
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
        // A subagent's closing message is a reason like any other; only the
        // *asking* path (`ask_for_intent`) is Stop-only, never this one.
        HookEvent::Stop | HookEvent::SubagentStop => ingest_label(root, provider, payload),
        // A post-commit hook has no payload to ingest — the recorder handles it
        // on its own path before reaching here.
        HookEvent::PostCommit => Ok(0),
    }
}

/// The one file that records which turns have already been asked.
fn asked_path(root: &Path) -> PathBuf {
    intents_dir(root).join("asked.jsonl")
}

/// Should the agent be asked for an `Intent:` line before it stops?
///
/// `Some(message)` means yes, and the caller is expected to put the message in
/// front of the agent and refuse the stop. This is the only place the project
/// deliberately interrupts somebody else's tool, so every condition below is a
/// reason **not** to, and calling it also *records* that the turn was asked —
/// see the loop guard.
///
/// # Why the loop guard is ours
///
/// A `Stop` hook that always blocks makes a session impossible to end
/// gracefully. The platform may or may not expose a flag for this; the two
/// descriptions of the contract I could find disagree about whether it exists.
/// So the guard does not depend on one: the turn id is written here the first
/// time, and a second `Stop` for the same turn is never asked, whatever the
/// agent did or did not do in between. The worst case is one unlabelled turn.
///
/// # Why Claude Code only
///
/// Codex's hooks are a close relative and run this same binary, but nothing
/// establishes that Codex honours a blocking stop — and a hook that fails to
/// block but writes to stderr is pure noise in someone's session.
pub fn ask_for_intent(
    root: &Path,
    provider: ProviderId,
    event: HookEvent,
    payload: &Value,
) -> Option<String> {
    if event != HookEvent::Stop || provider != ProviderId::ClaudeCode {
        return None;
    }
    // A user-level hook fires in every repository on the machine.
    if !is_enabled(root) {
        return None;
    }
    if !crate::config::load(root)
        .map(|c| c.ask_for_intent)
        .unwrap_or(true)
    {
        return None;
    }

    let message = payload
        .get("last_assistant_message")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // The agent already said why. Nothing to ask for.
    if !parse_declared_labels(message).is_empty() {
        return None;
    }

    let turn = turn_id(payload);

    // Only a turn that actually changed something has anything to explain; a
    // conversational turn must never be nagged.
    if !turn_edited_anything(root, &turn) {
        return None;
    }

    if already_asked(root, &turn) {
        return None;
    }
    record_asked(root, &turn);

    Some(INTENT_REQUEST.to_string())
}

/// What the agent is told. Names the exact form [`parse_declared_labels`]
/// accepts, so complying actually works — the same wording
/// `providers::instructions` writes into `CLAUDE.md`.
const INTENT_REQUEST: &str = "\
This turn edited files but did not say why. Add one line to your reply so the \
change can be labelled in review:\n\
\n\
    Intent: <3-5 words describing why>\n\
\n\
Scope it to particular files if the turn did unrelated things:\n\
\n\
    Intent(src/api.ts, src/apiLogic.test.ts): <why, for those files>\n\
\n\
Keep it short enough to read at a glance — it titles a group of hunks in the \
Changes tab, not a commit message.";

/// Did this turn record any edit?
fn turn_edited_anything(root: &Path, turn: &str) -> bool {
    let Ok(intents) = super::load(root, &super::LoadOptions::default()) else {
        return false;
    };
    intents.records.iter().any(|r| r.turn_id == turn)
}

fn already_asked(root: &Path, turn: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(asked_path(root)) else {
        return false;
    };
    contents.lines().any(|line| line.trim() == turn)
}

/// Remember that this turn was asked.
///
/// A write failure is deliberately ignored: the cost of not remembering is one
/// extra request, and returning an error here would push a failure into a hook
/// whose whole contract is to stay quiet.
fn record_asked(root: &Path, turn: &str) {
    use std::io::Write;

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(asked_path(root))
        .and_then(|mut file| writeln!(file, "{turn}"));
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
    let labels = parse_labels_with_source(message);
    if labels.is_empty() {
        return Ok(0);
    }

    for (paths, text, source) in &labels {
        append_label(
            root,
            &IntentLabel {
                provider,
                turn_id: turn.clone(),
                label: text.clone(),
                paths: paths.clone(),
                anchor: None,
                source: *source,
            },
        )?;
    }

    Ok(labels.len())
}

/// Read the labels an agent declared in its closing message.
///
/// Explicit `Intent:` lines are preferred. Failing those, the first sentence
/// stands in — coarse, but an unexplained change is worse. The two are not
/// interchangeable and the caller is told which it got: see [`LabelSource`].
pub fn parse_labels_with_source(message: &str) -> Vec<(Vec<String>, String, LabelSource)> {
    let declared = parse_declared_labels(message);
    if !declared.is_empty() {
        return declared
            .into_iter()
            .map(|(paths, text)| (paths, text, LabelSource::Declared))
            .collect();
    }

    first_sentence(message)
        .map(|text| vec![(Vec::new(), text, LabelSource::Inferred)])
        .unwrap_or_default()
}

/// Only the explicitly declared `Intent:` lines.
pub fn parse_declared_labels(message: &str) -> Vec<(Vec<String>, String)> {
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

    found
}

/// Declared lines, or the first sentence when there are none.
///
/// Kept as the plain view for callers that only want the words; anything that
/// records a label wants [`parse_labels_with_source`] instead, because a card
/// may only claim a *stated* intent for a declared one.
pub fn parse_labels(message: &str) -> Vec<(Vec<String>, String)> {
    parse_labels_with_source(message)
        .into_iter()
        .map(|(paths, text, _)| (paths, text))
        .collect()
}

/// `line` without a leading `prefix`, compared case-insensitively, or `None`.
///
/// `str::get` rather than `line[..prefix.len()]`. The length check is in
/// **bytes**, so it passes for a line whose `prefix.len()`-th byte sits *inside*
/// a character, and slicing there is a panic rather than a mismatch. That is not
/// a corner case: `"intent"` is six bytes, so any line starting with four ASCII
/// bytes and then an em dash, a curly quote or an arrow lands exactly on it.
///
/// It brought the `Stop` hook down in the running app on the line
/// `"Yes — verified three ways:"` — ordinary prose, nothing to do with intents.
/// A panic here loses the recorded reason for the whole turn, which is the one
/// thing this module exists to keep, so it must fail closed and quietly.
///
/// `get` returns `None` for a non-boundary, which is the right answer for both
/// reasons a caller could get one: the line is shorter than the prefix, or it
/// does not begin with it. The `prefix` is ASCII at every call site, which is
/// what makes `eq_ignore_ascii_case` the correct comparison.
fn strip_prefix_ignoring_case<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let head = line.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &line[prefix.len()..])
}

/// A label has to fit on a card and actually say something.
fn is_usable_label(text: &str) -> bool {
    (3..=120).contains(&text.len()) && text.chars().any(char::is_alphanumeric)
}

/// Words that name the tooling rather than the code.
///
/// A sentence built around one of these is about the session — which model ran,
/// which agent reported back, how much context is left — and titling a set of
/// hunks with it says nothing about what changed. This is the shape that
/// produced *"The workflow is running with Opus"* over three files.
const TOOLING_WORDS: &[&str] = &[
    "workflow",
    "subagent",
    "sub-agent",
    "agent",
    "session",
    "context window",
    "token budget",
    "opus",
    "sonnet",
    "haiku",
    "claude",
    "codex",
];

/// Openers that acknowledge rather than describe.
const PLEASANTRIES: &[&str] = &[
    "perfect",
    "great",
    "excellent",
    "sure",
    "okay",
    "ok",
    "right",
    "absolutely",
    "certainly",
    "thanks",
    "thank you",
    "sorry",
    "done",
    "yes",
    "no",
    "exactly",
    "indeed",
    "understood",
    "correct",
    "good",
    "nice",
    "you are right",
    "you're right",
    // Sign-offs: "That's everything I needed", "That is done".
    "that's",
    "thats",
    "that is",
    "all done",
    "both done",
];

/// Openers that announce the next action instead of reporting a change.
///
/// These matter more than they look: the prose an agent writes before a tool
/// call describes what it is *about to* do, and the edits that follow are
/// frequently not the ones the sentence names.
const ANNOUNCEMENTS: &[&str] = &[
    "let me",
    "let us",
    "let's",
    "i'll",
    "i will",
    "i am going to",
    "i'm going to",
    "now i",
    "next i",
    "first i",
    "here is",
    "here's",
    "looking at",
    "starting with",
];

/// Words that open a question.
const INTERROGATIVES: &[&str] = &[
    "what", "why", "how", "when", "where", "which", "who", "should", "could", "would", "shall",
    "do ", "does ", "did ", "is ", "are ", "can ", "was ", "were ",
];

/// Would this sentence be narration rather than a description of the change?
///
/// Only ever applied to an **inferred** label — a first sentence mined out of
/// prose that was written for a human reading a chat. A declared `Intent:` line
/// is the agent's own words for the card and is taken as given.
///
/// Deliberately blunt. Every rule here can refuse a sentence that would have
/// been fine — "record what the agent said" names this repository's own domain
/// and would be refused as tooling talk. That trade is the project's standing
/// one: a card titled *"The workflow is running with Opus"* is worse than a
/// card that admits it has no stated reason, and the hunks are still grouped
/// and still reviewable either way.
pub fn looks_like_narration(text: &str) -> bool {
    let lower = text.trim().to_lowercase();

    // A tooling word anywhere is enough: these sentences are about the run.
    if TOOLING_WORDS.iter().any(|word| contains_word(&lower, word)) {
        return true;
    }

    let opens_with = |candidates: &[&str]| {
        candidates.iter().any(|candidate| {
            lower.strip_prefix(candidate).is_some_and(|rest| {
                // A prefix only counts at a word boundary, so "index" is not
                // read as the pleasantry "in".
                rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric())
            })
        })
    };

    opens_with(PLEASANTRIES) || opens_with(ANNOUNCEMENTS) || opens_with(INTERROGATIVES)
}

/// Whole-word containment, so "management" does not match "agent".
///
/// A trailing `s` is allowed, because the plural is the same word and missing
/// it let *"Running — 10 agents, 5 phases"* through. Nothing longer is: that
/// would make "agenda" and "sessionStorage" match, and refusing a real label is
/// the cost this gate is trying to keep small.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let boundary = |c: char| !c.is_alphanumeric();

    haystack.match_indices(needle).any(|(at, _)| {
        let before_ok = at == 0 || haystack[..at].chars().next_back().is_some_and(boundary);

        let after = &haystack[at + needle.len()..];
        let after = after.strip_prefix('s').unwrap_or(after);
        let after_ok = after.is_empty() || after.starts_with(boundary);

        before_ok && after_ok
    })
}

/// The shortest inferred label that can mean anything.
///
/// Eight characters was the bar `providers::claude_code::summarise` used and
/// the live hook did not apply at all, which is how cards came to be titled
/// *"Running"* and *"Both done"*. Twelve is about three words — under that a
/// sentence cannot name both a thing and what happened to it, and "N files
/// changed together" is the more useful title.
const MIN_INFERRED_LABEL: usize = 12;

/// Is this a label worth showing, given that nobody offered it as one?
///
/// The single place inferred labels are judged, so the hook that records them
/// and the loader that reads back everything recorded before this existed
/// cannot disagree.
pub fn is_usable_inferred_label(text: &str) -> bool {
    let trimmed = text.trim();
    is_usable_label(trimmed)
        && trimmed.len() >= MIN_INFERRED_LABEL
        && !looks_like_narration(trimmed)
}

/// The first sentence of a message, when it reads like a reason for a change.
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

    is_usable_inferred_label(cleaned).then(|| cleaned.to_string())
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

/// Which enabled workspace this invocation should record into.
///
/// An explicit workspace is taken as given — a project-scope hook lives inside
/// the repository it names, so there is nothing to search for and no reason to
/// climb out of it. The caller still checks [`is_enabled`].
///
/// With no workspace named — the user-scope case, which fires for every
/// repository on the machine — the payload's `cwd` decides. It is the
/// directory the agent was started in, which is routinely *below* the
/// workspace root, so the ancestors are walked until one has capture enabled.
/// Nothing enabled anywhere on that chain means this repository never opted
/// in, and `None` is the correct, silent answer.
pub fn resolve_enabled_root(
    explicit: Option<&Path>,
    payload_cwd: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|p| !p.as_os_str().is_empty()) {
        return Some(path.to_path_buf());
    }

    payload_cwd?
        .ancestors()
        .find(|candidate| is_enabled(candidate))
        .map(Path::to_path_buf)
}

#[cfg(test)]
#[path = "hook_tests.rs"]
mod tests;
