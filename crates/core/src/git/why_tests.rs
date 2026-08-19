//! Tests for the pure durable-why build and resolve logic.
//! Included by `why.rs` under `#[cfg(test)]`.

use super::*;
use crate::git::attribution::{self, Options};
use crate::git::patch::{DiffLine, FileDiff, Hunk, LineOrigin};
use crate::intents::{
    IntentEdit, IntentLabel, IntentPrompt, IntentRecord, Intents, LabelSource, ProviderId,
};

fn hunk(lines: &[&str], start: &mut u32) -> Hunk {
    let mut built = Vec::new();
    let mut old_no = 1;
    let mut new_no = 1;
    for line in lines {
        let (origin, content) = match line.chars().next() {
            Some('+') => (LineOrigin::Addition, &line[1..]),
            Some('-') => (LineOrigin::Deletion, &line[1..]),
            _ => (LineOrigin::Context, line.trim_start_matches(' ')),
        };
        built.push(DiffLine {
            index: *start,
            origin,
            content: content.to_string(),
            old_lineno: (origin != LineOrigin::Addition).then(|| {
                old_no += 1;
                old_no - 1
            }),
            new_lineno: (origin != LineOrigin::Deletion).then(|| {
                new_no += 1;
                new_no - 1
            }),
            no_newline: false,
        });
        *start += 1;
    }
    Hunk {
        old_start: 1,
        old_lines: old_no - 1,
        new_start: 1,
        new_lines: new_no - 1,
        header: String::new(),
        lines: built,
    }
}

fn simple(path: &str, lines: &[&str]) -> FileDiff {
    let mut index = 0;
    FileDiff {
        path: path.to_string(),
        old_path: None,
        hunks: vec![hunk(lines, &mut index)],
        is_binary: false,
    }
}

fn record(turn: &str, path: &str, lines: &[&str], seq: u64) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        tool_use_id: format!("tool-{seq}"),
        seq,
        path: path.to_string(),
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
    }
}

fn build(diffs: &[FileDiff], intents: &Intents) -> DurableWhy {
    build_with_prompts(diffs, intents, &[])
}

fn build_with_prompts(
    diffs: &[FileDiff],
    intents: &Intents,
    prompts: &[IntentPrompt],
) -> DurableWhy {
    let attributions = attribution::attribute(diffs, intents, Options::default());
    build_for_commit(diffs, &attributions, prompts)
}

#[test]
fn a_committed_line_keys_by_content_and_resolves_to_its_reason() {
    let diff = simple(
        "a.rs",
        &["+    let retry_limit = read_configured_retry_limit();"],
    );
    let intents = Intents {
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let retry_limit = read_configured_retry_limit();"],
            1,
        )],
        labels: vec![declared("turn-1", "add retry to token refresh")],
    };

    let why = build(&[diff], &intents);
    let file = file_in(&why, "a.rs").expect("a.rs mapped");

    // Resolve against the committed blob content.
    let blob = "fn refresh() {\n    let retry_limit = read_configured_retry_limit();\n}\n";
    let resolved = resolve_lines(file, blob);

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].line, 2);
    assert_eq!(
        resolved[0].label.as_deref(),
        Some("add retry to token refresh")
    );
    assert_eq!(resolved[0].turn_id, "turn-1");
}

/// The whole point: reformatting before commit still resolves, because the key
/// is the whitespace-insensitive skeleton, not the exact text.
#[test]
fn reformatting_before_commit_still_resolves() {
    let diff = simple("a.rs", &["+    let x = compute(a, b);"]);
    let intents = Intents {
        records: vec![record("turn-1", "a.rs", &["    let x = compute(a, b);"], 1)],
        labels: vec![declared("turn-1", "compute x")],
    };

    let why = build(&[diff], &intents);
    let file = file_in(&why, "a.rs").expect("mapped");

    // Reindented and re-spaced in the committed blob.
    let blob = "        let x = compute( a , b );\n";
    let resolved = resolve_lines(file, blob);

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].label.as_deref(), Some("compute x"));
}

#[test]
fn a_line_no_record_claims_resolves_to_nothing() {
    let diff = simple("a.rs", &["+    let a_distinctive_recorded_line = 1;"]);
    let intents = Intents {
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let a_distinctive_recorded_line = 1;"],
            1,
        )],
        labels: vec![declared("turn-1", "recorded")],
    };

    let why = build(&[diff], &intents);
    let file = file_in(&why, "a.rs").expect("mapped");

    let blob = "let something_nobody_recorded = 99;\n";
    assert!(resolve_lines(file, blob).is_empty());
}

/// A bare `}` is the commonest line in the file: it must never key anything.
#[test]
fn a_generic_line_is_never_keyed() {
    let diff = simple("a.rs", &["+}"]);
    let intents = Intents {
        records: vec![record("turn-1", "a.rs", &["}"], 1)],
        labels: vec![declared("turn-1", "close block")],
    };

    let why = build(&[diff], &intents);
    // Nothing anchorable ⇒ no file entry at all.
    assert!(why.is_empty());
}

/// A content key two different turns produced is ambiguous and dropped.
#[test]
fn an_ambiguous_key_across_turns_is_dropped() {
    let line = "    logger.info(\"starting the worker process now\");";
    let mut index = 0;
    let diff = FileDiff {
        path: "a.rs".into(),
        old_path: None,
        // The same distinctive line added in two separate hunks.
        hunks: vec![
            hunk(&[&format!("+{line}")], &mut index),
            hunk(&[&format!("+{line}")], &mut index),
        ],
        is_binary: false,
    };
    let intents = Intents {
        records: vec![
            record("turn-1", "a.rs", &[line], 1),
            record("turn-2", "a.rs", &[line], 2),
        ],
        labels: vec![declared("turn-1", "first"), declared("turn-2", "second")],
    };

    let why = build(&[diff], &intents);

    // The key is shared by two turns, so it is abstained on entirely.
    assert!(
        why.is_empty(),
        "an ambiguous key must not resolve to either turn"
    );
}

#[test]
fn content_hash_is_stable_and_deterministic() {
    assert_eq!(
        content_hash("letx=compute(a,b)"),
        content_hash("letx=compute(a,b)")
    );
    assert_ne!(content_hash("alpha"), content_hash("beta"));
}

// -- the IPC / persistence contract -----------------------------------------

fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn a_why_entry_serialises_with_the_keys_the_reader_expects() {
    let entry = WhyEntry {
        content_hash: "abc".into(),
        turn_id: "turn-1".into(),
        label: Some("add retry".into()),
        label_source: Some(LabelSource::Declared),
        prompt: Some("the original request".into()),
        seq: 1,
        confidence: Confidence::High,
    };

    assert_eq!(
        keys(&serde_json::to_value(&entry).unwrap()),
        [
            "confidence",
            "contentHash",
            "label",
            "labelSource",
            "prompt",
            "seq",
            "turnId"
        ]
    );
}

/// A recorded prompt for the turn is threaded onto the line's intent.
#[test]
fn a_recorded_prompt_is_threaded_onto_the_line() {
    let diff = simple(
        "a.rs",
        &["+    let retry_limit = read_configured_retry_limit();"],
    );
    let intents = Intents {
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let retry_limit = read_configured_retry_limit();"],
            1,
        )],
        labels: vec![declared("turn-1", "add retry")],
    };
    let prompts = vec![IntentPrompt {
        provider: ProviderId::ClaudeCode,
        turn_id: "turn-1".into(),
        prompt: "add exponential backoff, cap at 5".into(),
    }];

    let why = build_with_prompts(&[diff], &intents, &prompts);
    let file = file_in(&why, "a.rs").expect("mapped");
    let resolved = resolve_lines(
        file,
        "fn refresh() {\n    let retry_limit = read_configured_retry_limit();\n}\n",
    );

    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].prompt.as_deref(),
        Some("add exponential backoff, cap at 5")
    );
    // A turn with no recorded prompt threads nothing.
    let bare = build_with_prompts(
        &[simple("b.rs", &["+    let x = go();"])],
        &intents,
        &prompts,
    );
    assert!(file_in(&bare, "b.rs").is_none());
}

#[test]
fn a_line_intent_serialises_with_the_keys_the_ui_reads() {
    let intent = LineIntent {
        line: 3,
        label: Some("add retry".into()),
        label_source: Some(LabelSource::Declared),
        turn_id: "turn-1".into(),
        confidence: Confidence::Medium,
        prompt: None,
    };

    // `prompt` is absent (None) — the reader treats missing as "not captured".
    assert_eq!(
        keys(&serde_json::to_value(&intent).unwrap()),
        ["confidence", "label", "labelSource", "line", "turnId"]
    );
}
