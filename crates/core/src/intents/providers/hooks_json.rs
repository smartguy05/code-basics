//! Merging our hooks into a configuration file the user already owns.
//!
//! Both agents keep hooks in the same JSON shape:
//!
//! ```json
//! { "hooks": { "PostToolUse": [ { "matcher": "...",
//!     "hooks": [ { "type": "command", "command": "...", "timeout": 5 } ] } ] } }
//! ```
//!
//! Claude Code nests it inside `settings.json` alongside unrelated settings;
//! Codex gives it a file of its own. Either way the rule is the same and it is
//! the whole point of this module: **never rewrite the file**. On the machine
//! this was developed against, `~/.codex/hooks.json` already drove a physical
//! LCD dashboard from all seven events. Replacing that file would have broken
//! something the user never mentioned and would not have connected to this
//! feature.
//!
//! So the merge is surgical: parse what is there, add our entry to the arrays
//! for the two events we care about if it is not already present, and leave
//! every other key, event and handler exactly as found — including unknown
//! ones from a future version of either agent.
//!
//! Our entries are recognised by a marker in the command string rather than by
//! position, so installing twice is a no-op and the user reordering their own
//! hooks cannot confuse us.

use std::path::Path;

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::{settings_merge, EDIT_TOOL_MATCHER};

/// Present in every command we write, so our own entries can be recognised
/// again later without depending on their exact text.
pub const MARKER: &str = "code-basics-intent";

/// The events we register for, and why each is needed.
///
/// `PostToolUse` gives the geometry of an edit; `Stop` and `SubagentStop` are
/// where an agent exposes the reasoning, via `last_assistant_message` — the
/// latter capturing a subagent's closing message the same way.
pub const EVENTS: &[&str] = &["PostToolUse", "Stop", "SubagentStop"];

/// Is our hook already configured in this file?
pub fn is_installed(path: &Path) -> bool {
    settings_merge::is_installed(path, EVENTS, MARKER)
}

/// The hook entries as they should appear in the file.
///
/// `workspace` is `Some` for a project-scope install, which belongs to exactly
/// one repository, and `None` for a user-scope one, which belongs to all of
/// them — see [`command_line`].
pub fn commands_for(workspace: Option<&Path>, provider: &str) -> Value {
    let mut hooks = Map::new();

    for event in EVENTS {
        let entry = json!({
            "matcher": if *event == "PostToolUse" { EDIT_TOOL_MATCHER } else { "" },
            "hooks": [ {
                "type": "command",
                "command": command_line(workspace, provider, event),
                "timeout": 5,
            } ],
        });
        hooks.insert((*event).to_string(), Value::Array(vec![entry]));
    }

    Value::Object(hooks)
}

/// The command a hook runs.
///
/// It invokes this application rather than a shipped script, so there is no
/// second artifact to keep in step with the record format, and no interpreter
/// to depend on being installed.
///
/// A project-scope hook lives inside one repository and names it, so the
/// recorder never has to guess. A user-scope hook must **not**: it fires for
/// every repository on the machine, and naming the one that happened to be
/// open at install time pinned it there forever — every other workspace's
/// edits were silently dropped. Without the flag the recorder resolves the
/// workspace from the payload's `cwd` and records only where capture was
/// enabled, which is the check that was meant to do this job all along.
fn command_line(workspace: Option<&Path>, provider: &str, event: &str) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "code-basics".to_string());

    let mut line =
        format!("\"{exe}\" record-intent --{MARKER} --provider {provider} --event {event}");
    if let Some(root) = workspace {
        line.push_str(&format!(" --workspace \"{}\"", root.display()));
    }
    line
}

/// The workspace an already-installed hook is pinned to, if any.
///
/// `None` covers both "not installed" and the un-pinned form a user-scope
/// install now writes. A `Some` from a user-scope file is the bug: the hook
/// records for that path and nowhere else.
pub fn pinned_workspace(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let hooks = value.get("hooks")?.as_object()?;

    for event in EVENTS {
        let Some(entries) = hooks.get(*event).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(handlers) = entry.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for handler in handlers {
                let Some(command) = handler.get("command").and_then(Value::as_str) else {
                    continue;
                };
                if !command.contains(MARKER) {
                    continue;
                }
                if let Some(pinned) = quoted_workspace(command) {
                    return Some(pinned);
                }
            }
        }
    }

    None
}

/// The workspace an installed hook is pinned to, when that is *not* the one
/// being asked about — the case where the hook looks installed and records
/// nothing.
///
/// Compared the way every other path comparison in this crate is: separators
/// normalised, and case-insensitively on Windows, where the same directory
/// legitimately differs in case between what was recorded and what the
/// workspace was opened as.
pub fn pinned_elsewhere(path: &Path, root: &Path) -> Option<String> {
    let pinned = pinned_workspace(path)?;

    let same = {
        let a = crate::intents::normalise_path(&pinned);
        let b = crate::intents::normalise_path(&root.to_string_lossy());
        let (a, b) = (a.trim_end_matches('/'), b.trim_end_matches('/'));
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };

    (!same).then_some(pinned)
}

/// What to tell the user about a hook pinned somewhere else.
///
/// Re-installing is the whole repair, and saying so matters: the entry is
/// replaced rather than added to, so nobody has to hand-edit a shared file.
pub fn pinned_caveat(pinned: &str) -> String {
    format!(
        "Your user-level hook is pinned to {pinned} and will not record here. \
         Enable capture again to repair it — the entry is replaced, not duplicated."
    )
}

/// Read `--workspace "<path>"` back out of a command we wrote.
fn quoted_workspace(command: &str) -> Option<String> {
    let after = command.split("--workspace").nth(1)?.trim_start();
    let inside = after.strip_prefix('"')?;
    let (path, _) = inside.split_once('"')?;
    (!path.is_empty()).then(|| path.to_string())
}

/// Compute the file's contents after merging, without writing anything.
///
/// Returns the new text and whether an existing file is being merged into,
/// which the confirmation UI shows prominently — that is the case where the
/// user has something to lose.
pub fn plan_merge(path: &Path, workspace: Option<&Path>) -> Result<(String, bool)> {
    let provider = if path.components().any(|c| c.as_os_str() == ".codex")
        || path.parent().is_some_and(|p| p.ends_with(".codex"))
    {
        "codex"
    } else {
        "claudeCode"
    };

    settings_merge::merged_text(path, &commands_for(workspace, provider), MARKER)
}

/// Remove our entries again, leaving everything else untouched.
pub fn plan_removal(path: &Path) -> Result<Option<String>> {
    settings_merge::plan_removal(path, EVENTS, MARKER)
}
