//! The `post-commit` hook that persists durable intent for external commits.
//!
//! The in-app commit writes the durable-why note itself, but a commit made from
//! the command line — including one an agent makes — goes through the system
//! `git`, which libgit2's in-process commit never touches. `git commit` is the
//! one gate every such commit shares, so enabling intent capture also installs a
//! `post-commit` hook that re-invokes this application to write the note for the
//! commit that was just made.
//!
//! Same discipline as the rejected-change guard ([`super::guard`]) and for the
//! same reason — this is a file the user may already have written: the block is
//! bounded by markers, a re-install rewrites only what is between them, and an
//! existing script is appended to rather than replaced.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::providers::hooks_json::MARKER;
use super::providers::PlannedWrite;

/// Opens the block, and how a previously installed copy is recognised.
pub const BEGIN: &str = "# >>> code-basics: durable-why >>>";

/// Closes the block, bounding what a re-install may rewrite.
pub const END: &str = "# <<< code-basics: durable-why <<<";

/// The hook body for a workspace, invoking this application to record the note.
///
/// POSIX `sh` only — Git for Windows runs hooks through its bundled shell. The
/// command is redirected and `|| true`'d so it can never disturb git's output
/// or fail the commit: persisting the note is bookkeeping, not a gate.
pub fn block(exe: &str, root: &Path) -> String {
    format!(
        r#"{BEGIN}
# Persist the durable "why" note for the commit just made, so the reason
# survives into history. Installed by code-basics when intent capture was
# enabled here. Best-effort: it never blocks or fails the commit.
"{exe}" record-intent --{MARKER} --event PostCommit --workspace "{root}" >/dev/null 2>&1 || true
{END}
"#,
        root = root.display()
    )
}

/// This application's path, for baking into the hook. Falls back to a bare name
/// resolved on `PATH` when the current exe cannot be determined.
fn current_exe() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "code-basics".to_string())
}

/// The `post-commit` hook for a workspace, honouring `core.hooksPath`, or
/// `None` when it is not a git repository at all.
pub fn hook_path(root: &Path) -> Option<PathBuf> {
    crate::git::Repo::open(root)
        .ok()
        .map(|repo| repo.hooks_dir().join("post-commit"))
}

/// What installing the hook into one file would write, or `None` when nothing
/// needs to change.
pub fn plan_for(hook: &Path, exe: &str, root: &Path) -> Option<PlannedWrite> {
    let Ok(text) = std::fs::read_to_string(hook) else {
        return Some(PlannedWrite {
            path: hook.to_path_buf(),
            content: format!("#!/bin/sh\n\n{}", block(exe, root)),
            merges_existing: false,
        });
    };

    if text.contains(BEGIN) {
        return refreshed(hook, &text, exe, root);
    }

    let mut content = text;
    while content.ends_with('\n') {
        content.pop();
    }
    if !content.is_empty() {
        content.push_str("\n\n");
    }
    content.push_str(&block(exe, root));

    Some(PlannedWrite {
        path: hook.to_path_buf(),
        content,
        merges_existing: true,
    })
}

/// Replace the marked span with the current hook body.
///
/// A block whose end never arrives bounds nothing, so the file is left alone
/// rather than guessed at — the same rule the guard follows.
fn refreshed(hook: &Path, text: &str, exe: &str, root: &Path) -> Option<PlannedWrite> {
    let start = text.find(BEGIN)?;
    let end = start + text[start..].find(END)? + END.len();

    let current = block(exe, root);
    let current = current.trim_end();
    if &text[start..end] == current {
        return None;
    }

    Some(PlannedWrite {
        path: hook.to_path_buf(),
        content: format!("{}{}{}", &text[..start], current, &text[end..]),
        merges_existing: true,
    })
}

/// What installing the hook into a workspace would write.
pub fn planned_write(root: &Path) -> Option<PlannedWrite> {
    let exe = current_exe();
    hook_path(root).and_then(|hook| plan_for(&hook, &exe, root))
}

/// Is the hook already installed in this file?
pub fn is_installed(hook: &Path) -> bool {
    std::fs::read_to_string(hook).is_ok_and(|text| text.contains(BEGIN) && text.contains(END))
}

/// Make the hook executable, which git requires on unix. Reuses the guard's
/// implementation — the requirement is identical.
pub fn ensure_executable(hook: &Path) -> Result<()> {
    super::guard::ensure_executable(hook)
}

/// The hook with our block taken out, or `None` when it was never there.
pub fn plan_removal(hook: &Path) -> Result<Option<String>> {
    if !hook.exists() {
        return Ok(None);
    }

    let text = std::fs::read_to_string(hook)
        .with_context(|| format!("failed to read {}", hook.display()))?;

    let Some(start) = text.find(BEGIN) else {
        return Ok(None);
    };
    let Some(offset) = text[start..].find(END) else {
        return Ok(None);
    };

    let mut end = start + offset + END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }

    let mut head = text[..start].to_string();
    while head.ends_with('\n') {
        head.pop();
    }
    if !head.is_empty() {
        head.push('\n');
    }
    head.push_str(&text[end..]);

    Ok(Some(head))
}

#[cfg(test)]
#[path = "whyhook_tests.rs"]
mod tests;
