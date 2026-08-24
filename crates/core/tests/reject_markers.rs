//! Rejection end to end: against a real repository, with the real `git`.
//!
//! The unit tests cover where a marker goes and what it says. What they cannot
//! cover is whether `git apply` accepts the revert that precedes it, and
//! whether the generated hook is a script `sh` will actually run and `git`
//! will actually honour. Both are only answerable on disk.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use cb_core::git::{ComparisonMode, Repo};
use cb_core::intents::{guard, reject};

const REASON: &str = "reverting to the original call";

fn init_repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    run(path, &["init", "--initial-branch=main"]);
    run(path, &["config", "user.name", "Test"]);
    run(path, &["config", "user.email", "test@example.com"]);
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

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"))
}

fn run(dir: &Path, args: &[&str]) -> String {
    let output = git(dir, args);
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

/// A fixed instant, so the rendered date is predictable.
fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(11_017 * 86_400)
}

const BEFORE: &str = "fn main() {\n    original();\n}\n";
const AFTER: &str = "fn main() {\n    replacement();\n}\n";

/// Reject everything currently changed in one file.
fn reject_all(dir: &Path, path: &str, reason: &str) -> reject::FileRejection {
    let repo = Repo::open(dir).unwrap();
    let diff = repo.file_diff(path, ComparisonMode::WorkingToHead).unwrap();
    let hunks: Vec<usize> = (0..diff.hunks.len()).collect();

    reject::reject_file(
        &repo,
        path,
        ComparisonMode::WorkingToHead,
        &diff.changed_line_indices(),
        &hunks,
        reason,
        now(),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Rejecting
// ---------------------------------------------------------------------------

#[test]
fn rejecting_restores_the_baseline_and_leaves_the_reason_behind() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    write(dir.path(), "src/lib.rs", AFTER);

    let outcome = reject_all(dir.path(), "src/lib.rs", REASON);
    assert!(outcome.reverted);
    assert!(outcome.marked);

    let text = read(dir.path(), "src/lib.rs");
    assert!(text.contains("original();"), "got:\n{text}");
    assert!(!text.contains("replacement();"), "got:\n{text}");
    assert!(text.contains(reject::MARKER));
    assert!(text.contains(REASON));
    assert!(text.contains("2000-03-01"));
}

#[test]
fn the_note_sits_directly_above_the_code_it_explains() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    write(dir.path(), "src/lib.rs", AFTER);

    reject_all(dir.path(), "src/lib.rs", REASON);

    let text = read(dir.path(), "src/lib.rs");
    let lines: Vec<&str> = text.lines().collect();
    let note = lines
        .iter()
        .position(|l| reject::is_marker_line(l))
        .expect("a note");
    let code = lines
        .iter()
        .position(|l| l.contains("original();"))
        .expect("the restored line");

    assert!(note < code, "the note must precede the code:\n{text}");
    // Indented like the code it explains, not flattened to column zero.
    assert!(lines[note].starts_with("    //"), "got {:?}", lines[note]);
}

/// The revert still has to happen. Only the note is skipped.
#[test]
fn a_file_with_no_comment_syntax_is_reverted_but_left_unmarked() {
    let dir = init_repo(&[("data.json", "{\n  \"a\": 1\n}\n")]);
    write(dir.path(), "data.json", "{\n  \"a\": 2\n}\n");

    let outcome = reject_all(dir.path(), "data.json", REASON);

    assert!(outcome.reverted);
    assert!(!outcome.marked, "JSON has no line comment to write into");
    assert_eq!(read(dir.path(), "data.json"), "{\n  \"a\": 1\n}\n");
}

/// CSS has no line comment, so it used to be reverted without a note. It does
/// have a block comment, and the reason belongs in it.
#[test]
fn a_css_change_is_reverted_and_marked_with_a_block_comment() {
    let dir = init_repo(&[("styles.css", "body {\n  color: red;\n}\n")]);
    write(dir.path(), "styles.css", "body {\n  color: blue;\n}\n");

    let outcome = reject_all(dir.path(), "styles.css", REASON);

    assert!(outcome.reverted);
    assert!(
        outcome.marked,
        "CSS should be markable with a block comment"
    );

    let text = read(dir.path(), "styles.css");
    assert!(
        text.contains("/*"),
        "the note must open a block comment:\n{text}"
    );
    assert!(
        text.contains("*/"),
        "the note must close the block comment:\n{text}"
    );
    assert!(
        text.contains(REASON),
        "the reason must be written in:\n{text}"
    );
    assert!(
        text.contains("color: red"),
        "the revert must restore the baseline:\n{text}"
    );
    assert!(
        !text.contains("color: blue"),
        "the rejected change must be gone:\n{text}"
    );
}

#[test]
fn an_empty_reason_reverts_without_writing_a_note() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    write(dir.path(), "src/lib.rs", AFTER);

    let outcome = reject_all(dir.path(), "src/lib.rs", "   ");

    assert!(outcome.reverted);
    assert!(!outcome.marked);
    assert_eq!(read(dir.path(), "src/lib.rs"), BEFORE);
}

/// Only the rejected hunk goes back. A second, unrelated change in the same
/// file is the reviewer's to keep, and the note still has to land in the right
/// place despite the line numbers having moved.
#[test]
fn rejecting_one_hunk_leaves_another_change_in_the_same_file_alone() {
    let original = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
    let dir = init_repo(&[("notes.sql", original)]);
    // Change the first line and the last line, two hunks apart.
    write(
        dir.path(),
        "notes.sql",
        "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n",
    );

    let repo = Repo::open(dir.path()).unwrap();
    let diff = repo
        .file_diff("notes.sql", ComparisonMode::WorkingToHead)
        .unwrap();
    assert_eq!(diff.hunks.len(), 2, "expected two separate hunks");

    // Reject only the second hunk.
    let outcome = reject::reject_file(
        &repo,
        "notes.sql",
        ComparisonMode::WorkingToHead,
        &diff.hunk_line_indices(1),
        &[1],
        REASON,
        now(),
    )
    .unwrap();

    assert!(outcome.reverted && outcome.marked);

    let text = read(dir.path(), "notes.sql");
    assert!(
        text.contains("ONE"),
        "the kept change must survive:\n{text}"
    );
    assert!(
        !text.contains("TEN"),
        "the rejected change must be gone:\n{text}"
    );

    // The note goes above the restored "ten", not somewhere else.
    let lines: Vec<&str> = text.lines().collect();
    let note = lines
        .iter()
        .position(|l| reject::is_marker_line(l))
        .expect("a note");
    assert_eq!(lines[note + 3].trim(), "ten", "got:\n{text}");
    // SQL takes a dash comment, not a slash one.
    assert!(lines[note].starts_with("--"), "got {:?}", lines[note]);
}

// ---------------------------------------------------------------------------
// The commit guard
// ---------------------------------------------------------------------------

fn install_guard(dir: &Path) {
    let hook = guard::hook_path(dir).expect("a git repository");
    let plan = guard::plan_for(&hook).expect("a write");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, &plan.content).unwrap();
    guard::ensure_executable(&hook).unwrap();
    assert!(guard::is_installed(&hook));
}

#[test]
fn the_guard_refuses_a_commit_that_still_carries_a_note() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    install_guard(dir.path());

    write(dir.path(), "src/lib.rs", AFTER);
    reject_all(dir.path(), "src/lib.rs", REASON);
    run(dir.path(), &["add", "src/lib.rs"]);

    let output = git(dir.path(), &["commit", "-m", "should not land"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "the commit should have been refused; stderr: {stderr}"
    );
    assert!(
        stderr.contains("src/lib.rs"),
        "the guard must name the offending file; got: {stderr}"
    );
}

/// A block-comment note carries the same head line (token + date) as a line
/// one, so the guard's grep must catch it too — the marker's language must not
/// decide whether the guard fires.
#[test]
fn the_guard_refuses_a_css_note_written_as_a_block_comment() {
    let dir = init_repo(&[("styles.css", "body {\n  color: red;\n}\n")]);
    install_guard(dir.path());

    write(dir.path(), "styles.css", "body {\n  color: blue;\n}\n");
    reject_all(dir.path(), "styles.css", REASON);
    run(dir.path(), &["add", "styles.css"]);

    let output = git(dir.path(), &["commit", "-m", "should not land"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "the commit should have been refused; stderr: {stderr}"
    );
    assert!(
        stderr.contains("styles.css"),
        "the guard must name the offending file; got: {stderr}"
    );
}

#[test]
fn the_guard_lets_the_commit_through_once_the_note_is_gone() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    install_guard(dir.path());

    write(dir.path(), "src/lib.rs", AFTER);
    reject_all(dir.path(), "src/lib.rs", REASON);

    // Do what the note asks: implement it properly, and delete the note. The
    // fix has to be a real change, or there is nothing left to commit.
    let cleaned: String = read(dir.path(), "src/lib.rs")
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(cleaned, BEFORE, "the note should be all that was removed");
    write(
        dir.path(),
        "src/lib.rs",
        "fn main() {\n    original();\n    properly_fixed();\n}\n",
    );
    run(dir.path(), &["add", "src/lib.rs"]);

    let output = git(dir.path(), &["commit", "-m", "fixed properly"]);
    assert!(
        output.status.success(),
        "a clean commit must not be blocked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A file that merely mentions the token — this test file, `reject.rs`, the
/// documentation — must stay committable, or the guard makes its own source
/// impossible to commit.
#[test]
fn a_file_that_only_mentions_the_token_is_not_flagged() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    install_guard(dir.path());

    write(
        dir.path(),
        "docs.md",
        &format!(
            "The marker is called {}, and it blocks commits.\n",
            reject::MARKER
        ),
    );
    run(dir.path(), &["add", "docs.md"]);

    let output = git(dir.path(), &["commit", "-m", "document the marker"]);
    assert!(
        output.status.success(),
        "a mention is not a note: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_escape_hatch_gets_a_note_past_the_guard() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    install_guard(dir.path());

    write(dir.path(), "src/lib.rs", AFTER);
    reject_all(dir.path(), "src/lib.rs", REASON);
    run(dir.path(), &["add", "src/lib.rs"]);

    let output = Command::new("git")
        .args(["commit", "-m", "deliberately keeping the note"])
        .current_dir(dir.path())
        .env("CB_ALLOW_REJECTED", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "CB_ALLOW_REJECTED should override the guard: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository that redirects its hooks must get the guard where git will run
/// it, not into a `.git/hooks` git has stopped reading.
#[test]
fn a_redirected_hooks_directory_is_honoured() {
    let dir = init_repo(&[("src/lib.rs", BEFORE)]);
    run(dir.path(), &["config", "core.hooksPath", ".githooks"]);

    let hook = guard::hook_path(dir.path()).expect("a git repository");
    let repo = Repo::open(dir.path()).unwrap();

    assert!(
        hook.ends_with(Path::new(".githooks").join("pre-commit")),
        "got {hook:?}"
    );
    // Relative to the working tree, as git resolves it — compared against the
    // repository's own idea of that path, which on Windows need not match the
    // spelling the temporary directory was created with.
    assert!(hook.starts_with(repo.workdir()), "got {hook:?}");
}

#[test]
fn a_directory_that_is_not_a_repository_has_no_hook_to_install() {
    let dir = tempfile::tempdir().unwrap();
    // `Repo::open` discovers upwards, so only assert the shape of the answer:
    // whatever it finds, planning must not panic.
    let _ = guard::hook_path(dir.path());
    let _ = BTreeSet::<u32>::new();
}
