//! An on-demand, isolated checkout of a commit to build and test against.
//!
//! To compare the working tree against HEAD *behaviorally*, HEAD's code has to
//! be built and run somewhere that is not the user's working tree — otherwise
//! the comparison would trample the very changes it is measuring. Nothing in
//! the codebase materialised a whole tree on disk before this; every existing
//! checkout mutates the single working tree in place.
//!
//! # Why shell out to `git worktree`
//!
//! A worktree is a checkout *with its own `.git` gitlink and index*. git2's
//! `checkout_tree(target_dir)` writes the files but registers no worktree, so
//! `dotnet`/`npm` and any nested `git` run inside would see a detached,
//! half-initialised tree. `git worktree add --detach <dir> <oid>` produces a
//! real, tool-safe checkout in one call — squarely the class of operation the
//! git module already shells out for (network ops, `git apply`; see
//! [`crate::git`]).
//!
//! # Teardown never hides a result
//!
//! The checkout lives under `.code-basics/behavioral/base/<oid>` — inside
//! `SKIP_DIRS`, so it never pollutes scans or the symbol index, and inside the
//! gitignored config tree. [`BaselineWorktree`] tears itself down on drop so a
//! panic or early return can't leak one, but a *successful* run keeps it
//! (keyed by oid) so the next run at the same HEAD skips both the `worktree
//! add` and the rebuild. Whatever teardown cannot remove (a Windows lock, a
//! read-only directory) becomes a warning, never a hard error that would hide
//! a report the run already produced.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::config;
use crate::git::repo::clear_readonly_directories;

const TEARDOWN_PAUSE: Duration = Duration::from_millis(150);

/// The directory holding every cached baseline checkout.
fn base_root(repo_root: &Path) -> PathBuf {
    config::config_dir(repo_root)
        .join("behavioral")
        .join("base")
}

/// A short, filesystem-friendly slice of a full hex oid.
fn short_oid(oid: &str) -> String {
    oid.chars().take(12).collect()
}

/// Does `dir` look like a live git worktree (a `.git` gitlink file present)?
fn is_valid_worktree(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Options for [`BaselineWorktree::create`].
pub struct WorktreeOptions {
    /// Reuse an existing valid checkout at this oid instead of re-adding, and
    /// remove checkouts left over from *other* oids. Default `true`.
    pub cache_by_oid: bool,
}

impl Default for WorktreeOptions {
    fn default() -> Self {
        Self { cache_by_oid: true }
    }
}

/// A checkout of one commit in its own directory, cleaned up on drop unless
/// kept as a cache.
pub struct BaselineWorktree {
    path: PathBuf,
    repo_root: PathBuf,
    head_oid: String,
    /// This checkout already existed and was reused, so it is a cache hit.
    adopted: bool,
    /// Whether the directory should survive drop — a cache hit, or a fresh
    /// build a caller marked good with [`BaselineWorktree::keep_for_reuse`].
    keep: bool,
    torn_down: bool,
}

impl BaselineWorktree {
    /// Check out `head_oid` under `.code-basics/behavioral/base/<oid>`,
    /// reusing an existing checkout when one is present and caching is on.
    pub fn create(repo_root: &Path, head_oid: &str, opts: &WorktreeOptions) -> Result<Self> {
        let root = base_root(repo_root);
        let dir = root.join(short_oid(head_oid));

        if opts.cache_by_oid {
            // Keep disk bounded to (roughly) one baseline: drop checkouts from
            // other commits. Best-effort — a failure here is not fatal.
            sweep_other_oids(repo_root, &root, &short_oid(head_oid));
        }

        let adopt = opts.cache_by_oid && is_valid_worktree(&dir);
        if !adopt {
            if dir.exists() {
                // A stale or half-written directory at this path: clear it so
                // `worktree add` has a clean target.
                let _ = teardown(repo_root, &dir);
            }
            std::fs::create_dir_all(&root)
                .with_context(|| format!("could not create {}", root.display()))?;
            worktree_add(repo_root, &dir, head_oid)?;
        }

        Ok(Self {
            path: dir,
            repo_root: repo_root.to_path_buf(),
            head_oid: head_oid.to_string(),
            adopted: adopt,
            keep: adopt,
            torn_down: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn head_oid(&self) -> &str {
        &self.head_oid
    }

    /// Was an existing checkout reused (rather than freshly added)?
    pub fn adopted(&self) -> bool {
        self.adopted
    }

    /// Keep this checkout after drop so a later run at the same HEAD reuses it.
    /// Call only once the run against it succeeded.
    pub fn keep_for_reuse(&mut self) {
        self.keep = true;
    }

    /// Tear the checkout down now (unless it is being kept) and return any
    /// residue as warnings. Idempotent.
    pub fn finish(mut self) -> Vec<String> {
        self.run_teardown()
    }

    fn run_teardown(&mut self) -> Vec<String> {
        if self.torn_down || self.keep {
            self.torn_down = true;
            return Vec::new();
        }
        self.torn_down = true;
        teardown(&self.repo_root, &self.path)
    }
}

impl Drop for BaselineWorktree {
    fn drop(&mut self) {
        // Belt-and-braces for a panic or early return; never propagates.
        let _ = self.run_teardown();
    }
}

/// `git -C <repo_root> worktree add --detach <dir> <oid>`.
fn worktree_add(repo_root: &Path, dir: &Path, oid: &str) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_root)
        .args(["worktree", "add", "--detach"])
        .arg(dir)
        .arg(oid);
    #[cfg(windows)]
    crate::process::no_window(&mut cmd);
    let output = cmd
        .output()
        .context("failed to run `git worktree add` (is git on PATH?)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{}",
            describe_process_failure(
                &format!("git worktree add for {oid}"),
                output.status.code(),
                &stderr,
            )
        );
    }
    Ok(())
}

/// Format a spawned command's failure, surfacing the exit code when the child
/// wrote nothing.
///
/// git normally explains itself on stderr, but a child that dies *before it can
/// initialise* writes nothing at all — and then a bare "failed:" is the only
/// trace, which is impossible to diagnose. The specific case seen in the wild
/// on Windows is `STATUS_DLL_INIT_FAILED` (`0xC0000142`, surfaced as the exit
/// code `-1073741502`): the process could not start, usually because the
/// interactive window station's desktop heap is exhausted after long uptime or
/// many spawned processes, or an antivirus hook blocked process creation.
/// Neither is a problem with the diff, so the message says so.
fn describe_process_failure(what: &str, code: Option<i32>, stderr: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return format!("{what} failed: {stderr}");
    }
    match code {
        None => format!("{what} failed (terminated by a signal) with no output"),
        Some(code) => match windows_status_hint(code) {
            Some(hint) => format!(
                "{what} failed with exit code {code} ({:#010x}) and no output — {hint}",
                code as u32
            ),
            None => format!("{what} failed with exit code {code} and no output"),
        },
    }
}

/// A human explanation for the Windows NTSTATUS codes a failed-to-start child
/// reports as its exit code, or `None` for anything not specifically known.
fn windows_status_hint(code: i32) -> Option<&'static str> {
    match code as u32 {
        0xC000_0142 => Some(
            "the child process could not initialise (STATUS_DLL_INIT_FAILED). This is usually \
             Windows desktop-heap exhaustion after long uptime or many spawned processes, or an \
             antivirus hook blocking process creation — not a problem with the diff. Restart the \
             app to free the desktop heap, and reboot if it recurs.",
        ),
        _ => None,
    }
}

/// Remove a baseline checkout, tolerating the Windows locks / read-only
/// directories the in-place checkout code already handles. Idempotent: a
/// missing directory is success. Returns residual warnings, never an error —
/// a produced report must not be lost to a teardown failure.
pub fn teardown(repo_root: &Path, dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();

    if !dir.exists() {
        // Nothing on disk, but the admin entry might linger.
        run_prune(repo_root);
        return warnings;
    }

    if worktree_remove(repo_root, dir) {
        run_prune(repo_root);
        return warnings;
    }

    // The usual Windows cause is a read-only directory attribute, not a real
    // lock. Clear it and retry once, then pause and try a manual removal.
    clear_readonly_directories(dir);
    if worktree_remove(repo_root, dir) {
        run_prune(repo_root);
        return warnings;
    }

    std::thread::sleep(TEARDOWN_PAUSE);
    if let Err(e) = std::fs::remove_dir_all(dir) {
        if dir.exists() {
            warnings.push(format!(
                "could not remove baseline checkout {}: {e}. Close anything holding it \
                 open (a terminal inside it, Explorer, a file watcher) and it will be \
                 cleaned up on the next run.",
                dir.display()
            ));
        }
    }
    // Drop the admin entry for whatever we removed by hand.
    run_prune(repo_root);
    warnings
}

/// `git worktree remove --force <dir>`; true on success.
fn worktree_remove(repo_root: &Path, dir: &Path) -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove", "--force"])
        .arg(dir);
    #[cfg(windows)]
    crate::process::no_window(&mut cmd);
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

fn run_prune(repo_root: &Path) {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_root).args(["worktree", "prune"]);
    #[cfg(windows)]
    crate::process::no_window(&mut cmd);
    let _ = cmd.output();
}

/// Remove every baseline checkout whose directory name is not `keep_oid`.
fn sweep_other_oids(repo_root: &Path, root: &Path, keep_oid: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy() == keep_oid {
            continue;
        }
        if entry.path().is_dir() {
            let _ = teardown(repo_root, &entry.path());
        }
    }
}

/// Remove the whole `behavioral/` cache directory (the `behavioral_clear`
/// maintenance path). Best-effort; returns warnings for anything that survived.
pub fn clear_all(repo_root: &Path) -> Vec<String> {
    let root = base_root(repo_root);
    let mut warnings = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                warnings.extend(teardown(repo_root, &entry.path()));
            }
        }
    }
    // The parent `behavioral/` dir holds only these; remove it if now empty.
    let behavioral = config::config_dir(repo_root).join("behavioral");
    let _ = std::fs::remove_dir_all(&behavioral);
    warnings
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
