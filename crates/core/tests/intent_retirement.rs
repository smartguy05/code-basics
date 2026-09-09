//! End-to-end tests for retiring intents a commit has absorbed.
//!
//! The bug these pin: an intent record is content-matched against *every* future
//! diff for its path, forever, so a reason recorded for work that was committed
//! weeks ago re-titles a card the moment that text reappears.

use std::path::Path;
use std::process::Command;

use cb_core::git::attribution::{self, Options};
use cb_core::git::grouping;
use cb_core::git::repo::ComparisonMode;
use cb_core::git::Repo;
use cb_core::intents::{
    self, retire, IntentEdit, IntentLabel, IntentRecord, LabelSource, LoadOptions, ProviderId,
};

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
    std::fs::write(dir.join(name), contents).unwrap();
}

fn record(turn: &str, tool: &str, seq: u64, path: &str, lines: &[&str]) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        tool_use_id: tool.into(),
        seq,
        path: path.into(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: lines.iter().map(|s| s.to_string()).collect(),
            whole_file: false,
        },
        branch: None,
    }
}

fn declared(turn: &str, text: &str) -> IntentLabel {
    IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: text.into(),
        paths: Vec::new(),
        anchor: None,
        source: LabelSource::Declared,
        self_confidence: None,
    }
}

/// The four distinctive lines a turn is recorded as having written.
const BLOCK: [&str; 4] = [
    "    let retry_budget = compute_retry_budget(config);",
    "    let backoff = ExponentialBackoff::new(retry_budget);",
    "    let deadline = clock.now() + backoff.total_window();",
    "    tracing::debug!(?retry_budget, ?deadline, \"retry policy resolved\");",
];

fn block() -> String {
    BLOCK.join("\n")
}

/// Every card label in the working tree, for asserting a reason is or is not
/// shown. Mirrors what the `intent_groups` command does.
fn card_labels(root: &Path) -> Vec<String> {
    let repo = Repo::open(root).unwrap();
    let diffs = repo.diff_all(ComparisonMode::WorkingToHead).unwrap();
    let branch = repo.status().ok().and_then(|s| s.branch);
    let intents = intents::load(root, &LoadOptions { branch }).unwrap();
    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    grouping::group(&diffs, &attributions, &intents)
        .into_iter()
        .map(|g| g.label)
        .collect()
}

/// What the Changes tab does on every refresh: notice a moved HEAD, then read.
fn refresh(root: &Path) -> Vec<String> {
    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();
    card_labels(root)
}

/// The reproduction. A reason recorded for work that has since been committed
/// must not label a later, unrelated change that reintroduces the same text.
#[test]
fn a_committed_edits_intent_still_labels_an_unrelated_later_change() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    // The app is open, so the prune has a baseline to notice movement against.
    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();

    // Turn 1 writes a distinctive block, and says why.
    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    intents::append_label(root, &declared("turn-1", "add the retry budget")).unwrap();

    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "add retry budget"]);

    // That work is now history. Later, unrelated work copies the same pattern
    // into a second function — the text reappears as an *addition*, carrying no
    // intent of its own.
    write(
        root,
        "a.rs",
        &format!("fn main() {{\n{body}\n}}\n\nfn setup() {{\n{body}\n}}\n"),
    );

    let labels = refresh(root);
    assert!(
        !labels.iter().any(|l| l == "add the retry budget"),
        "an intent for an already-committed edit is still labelling the working tree: {labels:?}"
    );
}

#[test]
fn committing_a_card_retires_its_records_into_the_archive() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();

    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    intents::append_label(root, &declared("turn-1", "add the retry budget")).unwrap();
    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "add retry budget"]);

    let summary = retire::run_if_head_moved(&repo, root).unwrap();
    assert!(summary.pruned);
    assert_eq!(summary.records_retired, 1);

    // Gone from the live store...
    let left = intents::load(root, &LoadOptions::default()).unwrap();
    assert!(left.records.is_empty(), "records: {:?}", left.records);
    assert!(left.labels.is_empty(), "labels: {:?}", left.labels);

    // ...but not gone. Nothing here is destroyed.
    let archive = std::fs::read_to_string(retire::archive_path(root)).unwrap();
    assert!(archive.contains("compute_retry_budget"));
    let tombs = retire::load_tombstones(root);
    assert_eq!(tombs.len(), 1);
}

/// The one that must never regress: line-level partial staging is a first-class
/// feature, so a half-committed card keeps the label for what is left.
#[test]
fn a_partially_committed_card_keeps_its_intent() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();

    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    intents::append_label(root, &declared("turn-1", "add the retry budget")).unwrap();

    // Only the first half of the block is committed.
    let committed = BLOCK[..2].join("\n");
    write(root, "a.rs", &format!("fn main() {{\n{committed}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "half of it"]);

    // The rest is still sitting in the working tree.
    let whole = block();
    write(root, "a.rs", &format!("fn main() {{\n{whole}\n}}\n"));

    let summary = retire::run_if_head_moved(&repo, root).unwrap();
    assert_eq!(
        summary.records_retired, 0,
        "a record with uncommitted lines left was retired"
    );

    let left = intents::load(root, &LoadOptions::default()).unwrap();
    assert_eq!(left.records.len(), 1);
    assert!(
        card_labels(root)
            .iter()
            .any(|l| l == "add the retry budget"),
        "the remaining hunk lost its label"
    );
}

#[test]
fn a_commit_made_outside_the_app_is_caught_on_the_next_refresh() {
    // No hook fires for a commit typed in a floating terminal, so the prune has
    // to notice HEAD moving on its own.
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();

    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "committed from a shell"]);

    let summary = retire::run_if_head_moved(&repo, root).unwrap();
    assert_eq!(summary.records_retired, 1);
}

#[test]
fn an_amend_moves_head_and_prunes() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();

    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "wip"]);
    run(root, &["commit", "--amend", "-m", "add retry budget"]);

    let summary = retire::run_if_head_moved(&repo, root).unwrap();
    assert_eq!(summary.records_retired, 1);
}

#[test]
fn the_first_look_conservatively_cleans_an_absorbed_backlog() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "add retry budget"]);

    let repo = Repo::open(root).unwrap();
    let summary = retire::run_if_head_moved(&repo, root).unwrap();
    assert!(summary.pruned);
    assert_eq!(summary.records_retired, 1);
    assert_eq!(
        intents::load(root, &LoadOptions::default())
            .unwrap()
            .records
            .len(),
        0
    );

    let done = retire::run_now(&repo, root).unwrap();
    assert_eq!(done.records_retired, 0);
    assert!(intents::load(root, &LoadOptions::default())
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn an_unchanged_head_prunes_nothing() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();
    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();

    let summary = retire::run_if_head_moved(&repo, root).unwrap();
    assert!(!summary.pruned);
    assert_eq!(
        intents::load(root, &LoadOptions::default())
            .unwrap()
            .records
            .len(),
        1
    );
}

/// The easily-missed regression: `next_seq` is max+1 over the file, so a prune
/// that lowered the maximum would hand out numbers that collide with survivors
/// and break attribution's "later edits win" rule.
#[test]
fn pruning_never_lowers_the_next_sequence_number() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();

    intents::append_edit(root, &record("turn-1", "tool-1:0", 40, "a.rs", &BLOCK)).unwrap();
    let before = intents::next_seq(root);
    assert_eq!(before, 41);

    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "add retry budget"]);
    retire::run_if_head_moved(&repo, root).unwrap();

    assert!(
        intents::next_seq(root) >= before,
        "the prune lowered the sequence high-water mark"
    );
}

#[test]
fn clearing_history_forgets_the_tombstones_too() {
    // Otherwise a clear-then-import would silently return nothing.
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let root = dir.path();
    let body = block();

    let repo = Repo::open(root).unwrap();
    retire::run_if_head_moved(&repo, root).unwrap();
    intents::append_edit(root, &record("turn-1", "tool-1:0", 1, "a.rs", &BLOCK)).unwrap();
    write(root, "a.rs", &format!("fn main() {{\n{body}\n}}\n"));
    run(root, &["add", "."]);
    run(root, &["commit", "-m", "add retry budget"]);
    retire::run_if_head_moved(&repo, root).unwrap();
    assert_eq!(retire::load_tombstones(root).len(), 1);

    intents::clear(root).unwrap();
    assert!(retire::load_tombstones(root).is_empty());
}
