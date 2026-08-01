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
    let diff = repo.file_diff("f.txt", ComparisonMode::WorkingToHead).unwrap();

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

    let to_head = repo.file_diff("f.txt", ComparisonMode::WorkingToHead).unwrap();
    let to_index = repo.file_diff("f.txt", ComparisonMode::WorkingToIndex).unwrap();
    let staged = repo.file_diff("f.txt", ComparisonMode::IndexToHead).unwrap();

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
        repo.baseline_content("f.txt", ComparisonMode::WorkingToHead).unwrap(),
        Some("committed\n".to_string())
    );
    assert_eq!(
        repo.baseline_content("f.txt", ComparisonMode::WorkingToIndex).unwrap(),
        Some("staged\n".to_string())
    );
}

#[test]
fn a_new_file_has_no_baseline() {
    let dir = init_repo(&[("a.txt", "x\n")]);
    write(dir.path(), "new.txt", "hello\n");

    let repo = Repo::open(dir.path()).unwrap();
    assert_eq!(
        repo.baseline_content("new.txt", ComparisonMode::WorkingToHead).unwrap(),
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
    assert!(repo.revert_lines("f.txt", mode, &diff.changed_line_indices()).unwrap());

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
    assert_eq!(porcelain(path).trim_end(), " M f.txt", "should be unstaged again");
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
    let selection =
        BTreeSet::from([changed_index_of(&repo, "f.txt", ComparisonMode::IndexToHead, "beta")]);
    assert!(repo.unstage_lines("f.txt", &selection).unwrap());

    let staged = run(path, &["show", ":f.txt"]);
    assert_eq!(staged, "keep\nalpha\n", "beta should no longer be staged");
    assert_eq!(read(path, "f.txt"), "keep\nalpha\nbeta\n", "working copy untouched");
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

    repo.write_working_file("f.txt", "edited in place\n").unwrap();

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

    assert_eq!(run(path, &["log", "-1", "--pretty=%s"]).trim(), "corrected message");
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

#[test]
fn stashes_and_restores_changes() {
    let dir = init_repo(&[("f.txt", "original\n")]);
    let path = dir.path();
    write(path, "f.txt", "work in progress\n");

    let mut repo = Repo::open(path).unwrap();
    repo.stash_save("wip").unwrap();
    assert_eq!(read(path, "f.txt"), "original\n");

    repo.stash_pop().unwrap();
    assert_eq!(read(path, "f.txt"), "work in progress\n");
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
    assert!(push.env.is_empty(), "network commands must not override the environment");
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
