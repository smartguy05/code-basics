//! End-to-end tests for durable intent: writing a git note at commit and
//! reading a committed line back to its reason.

use std::path::Path;
use std::process::Command;

use cb_core::git::attribution::{self, Options};
use cb_core::git::why::build_for_commit;
use cb_core::git::Repo;
use cb_core::intents::{IntentEdit, IntentLabel, IntentRecord, Intents, LabelSource, ProviderId};

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

fn record(turn: &str, path: &str, lines: &[&str]) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        tool_use_id: "tool-1".into(),
        seq: 1,
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

#[test]
fn a_committed_line_maps_back_to_its_intent_through_a_note() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let path = dir.path();
    let repo = Repo::open(path).unwrap();

    let changed = "fn main() {\n    let retry_limit = read_configured_retry_limit();\n}\n";
    write(path, "a.rs", changed);
    run(path, &["add", "."]);

    let oid = repo.commit("add retry", false).unwrap();

    // Build the durable mapping from the commit's own diff and hand-made intent.
    let diffs = repo.commit_diff(&oid).unwrap();
    let intents = Intents {
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let retry_limit = read_configured_retry_limit();"],
        )],
        labels: vec![declared("turn-1", "add retry to token refresh")],
    };
    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    let why = build_for_commit(&diffs, &attributions, &[]);

    repo.write_why_note(&oid, &why).unwrap();

    // Read it back — the added line resolves to its stated reason.
    let resolved = repo.why_for_file(&oid, "a.rs").unwrap();
    assert!(
        resolved
            .iter()
            .any(|l| l.label.as_deref() == Some("add retry to token refresh")),
        "expected the committed line to resolve to its intent, got {resolved:?}"
    );

    // A line nobody recorded resolves to nothing: `fn main()` is not attributed.
    assert!(
        resolved.iter().all(|l| l.line != 1),
        "the unrecorded first line must not carry an intent"
    );
}

#[test]
fn writing_a_note_twice_for_a_commit_is_idempotent() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let path = dir.path();
    let repo = Repo::open(path).unwrap();

    write(
        path,
        "a.rs",
        "fn main() {\n    let a_distinctive_line = compute_it();\n}\n",
    );
    run(path, &["add", "."]);
    let oid = repo.commit("change", false).unwrap();

    let diffs = repo.commit_diff(&oid).unwrap();
    let intents = Intents {
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let a_distinctive_line = compute_it();"],
        )],
        labels: vec![declared("turn-1", "compute it")],
    };
    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    let why = build_for_commit(&diffs, &attributions, &[]);

    repo.write_why_note(&oid, &why).unwrap();
    // A second write (force) must not error.
    repo.write_why_note(&oid, &why).unwrap();

    let resolved = repo.why_for_file(&oid, "a.rs").unwrap();
    assert_eq!(resolved.len(), 1);
}

/// The full mining→commit→read path: a prompt recorded on disk is loaded by
/// `record_note` and threaded onto the committed line.
#[test]
fn record_note_threads_the_recorded_prompt_onto_the_line() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let path = dir.path();
    let repo = Repo::open(path).unwrap();

    let line = "    let retry_limit = read_configured_retry_limit();";
    cb_core::intents::append_edit(path, &record("turn-1", "a.rs", &[line])).unwrap();
    cb_core::intents::append_label(path, &declared("turn-1", "add retry")).unwrap();
    cb_core::intents::append_prompt(
        path,
        &cb_core::intents::IntentPrompt {
            provider: ProviderId::ClaudeCode,
            turn_id: "turn-1".into(),
            prompt: "add exponential backoff, cap at 5".into(),
        },
    )
    .unwrap();

    write(path, "a.rs", &format!("fn main() {{\n{line}\n}}\n"));
    run(path, &["add", "."]);
    let oid = repo.commit("add retry", false).unwrap();

    cb_core::git::why::record_note(&repo, path, &oid).unwrap();

    let resolved = repo.why_for_file(&oid, "a.rs").unwrap();
    assert!(
        resolved
            .iter()
            .any(|l| l.prompt.as_deref() == Some("add exponential backoff, cap at 5")),
        "expected the recorded prompt on the line, got {resolved:?}"
    );
}

#[test]
fn a_commit_with_no_note_resolves_to_nothing() {
    let dir = init_repo(&[("a.rs", "fn main() {}\n")]);
    let path = dir.path();
    let repo = Repo::open(path).unwrap();

    let head = repo.history(1).unwrap();
    let id = &head[0].id;

    assert!(repo.read_why_note(id).unwrap().is_none());
    assert!(repo.why_for_file(id, "a.rs").unwrap().is_empty());
}
