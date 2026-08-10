//! The pre-commit guard that keeps rejection notes out of history.
//!
//! A rejection note ([`super::reject`]) is a work item written into the source
//! file. Left in place it is worse than nothing: it survives into a commit,
//! then into review, and eventually reads as ordinary commentary that nobody
//! dares delete. So enabling intent capture also installs a `pre-commit` hook
//! that refuses to commit a file still carrying one.
//!
//! # Why a git hook rather than a lint rule
//!
//! The marker can land in any language, in any repository the user opens.
//! A lint rule would have to be configured per project and per toolchain, and
//! would not exist at all in a repository with no linter. `git commit` is the
//! one gate every one of those repositories shares.
//!
//! The same discipline as hook configuration applies, for the same reason —
//! this is a file the user may already have written: the block is bounded by
//! markers, a re-install rewrites only what is between them, and an existing
//! script is appended to rather than replaced.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::providers::PlannedWrite;

/// Opens the block, and how a previously installed copy is recognised.
pub const BEGIN: &str = "# >>> code-basics: rejected-change guard >>>";

/// Closes the block, bounding what a re-install may rewrite.
pub const END: &str = "# <<< code-basics: rejected-change guard <<<";

/// The guard, as it appears inside a `pre-commit` hook.
///
/// POSIX `sh` only — Git for Windows runs hooks through its bundled shell, and
/// `/bin/sh` elsewhere may well be dash, so no bashisms (`read -d`, arrays,
/// `[[`). `IFS` is narrowed to newlines rather than using the default split so
/// that a path containing spaces is still checked as one path.
pub fn block() -> String {
    // Two separate defences against the guard flagging innocent files.
    //
    // The token is assembled from two string literals at runtime, so this
    // script never contains it whole — with `core.hooksPath` set to a tracked
    // directory the hook is itself a committed file, and a literal copy would
    // make the guard flag itself on every commit that touched it.
    //
    // And what is matched is the *head line* of a note — the token followed by
    // a date — not the bare token. Source that merely mentions the token has
    // to stay committable: `reject.rs` defines it, the tests assert on it and
    // the documentation explains it.
    format!(
        r#"{BEGIN}
# Refuse to commit a change that was rejected during review and still carries
# its note. Installed by code-basics when intent capture was enabled here.
# Remove this block, or set CB_ALLOW_REJECTED=1, to commit anyway.
if [ -z "$CB_ALLOW_REJECTED" ]; then
    # Assembled rather than written out, so this file does not contain the
    # token it searches for — see guard.rs.
    cb_marker="AI-""REJECTED"
    # The head line of a note, not the bare token: source that only mentions
    # the token stays committable.
    cb_pattern="$cb_marker [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]"
    cb_flagged=""
    cb_saved_ifs="$IFS"
    IFS='
'
    for cb_path in $(git diff --cached --name-only --diff-filter=ACM); do
        # The staged blob, not the diff: a note already in HEAD still has to
        # block the commit that carries the file forward.
        if git show ":$cb_path" 2>/dev/null | grep -q "$cb_pattern"; then
            cb_flagged="$cb_flagged  $cb_path
"
        fi
    done
    IFS="$cb_saved_ifs"

    if [ -n "$cb_flagged" ]; then
        echo "code-basics: these staged files still carry an unresolved $cb_marker note:" >&2
        printf '%s' "$cb_flagged" >&2
        echo "Fix the code and delete the note, or commit with CB_ALLOW_REJECTED=1." >&2
        exit 1
    fi
fi
{END}
"#
    )
}

/// The `pre-commit` hook for a workspace, or `None` when it is not a git
/// repository at all.
///
/// `core.hooksPath` is honoured, so a repository that keeps its hooks in a
/// tracked directory gets the guard where git will actually run it.
pub fn hook_path(root: &Path) -> Option<PathBuf> {
    crate::git::Repo::open(root)
        .ok()
        .map(|repo| repo.hooks_dir().join("pre-commit"))
}

/// What installing the guard into one hook file would write, or `None` when
/// nothing needs to change.
pub fn plan_for(hook: &Path) -> Option<PlannedWrite> {
    let Ok(text) = std::fs::read_to_string(hook) else {
        return Some(PlannedWrite {
            path: hook.to_path_buf(),
            content: format!("#!/bin/sh\n\n{}", block()),
            merges_existing: false,
        });
    };

    if text.contains(BEGIN) {
        return refreshed(hook, &text);
    }

    let mut content = text;
    while content.ends_with('\n') {
        content.pop();
    }
    if !content.is_empty() {
        content.push_str("\n\n");
    }
    content.push_str(&block());

    Some(PlannedWrite {
        path: hook.to_path_buf(),
        content,
        merges_existing: true,
    })
}

/// Replace the marked span with the current guard.
///
/// A block whose end never arrives bounds nothing, so the file is left alone
/// rather than guessed at — the same rule the instruction file follows.
fn refreshed(hook: &Path, text: &str) -> Option<PlannedWrite> {
    let start = text.find(BEGIN)?;
    let end = start + text[start..].find(END)? + END.len();

    let current = block();
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

/// What installing the guard into a workspace would write.
pub fn planned_write(root: &Path) -> Option<PlannedWrite> {
    hook_path(root).and_then(|hook| plan_for(&hook))
}

/// Is the guard already in this hook?
pub fn is_installed(hook: &Path) -> bool {
    std::fs::read_to_string(hook).is_ok_and(|text| text.contains(BEGIN) && text.contains(END))
}

/// The hook with the guard taken out, or `None` when it was never there.
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

/// Make the hook executable, which git requires on unix.
///
/// Deliberately separate from the install plan: the mode is not something the
/// user needs to preview, and on Windows there is nothing to set.
pub fn ensure_executable(hook: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(hook)
            .with_context(|| format!("failed to read {}", hook.display()))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(hook, permissions)
            .with_context(|| format!("failed to make {} executable", hook.display()))?;
    }

    #[cfg(not(unix))]
    let _ = hook;

    Ok(())
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
