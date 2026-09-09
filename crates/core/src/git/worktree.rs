//! Creating a **user** git worktree on a new branch, to open in its own project
//! tab.
//!
//! This is deliberately separate from `behavioral::worktree`, which checks out a
//! detached HEAD into a throwaway, gitignored directory for diffing and sweeps
//! it away on drop. A worktree made here is the opposite: a durable, named
//! branch the user works in, living **outside** the repository (a sibling
//! directory) so it is never scanned, gitignored, or mistaken for project
//! output.
//!
//! Like the rest of this module's mutating-with-checkout work, the checkout is
//! delegated to the system `git` (`git worktree add`) rather than reimplemented
//! on libgit2 — git already gets the working-tree materialisation, the branch
//! creation and the admin bookkeeping right on every platform.
//!
//! The decisions that do not need a process — validating the branch name and
//! choosing the default directory — are pure functions here so they are tested
//! headlessly.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Why a requested branch name cannot be used, or `Ok` if it is usable.
///
/// A friendly pre-check for the cases a user hits by accident (empty, spaces),
/// so the error names the problem rather than surfacing git's terse
/// `check-ref-format` wording. git still has the final say — anything this
/// misses is caught when `git worktree add` runs — so this rejects only what is
/// unambiguously wrong and never tries to be the full ref-name grammar.
pub fn validate_branch_name(name: &str) -> std::result::Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Enter a branch name.".into());
    }
    if trimmed != name {
        return Err("The branch name has leading or trailing whitespace.".into());
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err("A branch name cannot contain spaces.".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("A branch name cannot contain control characters.".into());
    }
    // The characters git's check-ref-format always refuses, plus the two-char
    // sequences. `/` is allowed (a namespaced branch), but not doubled or at an
    // edge.
    const BANNED: &[char] = &['~', '^', ':', '?', '*', '[', '\\', '\x7f'];
    if let Some(bad) = name.chars().find(|c| BANNED.contains(c)) {
        return Err(format!("A branch name cannot contain '{bad}'."));
    }
    if name.starts_with('-') || name.starts_with('/') || name.ends_with('/') {
        return Err("A branch name cannot start with '-' or begin or end with '/'.".into());
    }
    if name.contains("..") || name.contains("//") || name.contains("@{") || name.ends_with(".lock")
    {
        return Err(
            "The branch name has an invalid sequence ('..', '//', '@{' or '.lock').".into(),
        );
    }
    Ok(())
}

/// Turn a branch name into a single filesystem directory segment: a namespaced
/// branch (`feature/login`) must not become nested directories, so separators
/// collapse to `-`.
pub fn sanitize_dir_segment(name: &str) -> String {
    name.replace(['/', '\\'], "-")
}

/// The default directory for a worktree: a sibling of the repository named
/// `<repo>.worktrees/<sanitized branch>`. A sibling rather than a child so the
/// worktree is never inside the repository being scanned.
///
/// Falls back to placing `.worktrees` inside the repo root only when it has no
/// parent (a drive root) — an unusual case, but returning a path is better than
/// failing here; the caller still checks the final path before creating it.
pub fn default_worktree_dir(repo_root: &Path, name: &str) -> PathBuf {
    let segment = sanitize_dir_segment(name);
    let repo_name = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    let container = format!("{repo_name}.worktrees");
    match repo_root.parent() {
        Some(parent) => parent.join(container).join(segment),
        None => repo_root.join(".worktrees").join(segment),
    }
}

/// Create a new worktree on a new branch and return the directory it lives in.
///
/// `base` is the start point for the new branch (a branch name or any revision);
/// `None` uses the repository's current `HEAD`. `dir` is where the worktree is
/// checked out; `None` uses [`default_worktree_dir`].
///
/// Runs `git -C <repo_root> worktree add -b <name> <dir> [<base>]`. The `-b`
/// makes the branch, so an existing branch of that name is refused by git (a
/// real conflict worth surfacing, not worked around). Refuses up front if the
/// target directory already exists and is non-empty, since `git worktree add`
/// would fail partway and leave a confusing half-state.
pub fn add_worktree(
    repo_root: &Path,
    name: &str,
    base: Option<&str>,
    dir: Option<&Path>,
) -> Result<PathBuf> {
    if let Err(reason) = validate_branch_name(name) {
        bail!("{reason}");
    }

    let target = match dir {
        Some(d) => d.to_path_buf(),
        None => default_worktree_dir(repo_root, name),
    };

    if target.exists()
        && target
            .read_dir()
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(true)
    {
        bail!(
            "The folder {} already exists and is not empty.",
            target.display()
        );
    }

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_root)
        .args(["worktree", "add", "-b", name])
        .arg(&target);
    if let Some(base) = base {
        cmd.arg(base);
    }
    #[cfg(windows)]
    crate::process::no_window(&mut cmd);

    let output = cmd
        .output()
        .context("failed to run `git worktree add` (is git on PATH?)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!(
                "git worktree add failed with exit code {}",
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into())
            );
        }
        bail!("{stderr}");
    }

    Ok(target)
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod worktree_tests;
