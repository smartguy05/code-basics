//! End-to-end git tests against real repositories on disk.
//!
//! These deliberately go through the actual `git` binary and libgit2 rather
//! than mocking either: the whole point of the patch code is that `git apply`
//! accepts what it produces, and only a real repository can confirm that.
//! Every assertion checks observable state — file contents and `git status` —
//! rather than internal structures.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use cb_core::git::{ComparisonMode, Repo};

/// Create a repository with a deterministic identity and one initial commit.
fn init_repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    run(path, &["init", "--initial-branch=main"]);
    run(path, &["config", "user.name", "Test"]);
    run(path, &["config", "user.email", "test@example.com"]);
    // Keep the tests independent of the host's global git configuration.
    run(path, &["config", "commit.gpgsign", "false"]);
    run(path, &["config", "core.autocrlf", "false"]);

    for (name, contents) in files {
        write(path, name, contents);
    }
    if !files.is_empty() {
        run(path, &["add", "."]);
        run(path, &["commit", "-m", "initial"]);
    }

    dir
}

fn run(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));

    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write(dir: &Path, name: &str, contents: &str) {
    let full = dir.join(name);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, contents).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

/// `git status --porcelain`, the ground truth for staging assertions.
fn porcelain(dir: &Path) -> String {
    run(dir, &["status", "--porcelain"])
}

const NUMBERED: &str = "one\ntwo\nthree\nfour\nfive\n";

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[test]
fn reports_staged_and_unstaged_changes_separately() {
    let dir = init_repo(&[("a.txt", "original\n"), ("b.txt", "original\n")]);
    let path = dir.path();

    write(path, "a.txt", "staged change\n");
    run(path, &["add", "a.txt"]);
    write(path, "b.txt", "unstaged change\n");
    write(path, "c.txt", "brand new\n");

    let repo = Repo::open(path).unwrap();
    let status = repo.status().unwrap();

    let a = status.files.iter().find(|f| f.path == "a.txt").unwrap();
    assert!(a.staged.is_some() && a.unstaged.is_none());

    let b = status.files.iter().find(|f| f.path == "b.txt").unwrap();
    assert!(b.staged.is_none() && b.unstaged.is_some());

    let c = status.files.iter().find(|f| f.path == "c.txt").unwrap();
    assert_eq!(c.unstaged, Some(cb_core::git::ChangeKind::Untracked));
}

#[test]
fn reports_the_current_branch() {
    let dir = init_repo(&[("a.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));
}

#[test]
fn handles_a_repository_with_no_commits() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init", "--initial-branch=main"]);
    write(dir.path(), "new.txt", "hello\n");

    let repo = Repo::open(dir.path()).unwrap();
    let status = repo.status().unwrap();

    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(status.files.iter().any(|f| f.path == "new.txt"));
}

#[test]
fn a_clean_repository_reports_no_changes() {
    let dir = init_repo(&[("a.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    assert!(repo.status().unwrap().files.is_empty());
}

// ---------------------------------------------------------------------------
// Diffs
// ---------------------------------------------------------------------------

#[test]
fn diffs_the_working_copy_against_head() {
    let dir = init_repo(&[("f.txt", NUMBERED)]);
    write(dir.path(), "f.txt", "one\nTWO\nthree\nfour\nfive\n");

    let repo = Repo::open(dir.path()).unwrap();
    let diff = repo
        .file_diff("f.txt", ComparisonMode::WorkingToHead)
        .unwrap();

    assert_eq!(diff.hunks.len(), 1);
    let changed: Vec<&str> = diff.hunks[0]
        .lines
        .iter()
        .filter(|l| l.origin != cb_core::git::LineOrigin::Context)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(changed, vec!["two", "TWO"]);
}

#[test]
fn the_three_comparison_modes_see_different_things() {
    let dir = init_repo(&[("f.txt", NUMBERED)]);
    let path = dir.path();

    // Stage one change, then make a second one on top of it.
    write(path, "f.txt", "one\nSTAGED\nthree\nfour\nfive\n");
    run(path, &["add", "f.txt"]);
    write(path, "f.txt", "one\nSTAGED\nthree\nWORKING\nfive\n");

    let repo = Repo::open(path).unwrap();

    let to_head = repo
        .file_diff("f.txt", ComparisonMode::WorkingToHead)
        .unwrap();
    let to_index = repo
        .file_diff("f.txt", ComparisonMode::WorkingToIndex)
        .unwrap();
    let staged = repo
        .file_diff("f.txt", ComparisonMode::IndexToHead)
        .unwrap();

    let contents = |d: &cb_core::git::FileDiff| -> Vec<String> {
        d.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.origin != cb_core::git::LineOrigin::Context)
            .map(|l| l.content.clone())
            .collect()
    };

    // Everything since the last commit.
    assert!(contents(&to_head).contains(&"STAGED".to_string()));
    assert!(contents(&to_head).contains(&"WORKING".to_string()));

    // Only what is not yet staged.
    assert!(contents(&to_index).contains(&"WORKING".to_string()));
    assert!(!contents(&to_index).contains(&"STAGED".to_string()));

    // Only what is staged.
    assert!(contents(&staged).contains(&"STAGED".to_string()));
    assert!(!contents(&staged).contains(&"WORKING".to_string()));
}

#[test]
fn baseline_content_follows_the_comparison_mode() {
    // This is what "revert" restores, so it must differ between modes or
    // reverting in the staged view would silently jump past the index.
    let dir = init_repo(&[("f.txt", "committed\n")]);
    let path = dir.path();

    write(path, "f.txt", "staged\n");
    run(path, &["add", "f.txt"]);
    write(path, "f.txt", "working\n");

    let repo = Repo::open(path).unwrap();

    assert_eq!(
        repo.baseline_content("f.txt", ComparisonMode::WorkingToHead)
            .unwrap(),
        Some("committed\n".to_string())
    );
    assert_eq!(
        repo.baseline_content("f.txt", ComparisonMode::WorkingToIndex)
            .unwrap(),
        Some("staged\n".to_string())
    );
}

#[test]
fn a_new_file_has_no_baseline() {
    let dir = init_repo(&[("a.txt", "x\n")]);
    write(dir.path(), "new.txt", "hello\n");

    let repo = Repo::open(dir.path()).unwrap();
    assert_eq!(
        repo.baseline_content("new.txt", ComparisonMode::WorkingToHead)
            .unwrap(),
        None
    );
}

// ---------------------------------------------------------------------------
// Reverting
// ---------------------------------------------------------------------------

/// Indices of the changed lines in a file's diff, for selecting by content.
fn changed_index_of(repo: &Repo, path: &str, mode: ComparisonMode, content: &str) -> u32 {
    let diff = repo.file_diff(path, mode).unwrap();
    diff.hunks
        .iter()
        .flat_map(|h| &h.lines)
        .find(|l| l.content == content)
        .unwrap_or_else(|| panic!("no diff line with content {content:?}"))
        .index
}

#[test]
fn reverts_a_single_line_leaving_the_others_alone() {
    let dir = init_repo(&[("f.txt", NUMBERED)]);
    let path = dir.path();

    // Two independent edits.
    write(path, "f.txt", "ONE\ntwo\nthree\nFOUR\nfive\n");

    let repo = Repo::open(path).unwrap();
    let mode = ComparisonMode::WorkingToHead;

    // Revert only the first, by selecting both sides of that one change.
    let mut selection = BTreeSet::new();
    selection.insert(changed_index_of(&repo, "f.txt", mode, "one"));
    selection.insert(changed_index_of(&repo, "f.txt", mode, "ONE"));

    assert!(repo.revert_lines("f.txt", mode, &selection).unwrap());

    assert_eq!(
        read(path, "f.txt"),
        "one\ntwo\nthree\nFOUR\nfive\n",
        "the unselected edit must survive"
    );
}

#[test]
fn reverts_an_added_line_without_touching_neighbouring_additions() {
    let dir = init_repo(&[("f.txt", "keep\n")]);
    let path = dir.path();
    write(path, "f.txt", "keep\nalpha\nbeta\n");

    let repo = Repo::open(path).unwrap();
    let mode = ComparisonMode::WorkingToHead;

    let selection = BTreeSet::from([changed_index_of(&repo, "f.txt", mode, "alpha")]);
    assert!(repo.revert_lines("f.txt", mode, &selection).unwrap());

    assert_eq!(read(path, "f.txt"), "keep\nbeta\n");
}

#[test]
fn reverts_a_deleted_line_restoring_it() {
    let dir = init_repo(&[("f.txt", NUMBERED)]);
    let path = dir.path();
    write(path, "f.txt", "one\nthree\nfour\nfive\n");

    let repo = Repo::open(path).unwrap();
    let mode = ComparisonMode::WorkingToHead;

    let selection = BTreeSet::from([changed_index_of(&repo, "f.txt", mode, "two")]);
    assert!(repo.revert_lines("f.txt", mode, &selection).unwrap());

    assert_eq!(read(path, "f.txt"), NUMBERED);
}

#[test]
fn reverting_a_whole_hunk_restores_it_completely() {
    let dir = init_repo(&[("f.txt", NUMBERED)]);
    let path = dir.path();
    write(path, "f.txt", "one\nCHANGED\nALSO\nfour\nfive\n");

    let repo = Repo::open(path).unwrap();
    let mode = ComparisonMode::WorkingToHead;

    let diff = repo.file_diff("f.txt", mode).unwrap();
    let selection = diff.hunk_line_indices(0);
    assert!(repo.revert_lines("f.txt", mode, &selection).unwrap());

    assert_eq!(read(path, "f.txt"), NUMBERED);
    assert_eq!(porcelain(path), "", "the file should be clean again");
}

#[test]
fn reverting_in_the_unstaged_view_returns_to_the_staged_state() {
    // Not to HEAD: the staged change must survive.
    let dir = init_repo(&[("f.txt", "committed\n")]);
    let path = dir.path();

    write(path, "f.txt", "staged\n");
    run(path, &["add", "f.txt"]);
    write(path, "f.txt", "working\n");

    let repo = Repo::open(path).unwrap();
    let mode = ComparisonMode::WorkingToIndex;

    let diff = repo.file_diff("f.txt", mode).unwrap();
    assert!(repo
        .revert_lines("f.txt", mode, &diff.changed_line_indices())
        .unwrap());

    assert_eq!(read(path, "f.txt"), "staged\n");
}

#[test]
fn an_empty_selection_reverts_nothing() {
    let dir = init_repo(&[("f.txt", NUMBERED)]);
    write(dir.path(), "f.txt", "changed\n");

    let repo = Repo::open(dir.path()).unwrap();
    let reverted = repo
        .revert_lines("f.txt", ComparisonMode::WorkingToHead, &BTreeSet::new())
        .unwrap();

    assert!(!reverted);
    assert_eq!(read(dir.path(), "f.txt"), "changed\n");
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

#[test]
fn stages_and_unstages_a_whole_file() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    write(path, "f.txt", "changed\n");

    let repo = Repo::open(path).unwrap();

    // Only the trailing newline is trimmed: porcelain's leading column is
    // significant — "M " means staged, " M" means unstaged.
    repo.stage_file("f.txt").unwrap();
    assert_eq!(porcelain(path).trim_end(), "M  f.txt", "should be staged");

    repo.unstage_file("f.txt").unwrap();
    assert_eq!(
        porcelain(path).trim_end(),
        " M f.txt",
        "should be unstaged again"
    );
}

#[test]
fn stages_only_the_selected_lines() {
    let dir = init_repo(&[("f.txt", "keep\n")]);
    let path = dir.path();
    write(path, "f.txt", "keep\nalpha\nbeta\n");

    let repo = Repo::open(path).unwrap();
    let mode = ComparisonMode::WorkingToIndex;

    let selection = BTreeSet::from([changed_index_of(&repo, "f.txt", mode, "alpha")]);
    assert!(repo.stage_lines("f.txt", &selection).unwrap());

    // Staged content has alpha but not beta; the working copy still has both.
    let staged = run(path, &["show", ":f.txt"]);
    assert_eq!(staged, "keep\nalpha\n");
    assert_eq!(read(path, "f.txt"), "keep\nalpha\nbeta\n");

    // Partially staged files show as modified in both columns.
    assert_eq!(porcelain(path).trim_end(), "MM f.txt");
}

#[test]
fn unstages_only_the_selected_lines() {
    let dir = init_repo(&[("f.txt", "keep\n")]);
    let path = dir.path();

    write(path, "f.txt", "keep\nalpha\nbeta\n");
    run(path, &["add", "f.txt"]);

    let repo = Repo::open(path).unwrap();
    let selection = BTreeSet::from([changed_index_of(
        &repo,
        "f.txt",
        ComparisonMode::IndexToHead,
        "beta",
    )]);
    assert!(repo.unstage_lines("f.txt", &selection).unwrap());

    let staged = run(path, &["show", ":f.txt"]);
    assert_eq!(staged, "keep\nalpha\n", "beta should no longer be staged");
    assert_eq!(
        read(path, "f.txt"),
        "keep\nalpha\nbeta\n",
        "working copy untouched"
    );
}

#[test]
fn staging_a_deletion_records_it() {
    let dir = init_repo(&[("gone.txt", "bye\n"), ("stay.txt", "hi\n")]);
    let path = dir.path();
    std::fs::remove_file(path.join("gone.txt")).unwrap();

    let repo = Repo::open(path).unwrap();
    repo.stage_file("gone.txt").unwrap();

    assert_eq!(porcelain(path).trim_end(), "D  gone.txt");
}

#[test]
fn discarding_a_file_restores_it_from_head() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    write(path, "f.txt", "mangled\n");

    let repo = Repo::open(path).unwrap();
    repo.discard_file("f.txt").unwrap();

    assert_eq!(read(path, "f.txt"), "original\n");
}

#[test]
fn writing_a_file_from_the_diff_view_persists() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    repo.write_working_file("f.txt", "edited in place\n")
        .unwrap();

    assert_eq!(read(dir.path(), "f.txt"), "edited in place\n");
    assert_eq!(
        repo.working_content("f.txt").unwrap(),
        Some("edited in place\n".to_string())
    );
}

// ---------------------------------------------------------------------------
// Commits, branches, history
// ---------------------------------------------------------------------------

#[test]
fn commits_staged_changes() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    write(path, "f.txt", "changed\n");

    let repo = Repo::open(path).unwrap();
    repo.stage_file("f.txt").unwrap();
    let id = repo.commit("update f", false).unwrap();

    assert!(!id.is_empty());
    assert_eq!(porcelain(path), "", "working tree should be clean");
    assert!(run(path, &["log", "-1", "--pretty=%s"]).contains("update f"));
}

#[test]
fn refuses_an_empty_commit_message() {
    let dir = init_repo(&[("f.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    assert!(repo.commit("   ", false).is_err());
}

#[test]
fn amends_the_previous_commit() {
    let dir = init_repo(&[("f.txt", "x\n")]);
    let path = dir.path();

    let repo = Repo::open(path).unwrap();
    repo.commit("corrected message", true).unwrap();

    assert_eq!(
        run(path, &["log", "-1", "--pretty=%s"]).trim(),
        "corrected message"
    );
    assert_eq!(run(path, &["rev-list", "--count", "HEAD"]).trim(), "1");
}

#[test]
fn creates_switches_and_deletes_branches() {
    let dir = init_repo(&[("f.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    repo.create_branch("feature", true).unwrap();
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("feature"));

    repo.checkout_branch("main").unwrap();
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));

    repo.delete_branch("feature").unwrap();
    assert!(!repo.branches().unwrap().iter().any(|b| b.name == "feature"));
}

#[test]
fn creates_a_branch_from_another_branch() {
    let dir = init_repo(&[("f.txt", "x\n")]);
    let path = dir.path();
    let repo = Repo::open(path).unwrap();

    // `base` gains a commit that main does not have.
    repo.create_branch("base", true).unwrap();
    write(path, "g.txt", "on base\n");
    run(path, &["add", "."]);
    run(path, &["commit", "-m", "base work"]);
    repo.checkout_branch("main").unwrap();

    // Branching from `base` while standing on main must take base's tip.
    repo.create_branch_from("feature", "base", true).unwrap();

    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("feature"));
    assert_eq!(
        read(path, "g.txt"),
        "on base\n",
        "the branch must start at base's tip"
    );
}

#[test]
fn checking_out_a_remote_branch_creates_a_tracking_local() {
    // Repo A has a slash-named branch; a clone of it sees only the
    // remote-tracking ref. Checking that out must behave like `git switch`:
    // local branch, upstream set, no detached HEAD.
    let origin = init_repo(&[("f.txt", "x\n")]);
    run(origin.path(), &["branch", "users/anthony/feature"]);

    let clone_dir = tempfile::tempdir().unwrap();
    let clone_path = clone_dir.path().join("clone");
    run(
        clone_dir.path(),
        &[
            "clone",
            origin.path().to_str().unwrap(),
            clone_path.to_str().unwrap(),
        ],
    );

    let repo = Repo::open(&clone_path).unwrap();
    repo.checkout_remote_branch("origin/users/anthony/feature")
        .unwrap();

    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("users/anthony/feature"));

    let branch = repo
        .branches()
        .unwrap()
        .into_iter()
        .find(|b| !b.is_remote && b.name == "users/anthony/feature")
        .expect("a local branch must exist");
    assert_eq!(
        branch.upstream.as_deref(),
        Some("origin/users/anthony/feature")
    );

    // Doing it again (local already exists) is a plain switch, not an error.
    repo.checkout_branch("main").unwrap();
    repo.checkout_remote_branch("origin/users/anthony/feature")
        .unwrap();
    assert_eq!(
        repo.status().unwrap().branch.as_deref(),
        Some("users/anthony/feature")
    );
}

#[test]
fn refuses_to_delete_the_checked_out_branch() {
    let dir = init_repo(&[("f.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    let err = repo.delete_branch("main").unwrap_err().to_string();
    assert!(err.contains("checked out"), "got {err}");
}

#[test]
fn reads_history_newest_first() {
    let dir = init_repo(&[("f.txt", "one\n")]);
    let path = dir.path();

    write(path, "f.txt", "two\n");
    run(path, &["commit", "-am", "second"]);
    write(path, "f.txt", "three\n");
    run(path, &["commit", "-am", "third"]);

    let repo = Repo::open(path).unwrap();
    let history = repo.history(10).unwrap();

    assert_eq!(history.len(), 3);
    assert_eq!(history[0].summary, "third");
    assert_eq!(history[2].summary, "initial");
    assert_eq!(history[0].author_name, "Test");
    assert_eq!(history[0].short_id.len(), 7);
}

#[test]
fn history_of_an_empty_repository_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init", "--initial-branch=main"]);

    let repo = Repo::open(dir.path()).unwrap();
    assert!(repo.history(10).unwrap().is_empty());
}

#[test]
fn diffs_the_changes_a_commit_introduced() {
    let dir = init_repo(&[("f.txt", "one\n")]);
    let path = dir.path();
    write(path, "f.txt", "two\n");
    run(path, &["commit", "-am", "second"]);

    let repo = Repo::open(path).unwrap();
    let head = repo.history(1).unwrap().remove(0);
    let diff = repo.commit_diff(&head.id).unwrap();

    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].path, "f.txt");
    let changed: Vec<&str> = diff[0].hunks[0]
        .lines
        .iter()
        .filter(|l| l.origin != cb_core::git::LineOrigin::Context)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(changed, vec!["one", "two"]);
}

// -- both sides of a file at a commit, for the History tab's diff viewer -----

#[test]
fn reads_both_sides_of_a_file_a_commit_changed() {
    let dir = init_repo(&[("f.txt", "one\n")]);
    let path = dir.path();
    write(path, "f.txt", "two\n");
    run(path, &["commit", "-am", "second"]);

    let repo = Repo::open(path).unwrap();
    let head = repo.history(1).unwrap().remove(0);
    let contents = repo.commit_file_contents(&head.id, "f.txt").unwrap();

    assert_eq!(contents.baseline.as_deref(), Some("one\n"));
    assert_eq!(contents.working.as_deref(), Some("two\n"));
}

/// A file the commit added has no previous side. `None` rather than an empty
/// string, so the viewer shows a plain editor instead of an all-green diff.
#[test]
fn a_file_added_by_a_commit_has_no_baseline() {
    let dir = init_repo(&[("f.txt", "one\n")]);
    let path = dir.path();
    write(path, "added.txt", "brand new\n");
    run(path, &["add", "."]);
    run(path, &["commit", "-m", "add a file"]);

    let repo = Repo::open(path).unwrap();
    let head = repo.history(1).unwrap().remove(0);
    let contents = repo.commit_file_contents(&head.id, "added.txt").unwrap();

    assert_eq!(contents.baseline, None);
    assert_eq!(contents.working.as_deref(), Some("brand new\n"));
}

/// Deleted by the commit: the file exists before and not after.
#[test]
fn a_file_deleted_by_a_commit_has_no_content_after_it() {
    let dir = init_repo(&[("f.txt", "one\n"), ("gone.txt", "doomed\n")]);
    let path = dir.path();
    run(path, &["rm", "gone.txt"]);
    run(path, &["commit", "-m", "remove a file"]);

    let repo = Repo::open(path).unwrap();
    let head = repo.history(1).unwrap().remove(0);
    let contents = repo.commit_file_contents(&head.id, "gone.txt").unwrap();

    assert_eq!(contents.baseline.as_deref(), Some("doomed\n"));
    assert_eq!(contents.working, None);
}

/// The first commit has no parent, so everything in it is new.
#[test]
fn a_root_commit_has_no_baseline_for_any_of_its_files() {
    let dir = init_repo(&[("f.txt", "one\n")]);
    let path = dir.path();

    let repo = Repo::open(path).unwrap();
    let root = repo.history(1).unwrap().remove(0);
    let contents = repo.commit_file_contents(&root.id, "f.txt").unwrap();

    assert_eq!(contents.baseline, None);
    assert_eq!(contents.working.as_deref(), Some("one\n"));
}

#[test]
fn an_unknown_commit_is_an_error_rather_than_empty_contents() {
    // Silently returning "no content" would render an empty diff that looks
    // like a commit which changed nothing.
    let dir = init_repo(&[("f.txt", "one\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    assert!(repo.commit_file_contents("not-a-commit", "f.txt").is_err());
}

#[test]
fn stashes_and_restores_changes() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    write(path, "f.txt", "work in progress\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_save("wip").unwrap();
    assert_eq!(read(path, "f.txt"), "original\n");

    repo.stash_pop(0).unwrap();
    assert_eq!(read(path, "f.txt"), "work in progress\n");
}

#[test]
fn lists_every_stash_newest_first_with_its_message_and_commit() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    let mut repo = Repo::open(path).unwrap();

    write(path, "f.txt", "first change\n");
    repo.stash_save("first").unwrap();
    write(path, "f.txt", "second change\n");
    repo.stash_save("second").unwrap();

    let stashes = repo.stash_list().unwrap();
    assert_eq!(stashes.len(), 2);

    // index 0 is the most recent (git's stash@{0}).
    assert_eq!(stashes[0].index, 0);
    assert_eq!(stashes[1].index, 1);
    assert!(stashes[0].message.contains("second"));
    assert!(stashes[1].message.contains("first"));

    // The oid must name a real commit whose diff the preview can read.
    assert!(!stashes[0].id.is_empty());
    let diff = repo.commit_diff(&stashes[0].id).unwrap();
    assert!(diff.iter().any(|d| d.path == "f.txt"));

    // The branch is parsed from the "On <branch>:" prefix git writes.
    assert_eq!(stashes[0].branch.as_deref(), Some("main"));
}

#[test]
fn apply_restores_a_specific_stash_without_dropping_it() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    let mut repo = Repo::open(path).unwrap();

    write(path, "f.txt", "first change\n");
    repo.stash_save("first").unwrap();
    write(path, "f.txt", "second change\n");
    repo.stash_save("second").unwrap();

    // Apply the older stash (index 1) and confirm it stays in the list.
    repo.stash_apply(1).unwrap();
    assert_eq!(read(path, "f.txt"), "first change\n");
    assert_eq!(repo.stash_list().unwrap().len(), 2);
}

#[test]
fn drop_removes_one_stash_and_reindexes_the_rest() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    let mut repo = Repo::open(path).unwrap();

    write(path, "f.txt", "first change\n");
    repo.stash_save("first").unwrap();
    write(path, "f.txt", "second change\n");
    repo.stash_save("second").unwrap();

    repo.stash_drop(0).unwrap();
    let stashes = repo.stash_list().unwrap();
    assert_eq!(stashes.len(), 1);
    // What was stash@{1} ("first") is now stash@{0}.
    assert_eq!(stashes[0].index, 0);
    assert!(stashes[0].message.contains("first"));
}

#[test]
fn clear_removes_every_stash() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    let mut repo = Repo::open(path).unwrap();

    for text in ["a\n", "b\n", "c\n"] {
        write(path, "f.txt", text);
        repo.stash_save("wip").unwrap();
    }
    assert_eq!(repo.stash_list().unwrap().len(), 3);

    repo.stash_clear().unwrap();
    assert!(repo.stash_list().unwrap().is_empty());
}

#[test]
fn listing_stashes_on_a_clean_repo_is_empty_not_an_error() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let mut repo = Repo::open(dir.path()).unwrap();
    assert!(repo.stash_list().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Path-scoped stash (`git stash push -- <paths>`)
//
// libgit2 has no *usable* pathspec stash, so `stash_paths` builds the stash
// commit by hand. These tests exist to prove the hand-built commit is
// indistinguishable from the real thing — to our own reader and to git itself.
// ---------------------------------------------------------------------------

#[test]
fn stashing_one_path_leaves_every_other_change_alone() {
    // The headline behaviour, and the one libgit2's own pathspec stash gets
    // wrong: it reverts the whole working tree and keeps only the pathspec.
    let dir = init_repo(&[("a.txt", "a original\n"), ("b.txt", "b original\n")]);
    let path = dir.path();
    write(path, "a.txt", "a changed\n");
    write(path, "b.txt", "b changed\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("just a", &["a.txt".to_string()]).unwrap();

    assert_eq!(read(path, "a.txt"), "a original\n");
    assert_eq!(read(path, "b.txt"), "b changed\n");

    let still_dirty: Vec<String> = repo
        .status()
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(still_dirty, vec!["b.txt".to_string()]);
}

#[test]
fn a_path_scoped_stash_pops_back_only_what_it_took() {
    let dir = init_repo(&[("a.txt", "a original\n"), ("b.txt", "b original\n")]);
    let path = dir.path();
    write(path, "a.txt", "a changed\n");
    write(path, "b.txt", "b changed\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("just a", &["a.txt".to_string()]).unwrap();
    repo.stash_pop(0).unwrap();

    assert_eq!(read(path, "a.txt"), "a changed\n");
    assert_eq!(read(path, "b.txt"), "b changed\n");
    assert!(repo.stash_list().unwrap().is_empty());
}

#[test]
fn multiple_paths_go_into_one_stash() {
    let dir = init_repo(&[
        ("a.txt", "a original\n"),
        ("b.txt", "b original\n"),
        ("c.txt", "c original\n"),
    ]);
    let path = dir.path();
    for name in ["a.txt", "b.txt", "c.txt"] {
        write(path, name, "changed\n");
    }

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("a and c", &["a.txt".to_string(), "c.txt".to_string()])
        .unwrap();

    assert_eq!(read(path, "a.txt"), "a original\n");
    assert_eq!(read(path, "b.txt"), "changed\n");
    assert_eq!(read(path, "c.txt"), "c original\n");
    assert_eq!(repo.stash_list().unwrap().len(), 1);
}

#[test]
fn both_staged_and_unstaged_changes_for_a_path_are_stashed_and_reset() {
    // git captures the index state and the working state as two trees. Both
    // have to reset, or "stash this file" leaves half of it behind.
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();

    write(path, "a.txt", "staged\n");
    run(path, &["add", "a.txt"]);
    write(path, "a.txt", "staged then edited\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("both halves", &["a.txt".to_string()])
        .unwrap();

    assert_eq!(read(path, "a.txt"), "original\n");
    assert!(
        repo.status().unwrap().files.is_empty(),
        "nothing may be left staged or unstaged"
    );

    repo.stash_pop(0).unwrap();
    assert_eq!(read(path, "a.txt"), "staged then edited\n");
}

#[test]
fn an_untracked_selected_file_is_stashed_and_removed_from_disk() {
    let dir = init_repo(&[("kept.txt", "kept\n")]);
    let path = dir.path();
    write(path, "new.txt", "brand new\n");
    write(path, "other.txt", "also new\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("the new file", &["new.txt".to_string()])
        .unwrap();

    assert!(!path.join("new.txt").exists(), "the stashed file must go");
    assert_eq!(read(path, "other.txt"), "also new\n");
}

#[test]
fn popping_a_path_scoped_stash_restores_the_untracked_file() {
    let dir = init_repo(&[("kept.txt", "kept\n")]);
    let path = dir.path();
    write(path, "new.txt", "brand new\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("the new file", &["new.txt".to_string()])
        .unwrap();
    repo.stash_pop(0).unwrap();

    assert_eq!(read(path, "new.txt"), "brand new\n");
}

#[test]
fn a_deleted_selected_file_is_stashed_and_restored_from_head() {
    let dir = init_repo(&[("gone.txt", "here\n"), ("other.txt", "other\n")]);
    let path = dir.path();
    std::fs::remove_file(path.join("gone.txt")).unwrap();
    write(path, "other.txt", "edited\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("the deletion", &["gone.txt".to_string()])
        .unwrap();

    assert_eq!(read(path, "gone.txt"), "here\n");
    assert_eq!(read(path, "other.txt"), "edited\n");

    repo.stash_pop(0).unwrap();
    assert!(!path.join("gone.txt").exists());
}

#[test]
fn a_staged_new_file_is_stashed_and_the_index_entry_goes_with_it() {
    let dir = init_repo(&[("kept.txt", "kept\n")]);
    let path = dir.path();
    write(path, "added.txt", "added\n");
    run(path, &["add", "added.txt"]);

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("the addition", &["added.txt".to_string()])
        .unwrap();

    assert!(!path.join("added.txt").exists());
    assert!(repo.status().unwrap().files.is_empty());

    repo.stash_pop(0).unwrap();
    assert_eq!(read(path, "added.txt"), "added\n");
}

#[test]
fn the_stash_reads_back_through_the_existing_list_and_diff_path() {
    // The compatibility contract with `stash_list` and the panel's preview,
    // which reads a stash through the ordinary commit-diff path.
    let dir = init_repo(&[("a.txt", "a original\n"), ("b.txt", "b original\n")]);
    let path = dir.path();
    write(path, "a.txt", "a changed\n");
    write(path, "b.txt", "b changed\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("readable", &["a.txt".to_string()])
        .unwrap();

    let stashes = repo.stash_list().unwrap();
    assert_eq!(stashes.len(), 1);
    assert_eq!(stashes[0].index, 0);
    assert_eq!(stashes[0].branch.as_deref(), Some("main"));
    assert!(stashes[0].message.contains("readable"));
    assert!(!stashes[0].id.is_empty());

    let touched: BTreeSet<String> = repo
        .commit_diff(&stashes[0].id)
        .unwrap()
        .into_iter()
        .map(|d| d.path)
        .collect();
    assert!(touched.contains("a.txt"));
    assert!(
        !touched.contains("b.txt"),
        "the stash must not carry the file it was not given: {touched:?}"
    );
}

#[test]
fn the_first_path_stash_in_a_repo_is_listed() {
    // `refs/stash` is not a ref git logs automatically, so it has to be given a
    // reflog before it is written. Without that the very first stash in a
    // repository is created and then invisible — every later one works, because
    // by then the log file exists.
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();
    // Belt and braces: even with ref logging switched off entirely, a stash
    // must still be findable, exactly as it is under the git CLI.
    run(path, &["config", "core.logAllRefUpdates", "false"]);
    write(path, "a.txt", "changed\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("the very first", &["a.txt".to_string()])
        .unwrap();

    let stashes = repo.stash_list().unwrap();
    assert_eq!(stashes.len(), 1, "the first stash must be listed");
    assert!(stashes[0].message.contains("the very first"));
}

#[test]
fn the_stash_commit_has_the_shape_git_expects() {
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();
    write(path, "a.txt", "changed\n");
    write(path, "new.txt", "untracked\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("shaped", &["a.txt".to_string(), "new.txt".to_string()])
        .unwrap();

    // Three parents when untracked files are included: base, index, untracked.
    let raw = run(path, &["cat-file", "-p", "refs/stash"]);
    assert_eq!(
        raw.lines().filter(|l| l.starts_with("parent ")).count(),
        3,
        "expected base + index + untracked parents:\n{raw}"
    );
    assert!(raw.contains("On main: shaped"), "{raw}");

    assert!(run(path, &["log", "-1", "--format=%s", "refs/stash^2"]).starts_with("index on main:"));
    let untracked = run(path, &["log", "-1", "--format=%s", "refs/stash^3"]);
    assert!(
        untracked.starts_with("untracked files on main:"),
        "{untracked}"
    );
    // The untracked commit is a root commit — it has no parents at all.
    assert!(run(path, &["rev-list", "--count", "refs/stash^3"]).trim() == "1");
}

#[test]
fn the_git_cli_reads_the_stash_this_app_wrote() {
    // The strongest evidence the hand-built commit is right: git lists it and
    // pops it, restoring exactly what was taken and nothing else.
    let dir = init_repo(&[("a.txt", "a original\n"), ("b.txt", "b original\n")]);
    let path = dir.path();
    write(path, "a.txt", "a changed\n");
    write(path, "b.txt", "b changed\n");
    write(path, "new.txt", "untracked\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("for git", &["a.txt".to_string(), "new.txt".to_string()])
        .unwrap();

    let listed = run(path, &["stash", "list"]);
    assert!(listed.contains("for git"), "git stash list said: {listed}");

    let shown = run(
        path,
        &[
            "stash",
            "show",
            "--include-untracked",
            "--name-only",
            "stash@{0}",
        ],
    );
    assert!(shown.contains("a.txt"), "{shown}");
    assert!(shown.contains("new.txt"), "{shown}");
    assert!(!shown.contains("b.txt"), "{shown}");

    run(path, &["stash", "pop"]);
    assert_eq!(read(path, "a.txt"), "a changed\n");
    assert_eq!(read(path, "b.txt"), "b changed\n");
    assert_eq!(read(path, "new.txt"), "untracked\n");
}

#[test]
fn a_crlf_working_file_round_trips_through_a_path_scoped_stash() {
    // The blob written into the stash tree must go through the same filters as
    // any other blob. Storing the file's raw bytes would put CRLF where every
    // other tree in the repository holds LF, so the stash would show the whole
    // file as changed and popping it would dirty every line.
    let dir = init_repo(&[("a.txt", "one\ntwo\n"), ("b.txt", "b\n")]);
    let path = dir.path();
    run(path, &["config", "core.autocrlf", "true"]);
    std::fs::write(path.join("a.txt"), "one\r\ntwo changed\r\n").unwrap();

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("crlf", &["a.txt".to_string()]).unwrap();

    let stashes = repo.stash_list().unwrap();
    let diff = repo.commit_diff(&stashes[0].id).unwrap();
    let file = diff.iter().find(|d| d.path == "a.txt").unwrap();
    assert_eq!(
        file.changed_line_indices().len(),
        2,
        "only the edited line should differ, not the whole file"
    );
}

#[test]
fn a_detached_head_stash_records_no_branch() {
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();
    run(path, &["checkout", "--detach"]);
    write(path, "a.txt", "changed\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_paths("detached", &["a.txt".to_string()])
        .unwrap();

    let stashes = repo.stash_list().unwrap();
    assert_eq!(stashes[0].branch.as_deref(), Some("(no branch)"));
}

#[test]
fn stashing_a_path_with_no_changes_is_refused_by_name() {
    // Silently writing an empty stash would look like it worked. The user asked
    // for a change to be set aside; naming the wrong file has to say so.
    let dir = init_repo(&[("a.txt", "original\n"), ("b.txt", "b\n")]);
    let path = dir.path();
    write(path, "a.txt", "changed\n");

    let mut repo = Repo::open(path).unwrap();
    let err = repo
        .stash_paths("nothing here", &["b.txt".to_string()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("b.txt"), "the error must name the path: {err}");
    assert!(repo.stash_list().unwrap().is_empty());
    assert_eq!(read(path, "a.txt"), "changed\n");
}

#[test]
fn stashing_nothing_is_refused() {
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();
    write(path, "a.txt", "changed\n");

    let mut repo = Repo::open(path).unwrap();
    assert!(repo.stash_paths("empty", &[]).is_err());
    assert_eq!(read(path, "a.txt"), "changed\n");
}

#[test]
fn stashing_an_unknown_path_is_refused_by_name() {
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();
    write(path, "a.txt", "changed\n");

    let mut repo = Repo::open(path).unwrap();
    let err = repo
        .stash_paths("ghost", &["nope.txt".to_string()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("nope.txt"), "{err}");
}

#[test]
fn stashing_an_escaping_path_is_refused() {
    // The path comes from the UI, but a stash must never reach outside the
    // workspace even if something upstream sends a traversal.
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();
    write(path, "a.txt", "changed\n");

    let mut repo = Repo::open(path).unwrap();
    assert!(repo
        .stash_paths("escape", &["../outside.txt".to_string()])
        .is_err());
    assert!(repo
        .stash_paths("absolute", &["/etc/passwd".to_string()])
        .is_err());
}

#[test]
fn stashing_before_the_first_commit_is_refused() {
    // A stash commit needs a base commit to hang off; git refuses too.
    let dir = init_repo(&[]);
    let path = dir.path();
    write(path, "a.txt", "brand new\n");

    let mut repo = Repo::open(path).unwrap();
    assert!(repo
        .stash_paths("too early", &["a.txt".to_string()])
        .is_err());
    assert_eq!(read(path, "a.txt"), "brand new\n");
}

// ---------------------------------------------------------------------------
// Network commands
// ---------------------------------------------------------------------------

#[test]
fn network_commands_inherit_the_users_environment() {
    // Passing an environment would shadow the credential helpers, SSH agent
    // and keychain settings the system git needs to authenticate.
    let dir = init_repo(&[("f.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    let push = repo.network_command(cb_core::git::repo::NetworkOperation::Push);
    assert_eq!(push.program, "git");
    assert_eq!(push.args, vec!["push"]);
    assert!(
        push.env.is_empty(),
        "network commands must not override the environment"
    );
    assert_eq!(push.cwd, repo.workdir());
}

#[test]
fn pushing_a_new_branch_sets_its_upstream() {
    let dir = init_repo(&[("f.txt", "x\n")]);
    let repo = Repo::open(dir.path()).unwrap();

    let push = repo.network_command(cb_core::git::repo::NetworkOperation::PushSetUpstream(
        "feature".into(),
    ));
    assert_eq!(push.args, vec!["push", "-u", "origin", "feature"]);
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

#[test]
fn opens_from_a_subdirectory() {
    let dir = init_repo(&[("src/deep/f.txt", "x\n")]);
    let repo = Repo::open(&dir.path().join("src/deep")).unwrap();

    assert_eq!(
        repo.workdir().canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );
}

#[test]
fn opening_a_non_repository_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Repo::open(dir.path()).is_err());
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

use cb_core::git::repo::MergeOutcome;

/// A repository on `main` with a `feature` branch one commit ahead.
///
/// `main` is left checked out, so a merge of `feature` fast-forwards unless
/// the caller commits to `main` first.
fn repo_with_feature_branch() -> tempfile::TempDir {
    let dir = init_repo(&[("a.txt", "one\n")]);
    let path = dir.path();

    run(path, &["checkout", "-b", "feature"]);
    write(path, "b.txt", "from feature\n");
    run(path, &["add", "."]);
    run(path, &["commit", "-m", "feature work"]);
    run(path, &["checkout", "main"]);

    dir
}

#[test]
fn merging_a_branch_ahead_of_head_fast_forwards() {
    let dir = repo_with_feature_branch();
    let repo = Repo::open(dir.path()).unwrap();

    let report = repo.merge_branch("feature").unwrap();

    assert_eq!(report.outcome, MergeOutcome::FastForward);
    assert_eq!(read(dir.path(), "b.txt"), "from feature\n");
    // A fast-forward must not invent a merge commit.
    assert_eq!(
        run(dir.path(), &["rev-list", "--count", "HEAD"]).trim(),
        "2"
    );
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));
}

#[test]
fn merging_diverged_branches_creates_a_merge_commit() {
    let dir = repo_with_feature_branch();
    let path = dir.path();

    // Diverge: a commit on main that feature does not have.
    write(path, "c.txt", "from main\n");
    run(path, &["add", "."]);
    run(path, &["commit", "-m", "main work"]);

    let repo = Repo::open(path).unwrap();
    let report = repo.merge_branch("feature").unwrap();

    assert_eq!(report.outcome, MergeOutcome::Merged);
    assert!(report.commit.is_some());
    // Both sides' files are present, and the commit has two parents.
    assert_eq!(read(path, "b.txt"), "from feature\n");
    assert_eq!(read(path, "c.txt"), "from main\n");
    assert_eq!(
        run(path, &["cat-file", "-p", "HEAD"])
            .matches("parent ")
            .count(),
        2
    );
    // The merge must be finished, not left in progress.
    assert_eq!(repo.status().unwrap().in_progress_operation, None);
}

#[test]
fn merging_a_branch_already_contained_reports_up_to_date() {
    let dir = repo_with_feature_branch();
    let repo = Repo::open(dir.path()).unwrap();

    repo.merge_branch("feature").unwrap();
    let again = repo.merge_branch("feature").unwrap();

    assert_eq!(again.outcome, MergeOutcome::UpToDate);
    assert_eq!(again.commit, None);
}

#[test]
fn a_conflicting_merge_is_reported_and_left_in_progress() {
    // Left in progress on purpose: aborting silently would throw away the
    // resolution work the user is about to do in the Changes tab.
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();

    run(path, &["checkout", "-b", "feature"]);
    write(path, "a.txt", "feature version\n");
    run(path, &["commit", "-am", "feature edit"]);

    run(path, &["checkout", "main"]);
    write(path, "a.txt", "main version\n");
    run(path, &["commit", "-am", "main edit"]);

    let repo = Repo::open(path).unwrap();
    let report = repo.merge_branch("feature").unwrap();

    assert_eq!(report.outcome, MergeOutcome::Conflicted);
    assert_eq!(report.conflicts, vec!["a.txt"]);
    assert_eq!(report.commit, None);

    let status = repo.status().unwrap();
    assert_eq!(status.in_progress_operation.as_deref(), Some("merge"));
    assert!(status
        .files
        .iter()
        .any(|f| f.path == "a.txt" && f.is_conflicted()));
}

#[test]
fn aborting_a_conflicted_merge_restores_the_previous_state() {
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();

    run(path, &["checkout", "-b", "feature"]);
    write(path, "a.txt", "feature version\n");
    run(path, &["commit", "-am", "feature edit"]);
    run(path, &["checkout", "main"]);
    write(path, "a.txt", "main version\n");
    run(path, &["commit", "-am", "main edit"]);

    let repo = Repo::open(path).unwrap();
    repo.merge_branch("feature").unwrap();
    repo.abort_merge().unwrap();

    assert_eq!(read(path, "a.txt"), "main version\n");
    let status = repo.status().unwrap();
    assert_eq!(status.in_progress_operation, None);
    assert!(
        status.files.is_empty(),
        "the working tree should be clean again"
    );
}

#[test]
fn merging_refuses_to_start_with_modified_tracked_files() {
    // A conflicted merge writes into the working tree; doing that on top of
    // uncommitted edits makes the two impossible to tell apart.
    let dir = repo_with_feature_branch();
    write(dir.path(), "a.txt", "uncommitted edit\n");

    let error = Repo::open(dir.path())
        .unwrap()
        .merge_branch("feature")
        .unwrap_err()
        .to_string();

    assert!(error.contains("commit or stash"), "got: {error}");
    assert!(
        error.contains("a.txt"),
        "the error should name a file: {error}"
    );
    // Nothing may have happened.
    assert_eq!(read(dir.path(), "a.txt"), "uncommitted edit\n");
    assert!(!dir.path().join("b.txt").exists());
}

#[test]
fn untracked_files_do_not_block_a_merge() {
    // `git merge` allows them, and refusing would be gratuitous.
    let dir = repo_with_feature_branch();
    write(dir.path(), "scratch.txt", "not tracked\n");

    let report = Repo::open(dir.path())
        .unwrap()
        .merge_branch("feature")
        .unwrap();

    assert_eq!(report.outcome, MergeOutcome::FastForward);
    assert_eq!(read(dir.path(), "scratch.txt"), "not tracked\n");
}

#[test]
fn merging_an_unknown_branch_fails_clearly() {
    let dir = repo_with_feature_branch();
    let error = Repo::open(dir.path())
        .unwrap()
        .merge_branch("nope")
        .unwrap_err()
        .to_string();

    assert!(error.contains("nope"), "got: {error}");
}

#[test]
fn a_second_merge_is_refused_while_one_is_in_progress() {
    let dir = init_repo(&[("a.txt", "original\n")]);
    let path = dir.path();

    run(path, &["checkout", "-b", "feature"]);
    write(path, "a.txt", "feature version\n");
    run(path, &["commit", "-am", "feature edit"]);
    run(path, &["checkout", "main"]);
    write(path, "a.txt", "main version\n");
    run(path, &["commit", "-am", "main edit"]);

    let repo = Repo::open(path).unwrap();
    repo.merge_branch("feature").unwrap();

    let error = repo.merge_branch("feature").unwrap_err().to_string();
    assert!(error.contains("already in progress"), "got: {error}");
}
