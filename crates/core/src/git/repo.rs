//! Repository reads and mutations.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::patch::{self, DiffLine, Direction, FileDiff, Hunk, LineOrigin};

/// Strip the read-only attribute from every directory in a working tree,
/// returning how many were changed.
///
/// Windows will not remove a read-only directory, which breaks any checkout
/// that has to delete one. The attribute carries no meaning for directories —
/// unlike on a file, it does not protect the contents — so clearing it is safe.
/// Files are deliberately left alone: there the attribute *is* meaningful.
///
/// A no-op off Windows, where the attribute does not exist.
#[cfg(windows)]
fn clear_readonly_directories(root: &Path) -> usize {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;

    let mut cleared = 0;
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_dir() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.file_attributes() & FILE_ATTRIBUTE_READONLY == 0 {
            continue;
        }

        let mut permissions = metadata.permissions();
        // Windows-only, where this clears FILE_ATTRIBUTE_READONLY and nothing
        // else. Clippy's warning is about Unix, where the same call would
        // widen the mode to 0o777 — hence the `cfg(windows)` on this function.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        if std::fs::set_permissions(entry.path(), permissions).is_ok() {
            cleared += 1;
        }
    }
    cleared
}

#[cfg(not(windows))]
fn clear_readonly_directories(_root: &Path) -> usize {
    0
}

/// Explain a checkout blocked by a file or directory another process holds.
///
/// Kept separate from the checkout itself so the wording is testable without
/// having to manufacture an OS-level lock.
fn locked_checkout_message(name: &str, detail: &str) -> String {
    let mut message = format!(
        "Could not switch to {name}: another program is holding a file or directory \
         open, so Git could not update it."
    );
    if !detail.is_empty() {
        message.push_str("\n\n");
        message.push_str(detail);
    }
    message.push_str(
        "\n\nRead-only directory attributes have already been cleared and the switch retried, \
         so something is genuinely holding this path open. Windows will not let a directory be \
         removed while any process has it open: the usual causes are a terminal whose current \
         directory is inside the folder, the folder being open in Explorer, or a tool watching \
         the repository. Close whatever is holding it and switch again.",
    );
    message
}

/// Which two states a diff compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ComparisonMode {
    /// Working copy against `HEAD` — everything changed since the last commit,
    /// staged or not. The default review view.
    WorkingToHead,
    /// Working copy against the index — only what is *not* yet staged.
    WorkingToIndex,
    /// Index against `HEAD` — only what *is* staged.
    IndexToHead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChange,
    Untracked,
    Conflicted,
    Unknown,
}

/// One file appearing in `git status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    /// How the file differs in the index relative to `HEAD`.
    pub staged: Option<ChangeKind>,
    /// How the working copy differs from the index.
    pub unstaged: Option<ChangeKind>,
    pub is_binary: bool,
}

impl FileChange {
    pub fn is_conflicted(&self) -> bool {
        self.staged == Some(ChangeKind::Conflicted)
            || self.unstaged == Some(ChangeKind::Conflicted)
    }
}

/// A snapshot of the working tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkingStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileChange>,
    /// True during a merge, rebase or cherry-pick.
    pub in_progress_operation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub body: Option<String>,
    pub author_name: String,
    pub author_email: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
}

/// How a merge ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MergeOutcome {
    /// The branch was already contained in HEAD; nothing to do.
    UpToDate,
    /// HEAD simply moved forward — no merge commit was needed.
    FastForward,
    /// A merge commit was created.
    Merged,
    /// Conflicts need resolving. The merge is still in progress.
    Conflicted,
}

/// The result of merging a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    pub outcome: MergeOutcome,
    /// The resulting commit, for a fast-forward or a merge commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Paths left conflicted, when `outcome` is `conflicted`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

impl MergeReport {
    fn up_to_date() -> Self {
        Self {
            outcome: MergeOutcome::UpToDate,
            commit: None,
            conflicts: Vec::new(),
        }
    }
}

/// Where a staging operation applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageTarget {
    Index,
    WorkingTree,
}

/// A git repository.
pub struct Repo {
    inner: git2::Repository,
    workdir: PathBuf,
}

impl Repo {
    /// Open the repository containing `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let inner = git2::Repository::discover(path)
            .with_context(|| format!("no git repository found at or above {}", path.display()))?;
        let workdir = inner
            .workdir()
            .context("bare repositories have no working tree to review")?
            .to_path_buf();

        Ok(Self { inner, workdir })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    pub fn status(&self) -> Result<WorkingStatus> {
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);

        let statuses = self
            .inner
            .statuses(Some(&mut options))
            .context("failed to read repository status")?;

        let mut files: Vec<FileChange> = statuses
            .iter()
            .filter_map(|entry| self.to_file_change(&entry))
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));

        let (branch, upstream, ahead, behind) = self.branch_info();

        Ok(WorkingStatus {
            branch,
            upstream,
            ahead,
            behind,
            files,
            in_progress_operation: self.in_progress_operation(),
        })
    }

    fn to_file_change(&self, entry: &git2::StatusEntry) -> Option<FileChange> {
        let status = entry.status();
        if status.is_ignored() {
            return None;
        }

        let index_delta = entry.head_to_index();
        let worktree_delta = entry.index_to_workdir();

        let path = worktree_delta
            .as_ref()
            .and_then(|d| d.new_file().path())
            .or_else(|| index_delta.as_ref().and_then(|d| d.new_file().path()))
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| entry.path().map(str::to_string))?;

        let old_path = index_delta
            .as_ref()
            .or(worktree_delta.as_ref())
            .and_then(|d| d.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|old| *old != path);

        let is_binary = worktree_delta
            .as_ref()
            .or(index_delta.as_ref())
            .is_some_and(|d| d.flags().is_binary());

        Some(FileChange {
            path,
            old_path,
            staged: staged_kind(status),
            unstaged: unstaged_kind(status),
            is_binary,
        })
    }

    fn branch_info(&self) -> (Option<String>, Option<String>, u32, u32) {
        let Ok(head) = self.inner.head() else {
            // An unborn branch: a repository with no commits yet.
            return (self.unborn_branch_name(), None, 0, 0);
        };

        let branch = head.shorthand().map(str::to_string);
        let Some(name) = branch.clone() else {
            return (branch, None, 0, 0);
        };

        let Ok(local) = self.inner.find_branch(&name, git2::BranchType::Local) else {
            return (branch, None, 0, 0);
        };
        let Ok(upstream) = local.upstream() else {
            return (branch, None, 0, 0);
        };

        let upstream_name = upstream.name().ok().flatten().map(str::to_string);

        let counts = match (head.target(), upstream.get().target()) {
            (Some(local_oid), Some(upstream_oid)) => self
                .inner
                .graph_ahead_behind(local_oid, upstream_oid)
                .unwrap_or((0, 0)),
            _ => (0, 0),
        };

        (branch, upstream_name, counts.0 as u32, counts.1 as u32)
    }

    fn unborn_branch_name(&self) -> Option<String> {
        let reference = self.inner.find_reference("HEAD").ok()?;
        let target = reference.symbolic_target()?;
        Some(target.strip_prefix("refs/heads/").unwrap_or(target).to_string())
    }

    /// Detect a merge, rebase or cherry-pick in progress.
    ///
    /// Reported so the UI can warn before an action that would be confusing
    /// mid-operation.
    fn in_progress_operation(&self) -> Option<String> {
        let git_dir = self.inner.path();
        let candidates = [
            ("MERGE_HEAD", "merge"),
            ("CHERRY_PICK_HEAD", "cherry-pick"),
            ("REVERT_HEAD", "revert"),
            ("BISECT_LOG", "bisect"),
            ("rebase-merge", "rebase"),
            ("rebase-apply", "rebase"),
        ];

        candidates
            .iter()
            .find(|(file, _)| git_dir.join(file).exists())
            .map(|(_, label)| label.to_string())
    }

    // -----------------------------------------------------------------------
    // Diffs
    // -----------------------------------------------------------------------

    /// Diff a single file in the given comparison mode.
    pub fn file_diff(&self, path: &str, mode: ComparisonMode) -> Result<FileDiff> {
        let mut options = git2::DiffOptions::new();
        options
            .pathspec(path)
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true)
            // Enough surrounding lines to orient a reviewer without turning
            // small edits into whole-file views.
            .context_lines(3);

        let diff = self.build_diff(mode, &mut options)?;
        let mut files = collect_file_diffs(&diff)?;

        // An untracked or unchanged file yields no delta; return an empty
        // diff rather than an error so the UI can still show the file.
        Ok(files.pop().unwrap_or(FileDiff {
            path: path.to_string(),
            old_path: None,
            hunks: Vec::new(),
            is_binary: false,
        }))
    }

    /// Diff every changed file in the given comparison mode.
    pub fn diff_all(&self, mode: ComparisonMode) -> Result<Vec<FileDiff>> {
        let mut options = git2::DiffOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true)
            .context_lines(3);

        let diff = self.build_diff(mode, &mut options)?;
        collect_file_diffs(&diff)
    }

    fn build_diff<'a>(
        &'a self,
        mode: ComparisonMode,
        options: &mut git2::DiffOptions,
    ) -> Result<git2::Diff<'a>> {
        let head_tree = match self.inner.head() {
            Ok(head) => Some(head.peel_to_tree().context("failed to resolve HEAD")?),
            // No commits yet: everything is new relative to an empty tree.
            Err(_) => None,
        };

        let diff = match mode {
            ComparisonMode::WorkingToHead => self
                .inner
                .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(options)),
            ComparisonMode::WorkingToIndex => {
                self.inner.diff_index_to_workdir(None, Some(options))
            }
            ComparisonMode::IndexToHead => self
                .inner
                .diff_tree_to_index(head_tree.as_ref(), None, Some(options)),
        };

        diff.context("failed to compute diff")
    }

    /// The baseline content a diff is measured against.
    ///
    /// This is what "revert" restores, so it has to match the comparison mode:
    /// reverting in the staged view restores the index, not `HEAD`.
    pub fn baseline_content(&self, path: &str, mode: ComparisonMode) -> Result<Option<String>> {
        match mode {
            ComparisonMode::WorkingToIndex => self.index_content(path),
            ComparisonMode::WorkingToHead | ComparisonMode::IndexToHead => self.head_content(path),
        }
    }

    fn head_content(&self, path: &str) -> Result<Option<String>> {
        let Ok(head) = self.inner.head() else {
            return Ok(None);
        };
        let tree = head.peel_to_tree().context("failed to resolve HEAD")?;

        let Ok(entry) = tree.get_path(Path::new(path)) else {
            // The file is new; it has no committed content.
            return Ok(None);
        };
        let blob = self.inner.find_blob(entry.id()).context("failed to read blob")?;

        Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
    }

    fn index_content(&self, path: &str) -> Result<Option<String>> {
        let index = self.inner.index().context("failed to read index")?;
        let Some(entry) = index.get_path(Path::new(path), 0) else {
            return Ok(None);
        };
        let blob = self.inner.find_blob(entry.id).context("failed to read blob")?;

        Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
    }

    /// The file's current content on disk.
    pub fn working_content(&self, path: &str) -> Result<Option<String>> {
        let full = self.workdir.join(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("failed to read {}", full.display())),
        }
    }

    /// Overwrite a file in the working tree.
    ///
    /// Used when the user edits inside the diff view.
    pub fn write_working_file(&self, path: &str, content: &str) -> Result<()> {
        let full = self.workdir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&full, content)
            .with_context(|| format!("failed to write {}", full.display()))
    }

    // -----------------------------------------------------------------------
    // Staging and reverting
    // -----------------------------------------------------------------------

    /// Stage a whole file.
    pub fn stage_file(&self, path: &str) -> Result<()> {
        let mut index = self.inner.index().context("failed to read index")?;
        let full = self.workdir.join(path);

        if full.exists() {
            index
                .add_path(Path::new(path))
                .with_context(|| format!("failed to stage {path}"))?;
        } else {
            // A deletion is staged by removing the entry, not adding it.
            index
                .remove_path(Path::new(path))
                .with_context(|| format!("failed to stage deletion of {path}"))?;
        }

        index.write().context("failed to write index")
    }

    /// Unstage a whole file, leaving the working copy untouched.
    pub fn unstage_file(&self, path: &str) -> Result<()> {
        match self.inner.head() {
            Ok(head) => {
                let commit = head.peel_to_commit().context("failed to resolve HEAD")?;
                self.inner
                    .reset_default(Some(commit.as_object()), [path])
                    .with_context(|| format!("failed to unstage {path}"))
            }
            Err(_) => {
                // Before the first commit there is nothing to reset to, so the
                // entry is simply removed from the index.
                let mut index = self.inner.index().context("failed to read index")?;
                index
                    .remove_path(Path::new(path))
                    .with_context(|| format!("failed to unstage {path}"))?;
                index.write().context("failed to write index")
            }
        }
    }

    /// Stage only the selected lines of a file.
    pub fn stage_lines(&self, path: &str, selected: &BTreeSet<u32>) -> Result<bool> {
        let diff = self.file_diff(path, ComparisonMode::WorkingToIndex)?;
        let Some(text) = patch::build_patch(&diff, selected, Direction::Forward) else {
            return Ok(false);
        };
        self.apply_patch(&text, StageTarget::Index, false)?;
        Ok(true)
    }

    /// Unstage only the selected lines of a file.
    pub fn unstage_lines(&self, path: &str, selected: &BTreeSet<u32>) -> Result<bool> {
        let diff = self.file_diff(path, ComparisonMode::IndexToHead)?;
        let Some(text) = patch::build_patch(&diff, selected, Direction::Reverse) else {
            return Ok(false);
        };
        self.apply_patch(&text, StageTarget::Index, true)?;
        Ok(true)
    }

    /// Revert selected lines in the working copy back to the baseline.
    ///
    /// This is the "undo just this line" action. The baseline depends on the
    /// mode: in the staged view the line returns to its staged state, not to
    /// `HEAD`.
    pub fn revert_lines(
        &self,
        path: &str,
        mode: ComparisonMode,
        selected: &BTreeSet<u32>,
    ) -> Result<bool> {
        let diff = self.file_diff(path, mode)?;
        let Some(text) = patch::build_patch(&diff, selected, Direction::Reverse) else {
            return Ok(false);
        };
        self.apply_patch(&text, StageTarget::WorkingTree, true)?;
        Ok(true)
    }

    /// Hand a patch to the system `git apply`.
    ///
    /// `git apply` is the only correct implementation of partial patch
    /// application; reimplementing it would mean reimplementing its fuzz,
    /// whitespace and newline handling too.
    fn apply_patch(&self, patch_text: &str, target: StageTarget, reverse: bool) -> Result<()> {
        use std::io::Write;
        use std::process::Stdio;

        let mut args = vec!["apply".to_string(), "--unidiff-zero".to_string()];
        if reverse {
            args.push("--reverse".into());
        }
        match target {
            // `--cached` changes only the index; without it the working tree
            // would be modified too, discarding the user's edits.
            StageTarget::Index => args.push("--cached".into()),
            StageTarget::WorkingTree => {}
        }

        let mut child = Command::new("git")
            .args(&args)
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to run `git apply` — is git installed and on PATH?")?;

        child
            .stdin
            .as_mut()
            .context("git apply stdin was not piped")?
            .write_all(patch_text.as_bytes())
            .context("failed to send patch to git apply")?;

        let output = child.wait_with_output().context("git apply failed")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git apply rejected the patch: {}", stderr.trim());
        }
        Ok(())
    }

    /// Discard every working-tree change to a file, restoring it from `HEAD`.
    pub fn discard_file(&self, path: &str) -> Result<()> {
        let mut builder = git2::build::CheckoutBuilder::new();
        builder.force().path(path).remove_untracked(true);

        self.inner
            .checkout_head(Some(&mut builder))
            .with_context(|| format!("failed to discard changes to {path}"))
    }

    // -----------------------------------------------------------------------
    // Commits, branches and history
    // -----------------------------------------------------------------------

    pub fn commit(&self, message: &str, amend: bool) -> Result<String> {
        if message.trim().is_empty() {
            bail!("a commit needs a message");
        }

        let signature = self
            .inner
            .signature()
            .context("git has no user.name/user.email configured")?;

        let mut index = self.inner.index().context("failed to read index")?;
        let tree_id = index.write_tree().context("failed to write tree")?;
        let tree = self.inner.find_tree(tree_id).context("failed to read tree")?;

        let head = self.inner.head().ok();
        let parent = head.as_ref().and_then(|h| h.peel_to_commit().ok());

        let oid = if amend {
            let target = parent.context("there is no commit to amend")?;
            target
                .amend(Some("HEAD"), Some(&signature), Some(&signature), None, Some(message), Some(&tree))
                .context("failed to amend commit")?
        } else {
            let parents: Vec<&git2::Commit> = parent.iter().collect();
            self.inner
                .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)
                .context("failed to create commit")?
        };

        Ok(oid.to_string())
    }

    /// Merge `name` into the current branch, the way `git merge` does.
    ///
    /// Refuses to start with modified tracked files in the way: a merge that
    /// hits a conflict leaves the working tree half-written, and untangling
    /// that from pre-existing edits is exactly the mess this app should not
    /// create. Untracked files are fine — `git merge` allows them too.
    ///
    /// A conflicted merge is **left in place**, `MERGE_HEAD` and all, so the
    /// conflicts can be resolved in the Changes tab and committed there. That
    /// mirrors the command line rather than silently aborting the merge.
    pub fn merge_branch(&self, name: &str) -> Result<MergeReport> {
        let status = self.status()?;

        if status.in_progress_operation.is_some() {
            bail!(
                "a {} is already in progress; finish or abort it before merging",
                status.in_progress_operation.unwrap_or_default()
            );
        }

        let dirty: Vec<&FileChange> = status
            .files
            .iter()
            .filter(|f| {
                f.staged.is_some() || f.unstaged.is_some_and(|k| k != ChangeKind::Untracked)
            })
            .collect();
        if !dirty.is_empty() {
            bail!(
                "commit or stash your changes before merging — {} tracked file(s) are modified, \
                 starting with {}",
                dirty.len(),
                dirty[0].path
            );
        }

        let head_name = status
            .branch
            .clone()
            .unwrap_or_else(|| "HEAD".to_string());

        let (object, reference) = self
            .inner
            .revparse_ext(name)
            .with_context(|| format!("unknown branch {name}"))?;
        let source = match &reference {
            Some(r) => self.inner.reference_to_annotated_commit(r),
            None => self.inner.find_annotated_commit(object.id()),
        }
        .with_context(|| format!("failed to resolve {name}"))?;

        if self.inner.head()?.peel_to_commit()?.id() == source.id() {
            return Ok(MergeReport::up_to_date());
        }

        let (analysis, _) = self
            .inner
            .merge_analysis(&[&source])
            .context("failed to analyse the merge")?;

        if analysis.is_up_to_date() {
            return Ok(MergeReport::up_to_date());
        }

        if analysis.is_fast_forward() {
            let commit = self
                .inner
                .find_commit(source.id())
                .context("failed to read the branch tip")?;

            self.checkout_tree_tolerating_locks(commit.as_object(), name)?;

            let mut head = self.inner.head().context("failed to resolve HEAD")?;
            head.set_target(source.id(), &format!("merge {name}: fast-forward"))
                .with_context(|| format!("failed to fast-forward to {name}"))?;

            return Ok(MergeReport {
                outcome: MergeOutcome::FastForward,
                commit: Some(source.id().to_string()),
                conflicts: Vec::new(),
            });
        }

        if !analysis.is_normal() {
            bail!("{name} cannot be merged into {head_name} automatically");
        }

        self.inner
            .merge(&[&source], None, None)
            .with_context(|| format!("failed to merge {name}"))?;

        let mut index = self.inner.index().context("failed to read index")?;

        if index.has_conflicts() {
            let conflicts = index
                .conflicts()
                .context("failed to read merge conflicts")?
                .flatten()
                .filter_map(|c| {
                    c.our
                        .or(c.their)
                        .or(c.ancestor)
                        .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();

            // Deliberately left mid-merge for the user to resolve.
            return Ok(MergeReport {
                outcome: MergeOutcome::Conflicted,
                commit: None,
                conflicts,
            });
        }

        let signature = self
            .inner
            .signature()
            .context("git has no user.name/user.email configured")?;

        let tree_id = index.write_tree().context("failed to write the merged tree")?;
        let tree = self.inner.find_tree(tree_id).context("failed to read the merged tree")?;
        let ours = self.inner.head()?.peel_to_commit().context("HEAD is not a commit")?;
        let theirs = self
            .inner
            .find_commit(source.id())
            .context("failed to read the branch tip")?;

        let message = format!("Merge branch '{name}' into {head_name}");
        let oid = self
            .inner
            .commit(Some("HEAD"), &signature, &signature, &message, &tree, &[&ours, &theirs])
            .context("failed to create the merge commit")?;

        self.inner.cleanup_state().context("failed to clear the merge state")?;

        Ok(MergeReport {
            outcome: MergeOutcome::Merged,
            commit: Some(oid.to_string()),
            conflicts: Vec::new(),
        })
    }

    /// Abandon a conflicted merge and return to the pre-merge state.
    pub fn abort_merge(&self) -> Result<()> {
        let head = self
            .inner
            .head()
            .context("failed to resolve HEAD")?
            .peel_to_commit()
            .context("HEAD is not a commit")?;

        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force();
        self.inner
            .reset(head.as_object(), git2::ResetType::Hard, Some(&mut checkout))
            .context("failed to undo the merge")?;

        self.inner.cleanup_state().context("failed to clear the merge state")
    }

    pub fn branches(&self) -> Result<Vec<Branch>> {
        let mut out = Vec::new();

        for result in self.inner.branches(None).context("failed to list branches")? {
            let (branch, kind) = result.context("failed to read branch")?;
            let Some(name) = branch.name().ok().flatten() else {
                continue;
            };

            out.push(Branch {
                name: name.to_string(),
                is_head: branch.is_head(),
                is_remote: kind == git2::BranchType::Remote,
                upstream: branch
                    .upstream()
                    .ok()
                    .and_then(|u| u.name().ok().flatten().map(str::to_string)),
            });
        }

        out.sort_by(|a, b| a.is_remote.cmp(&b.is_remote).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    pub fn create_branch(&self, name: &str, checkout: bool) -> Result<()> {
        let head = self.inner.head().context("failed to resolve HEAD")?;
        let commit = head.peel_to_commit().context("HEAD is not a commit")?;

        self.inner
            .branch(name, &commit, false)
            .with_context(|| format!("failed to create branch {name}"))?;

        if checkout {
            self.checkout_branch(name)?;
        }
        Ok(())
    }

    pub fn checkout_branch(&self, name: &str) -> Result<()> {
        let (object, reference) = self
            .inner
            .revparse_ext(name)
            .with_context(|| format!("unknown branch {name}"))?;

        self.checkout_tree_tolerating_locks(&object, name)?;

        match reference {
            Some(r) => {
                let ref_name = r.name().context("branch reference has no name")?;
                self.inner.set_head(ref_name)
            }
            // A bare revision produces a detached HEAD.
            None => self.inner.set_head_detached(object.id()),
        }
        .with_context(|| format!("failed to switch to {name}"))
    }

    /// Check out a tree, retrying briefly when the filesystem reports a lock.
    ///
    /// Windows refuses to remove a directory while *any* process has it open,
    /// and libgit2 surfaces that as a failed checkout — even when the
    /// directory only needed tidying after a file was removed from it. Tool
    /// directories are the usual victims, because the tools that own them watch
    /// them: `.claude/` while a Claude Code session is running, `.idea/`,
    /// `.vs/`, and so on.
    ///
    /// Many such locks last only a moment, so a couple of quick retries clear
    /// them. What survives that is reported with the path and what to do about
    /// it, rather than as raw libgit2 text.
    fn checkout_tree_tolerating_locks(&self, object: &git2::Object<'_>, name: &str) -> Result<()> {
        const PAUSE: std::time::Duration = std::time::Duration::from_millis(150);

        match self.inner.checkout_tree(object, None) {
            Ok(()) => return Ok(()),
            Err(e) if e.code() == git2::ErrorCode::Locked => {}
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("failed to check out {name}"))
            }
        }

        // By far the most common cause on Windows is not a lock at all: a
        // directory carrying the read-only attribute cannot be removed, and
        // `RemoveDirectory` reports that as "Access is denied". The attribute
        // is meaningless on directories — Explorer uses it to mark customised
        // folders — but tooling sets it in bulk, so a whole repository can be
        // affected and every branch switch that removes a directory fails.
        if clear_readonly_directories(&self.workdir) > 0
            && self.inner.checkout_tree(object, None).is_ok()
        {
            return Ok(());
        }

        // Otherwise something really is holding it; that is often momentary.
        std::thread::sleep(PAUSE);
        match self.inner.checkout_tree(object, None) {
            Ok(()) => Ok(()),
            Err(e) => bail!(locked_checkout_message(name, e.message())),
        }
    }

    /// Create a branch pointing at `start_point` — a local or remote branch
    /// name, or any revision — rather than at HEAD.
    pub fn create_branch_from(
        &self,
        name: &str,
        start_point: &str,
        checkout: bool,
    ) -> Result<()> {
        let (object, _) = self
            .inner
            .revparse_ext(start_point)
            .with_context(|| format!("unknown revision {start_point}"))?;
        let commit = object
            .peel_to_commit()
            .with_context(|| format!("{start_point} does not point at a commit"))?;

        self.inner
            .branch(name, &commit, false)
            .with_context(|| format!("failed to create branch {name}"))?;

        if checkout {
            self.checkout_branch(name)?;
        }
        Ok(())
    }

    /// Check out a remote-tracking branch the way `git switch` does: create
    /// the local branch (remote prefix dropped) pointing at the remote tip
    /// with its upstream set, then switch to it. If the local branch already
    /// exists, just switch — never clobber local work.
    pub fn checkout_remote_branch(&self, name: &str) -> Result<()> {
        let local_name = name.split_once('/').map_or(name, |(_, rest)| rest);

        if self.inner.find_branch(local_name, git2::BranchType::Local).is_ok() {
            return self.checkout_branch(local_name);
        }

        let remote = self
            .inner
            .find_branch(name, git2::BranchType::Remote)
            .with_context(|| format!("unknown remote branch {name}"))?;
        let commit = remote
            .get()
            .peel_to_commit()
            .with_context(|| format!("{name} does not point at a commit"))?;

        let mut local = self
            .inner
            .branch(local_name, &commit, false)
            .with_context(|| format!("failed to create branch {local_name}"))?;
        local
            .set_upstream(Some(name))
            .with_context(|| format!("failed to track {name}"))?;

        self.checkout_branch(local_name)
    }

    pub fn delete_branch(&self, name: &str) -> Result<()> {
        let mut branch = self
            .inner
            .find_branch(name, git2::BranchType::Local)
            .with_context(|| format!("unknown branch {name}"))?;

        if branch.is_head() {
            bail!("cannot delete {name} while it is checked out");
        }
        branch.delete().with_context(|| format!("failed to delete {name}"))
    }

    pub fn history(&self, limit: usize) -> Result<Vec<Commit>> {
        let mut walk = self.inner.revwalk().context("failed to walk history")?;
        if walk.push_head().is_err() {
            // No commits yet.
            return Ok(Vec::new());
        }
        // TIME alone is not enough: commits made within the same second tie,
        // and the order among them becomes arbitrary — which happens routinely
        // with amends, rebases and scripted commits. TOPOLOGICAL breaks the
        // tie correctly by never listing a parent before its child.
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
            .context("failed to sort history")?;

        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let oid = oid.context("failed to read commit id")?;
            let commit = self.inner.find_commit(oid).context("failed to read commit")?;
            let author = commit.author();

            out.push(Commit {
                id: oid.to_string(),
                short_id: oid.to_string()[..7.min(oid.to_string().len())].to_string(),
                summary: commit.summary().unwrap_or_default().to_string(),
                body: commit.body().map(str::to_string),
                author_name: author.name().unwrap_or_default().to_string(),
                author_email: author.email().unwrap_or_default().to_string(),
                time: commit.time().seconds(),
            });
        }

        Ok(out)
    }

    /// Diff the changes introduced by one commit.
    pub fn commit_diff(&self, id: &str) -> Result<Vec<FileDiff>> {
        let oid = git2::Oid::from_str(id).with_context(|| format!("invalid commit id {id}"))?;
        let commit = self.inner.find_commit(oid).context("unknown commit")?;
        let tree = commit.tree().context("failed to read commit tree")?;
        let parent = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let mut options = git2::DiffOptions::new();
        options.context_lines(3);

        let diff = self
            .inner
            .diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut options))
            .context("failed to diff commit")?;

        collect_file_diffs(&diff)
    }

    pub fn stash_save(&mut self, message: &str) -> Result<()> {
        let signature = self
            .inner
            .signature()
            .context("git has no user.name/user.email configured")?;

        self.inner
            .stash_save(&signature, message, Some(git2::StashFlags::INCLUDE_UNTRACKED))
            .context("failed to stash changes")?;
        Ok(())
    }

    pub fn stash_pop(&mut self) -> Result<()> {
        self.inner
            .stash_pop(0, None)
            .context("failed to restore the most recent stash")
    }

    // -----------------------------------------------------------------------
    // Network operations
    // -----------------------------------------------------------------------

    /// Build the command line for a network operation.
    ///
    /// Returned rather than executed so it can go through the process
    /// supervisor like everything else, streaming progress to the console.
    pub fn network_command(&self, operation: NetworkOperation) -> crate::model::Invocation {
        let args = match operation {
            NetworkOperation::Fetch => vec!["fetch".to_string(), "--prune".to_string()],
            NetworkOperation::Pull => vec!["pull".to_string(), "--ff-only".to_string()],
            NetworkOperation::Push => vec!["push".to_string()],
            // A branch with no upstream needs one established, which plain
            // `git push` refuses to do.
            NetworkOperation::PushSetUpstream(branch) => {
                vec!["push".to_string(), "-u".to_string(), "origin".to_string(), branch]
            }
        };

        crate::model::Invocation {
            program: "git".into(),
            args,
            cwd: self.workdir.clone(),
            // Deliberately empty: the system git reads the user's credential
            // helpers, SSH agent and keychain from their own environment.
            env: Default::default(),
            report: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkOperation {
    Fetch,
    Pull,
    Push,
    PushSetUpstream(String),
}

/// Turn a libgit2 diff into our own structures.
fn collect_file_diffs(diff: &git2::Diff) -> Result<Vec<FileDiff>> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut next_index: u32 = 0;

    diff.print(git2::DiffFormat::Patch, |delta, hunk, line| {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        // A new file starts a new entry and restarts line indexing.
        if files.last().map(|f| f.path.as_str()) != Some(path.as_str()) {
            let old_path = delta
                .old_file()
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|old| *old != path);

            files.push(FileDiff {
                path,
                old_path,
                hunks: Vec::new(),
                is_binary: delta.flags().is_binary(),
            });
            next_index = 0;
        }

        let file = files.last_mut().expect("just pushed");

        let Some(hunk) = hunk else {
            // File-level header lines carry no hunk; nothing to record.
            return true;
        };

        // Start a new hunk when libgit2 moves on to the next one.
        let is_new_hunk = file
            .hunks
            .last()
            .is_none_or(|h| h.old_start != hunk.old_start() || h.new_start != hunk.new_start());
        if is_new_hunk {
            file.hunks.push(Hunk {
                old_start: hunk.old_start(),
                old_lines: hunk.old_lines(),
                new_start: hunk.new_start(),
                new_lines: hunk.new_lines(),
                header: String::from_utf8_lossy(hunk.header())
                    .split("@@")
                    .nth(2)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                lines: Vec::new(),
            });
        }

        let origin = match line.origin() {
            '+' => LineOrigin::Addition,
            '-' => LineOrigin::Deletion,
            ' ' => LineOrigin::Context,
            // Hunk headers and newline markers are not content lines.
            _ => return true,
        };

        let content = String::from_utf8_lossy(line.content())
            .trim_end_matches('\n')
            .to_string();

        let current = file.hunks.last_mut().expect("just pushed");
        current.lines.push(DiffLine {
            index: next_index,
            origin,
            content,
            old_lineno: line.old_lineno(),
            new_lineno: line.new_lineno(),
            no_newline: false,
        });
        next_index += 1;

        true
    })
    .context("failed to read diff")?;

    Ok(files)
}

fn staged_kind(status: git2::Status) -> Option<ChangeKind> {
    if status.is_conflicted() {
        return Some(ChangeKind::Conflicted);
    }
    if status.is_index_new() {
        Some(ChangeKind::Added)
    } else if status.is_index_modified() {
        Some(ChangeKind::Modified)
    } else if status.is_index_deleted() {
        Some(ChangeKind::Deleted)
    } else if status.is_index_renamed() {
        Some(ChangeKind::Renamed)
    } else if status.is_index_typechange() {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

fn unstaged_kind(status: git2::Status) -> Option<ChangeKind> {
    if status.is_conflicted() {
        return Some(ChangeKind::Conflicted);
    }
    if status.is_wt_new() {
        Some(ChangeKind::Untracked)
    } else if status.is_wt_modified() {
        Some(ChangeKind::Modified)
    } else if status.is_wt_deleted() {
        Some(ChangeKind::Deleted)
    } else if status.is_wt_renamed() {
        Some(ChangeKind::Renamed)
    } else if status.is_wt_typechange() {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The raw libgit2 text a held directory produces on Windows.
    const LIBGIT2_DETAIL: &str =
        "failed rmdir - 'C:/repo/.claude/commands/' is locked: Access is denied.";

    #[test]
    fn a_locked_checkout_explains_itself() {
        // The raw error names neither the cause nor a way out, which is the
        // whole problem with reporting it verbatim.
        let message = locked_checkout_message("users/anthony/docs", LIBGIT2_DETAIL);

        assert!(message.contains("users/anthony/docs"), "must name the branch");
        assert!(message.contains(".claude/commands"), "must keep the offending path");
        assert!(
            message.contains("already been cleared"),
            "must say the read-only cause was ruled out, or the user re-investigates it"
        );
        assert!(message.contains("Close whatever"), "must say what to do");
    }

    #[test]
    #[cfg(windows)]
    fn read_only_directories_are_cleared_but_read_only_files_are_not() {
        use std::os::windows::fs::MetadataExt;

        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("outer").join("inner");
        std::fs::create_dir_all(&nested).unwrap();

        let file = nested.join("locked.txt");
        std::fs::write(&file, "x").unwrap();

        let read_only = |path: &Path| {
            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(path, permissions).unwrap();
        };
        read_only(&nested);
        read_only(&temp.path().join("outer"));
        read_only(&file);

        let cleared = clear_readonly_directories(temp.path());
        assert!(cleared >= 2, "both read-only directories should be cleared, got {cleared}");

        const READONLY: u32 = 0x1;
        for dir in [temp.path().join("outer"), nested.clone()] {
            let attributes = std::fs::metadata(&dir).unwrap().file_attributes();
            assert_eq!(attributes & READONLY, 0, "{} should be writable", dir.display());
        }

        // A read-only *file* is a deliberate marker and must survive.
        assert!(
            std::fs::metadata(&file).unwrap().permissions().readonly(),
            "file attributes must not be touched"
        );

        // tempfile cannot clean up what it cannot write to.
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&file, permissions).unwrap();
    }

    #[test]
    fn clearing_attributes_on_a_missing_directory_is_harmless() {
        assert_eq!(clear_readonly_directories(Path::new("/nonexistent/repo")), 0);
    }

    #[test]
    fn a_locked_checkout_reads_sensibly_without_any_detail() {
        let message = locked_checkout_message("main", "");

        assert!(message.starts_with("Could not switch to main:"));
        assert!(!message.contains("\n\n\n"), "an absent detail must not leave a gap");
    }
}
