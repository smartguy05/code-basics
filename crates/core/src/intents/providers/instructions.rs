//! Asking the agent for a reason.
//!
//! The `Stop` hook can read the agent's closing message, but only the agent
//! can decide to put something useful in it. Neither Claude Code nor Codex
//! offers a structured channel for that, so the request goes where the agent
//! already reads standing instructions: `CLAUDE.md` or `AGENTS.md`.
//!
//! Without this, capture still works — the hook falls back to the first
//! sentence of the closing message. But that sentence was written for a human
//! reading a chat, not as a label, and it shows: measured against this
//! repository's own history the fallback produced labels like *"Now register
//! the module and write the attribution tests"*. Usable, but not the three or
//! four words that make a card scannable.
//!
//! Appending to a file the user wrote is intrusive, so the rules are the same
//! as for hook configuration: append only, never rewrite, mark our own section
//! so it can be recognised, and do nothing at all if it is already present.

use std::path::{Path, PathBuf};

use super::PlannedWrite;
use crate::intents::ProviderId;

/// Marks our section so a second install can recognise it.
pub const MARKER: &str = "<!-- code-basics: agent intent -->";

/// Closes the section, bounding what a re-install may rewrite.
pub const END_MARKER: &str = "<!-- /code-basics -->";

/// What the agent is asked to do.
///
/// Deliberately short. A long instruction competes for attention with the
/// project's own guidance, and this one only has to describe a single line of
/// output.
const SECTION: &str = r#"
<!-- code-basics: agent intent -->
## Recording why you changed something

When you finish a turn in which you edited files, end your reply with one line
saying why:

```
Intent: <3-5 words describing why>
```

If the turn made unrelated changes, scope each reason to its files instead of
writing several plain lines — only the first plain line is used. Paths are
workspace-relative, comma-separated:

```
Intent(src/api.ts, src/apiLogic.test.ts): <why, for those files>
```

A scoped line covers the files it names; one plain line may cover the rest.
Keep each label short enough to read at a glance — it titles a group of hunks
in the Changes tab, not a commit message.
<!-- /code-basics -->
"#;

/// The instruction file each agent reads.
pub fn path_for(provider: ProviderId, root: &Path) -> PathBuf {
    match provider {
        ProviderId::ClaudeCode => root.join("CLAUDE.md"),
        ProviderId::Codex => root.join("AGENTS.md"),
    }
}

/// Is the request already in this file?
pub fn is_present(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.contains(MARKER))
}

/// The file's contents with the request appended — or, when an earlier
/// install already left the marked section behind, rewritten in place so a
/// re-install refreshes stale wording. `None` when nothing needs to change.
///
/// Returns a [`PlannedWrite`] so it renders in the same confirmation dialog as
/// the hook configuration: the user sees every file that will be touched,
/// including one they may well have written themselves.
pub fn planned_write(provider: ProviderId, root: &Path) -> Option<PlannedWrite> {
    let path = path_for(provider, root);
    if is_present(&path) {
        return refreshed(&path);
    }

    let existing = std::fs::read_to_string(&path).ok();
    let merges_existing = existing.is_some();

    let mut content = existing.unwrap_or_default();
    if !content.is_empty() {
        // Exactly one blank line between whatever was there and our heading,
        // however the file happened to end.
        while content.ends_with('\n') {
            content.pop();
        }
        content.push_str("\n\n");
    }
    content.push_str(SECTION.trim_start());

    Some(PlannedWrite {
        path,
        content,
        merges_existing,
    })
}

/// Replace the marked span with the current request.
///
/// Only what sits between [`MARKER`] and [`END_MARKER`] is ours to rewrite; a
/// marker whose end is missing bounds nothing, so the file is left alone
/// rather than guessed at. `None` when the section is already current.
fn refreshed(path: &Path) -> Option<PlannedWrite> {
    let text = std::fs::read_to_string(path).ok()?;
    let start = text.find(MARKER)?;
    let end = start + text[start..].find(END_MARKER)? + END_MARKER.len();

    let current = SECTION.trim();
    if &text[start..end] == current {
        return None;
    }

    let mut content = String::with_capacity(text.len() + current.len());
    content.push_str(&text[..start]);
    content.push_str(current);
    content.push_str(&text[end..]);

    Some(PlannedWrite {
        path: path.to_path_buf(),
        content,
        merges_existing: true,
    })
}

#[cfg(test)]
#[path = "instructions_tests.rs"]
mod tests;
