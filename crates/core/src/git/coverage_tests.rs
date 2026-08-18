//! Tests for the intent-coverage reverse pass and scorecard.
//! Included by `coverage.rs` under `#[cfg(test)]`.

use super::*;
use crate::git::attribution::{self, Options};
use crate::git::patch::{DiffLine, FileDiff, Hunk, LineOrigin};
use crate::intents::{IntentEdit, IntentLabel, IntentRecord, Intents, LabelSource, ProviderId};

// -- builders ---------------------------------------------------------------

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

fn label(turn: &str, text: &str, source: LabelSource) -> IntentLabel {
    IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: text.to_string(),
        paths: Vec::new(),
        anchor: None,
        source,
    }
}

fn build(diffs: &[FileDiff], intents: &Intents) -> IntentReview {
    let attributions = attribution::attribute(diffs, intents, Options::default());
    review(diffs, &attributions, intents)
}

// -- unfulfilled claims -----------------------------------------------------

/// A declared intent whose turn edited a file in this diff, but whose recorded
/// text is nowhere in the diff, is reported as unmatched.
#[test]
fn a_declared_claim_no_hunk_evidences_is_reported_unfulfilled() {
    // The diff changed something unrelated to what the turn recorded.
    let diff = simple("a.rs", &["+    let completely_unrelated_change = 1;"]);
    let intents = Intents {
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let the_recorded_edit_that_is_absent = 2;"],
            1,
        )],
        labels: vec![label("turn-1", "add retry to token refresh", LabelSource::Declared)],
    };

    let out = build(&[diff], &intents);

    assert_eq!(out.unfulfilled.len(), 1);
    assert_eq!(out.unfulfilled[0].turn_id, "turn-1");
    assert_eq!(out.unfulfilled[0].label, "add retry to token refresh");
    assert_eq!(out.scorecard.unmatched, 1);
    assert_eq!(out.scorecard.evidenced, 0);
    assert_eq!(out.scorecard.claims, 1);
}

/// Evidenced anywhere is evidenced: a claim matched in one file is not
/// unfulfilled just because another edit of the same turn did not match.
#[test]
fn a_claim_evidenced_in_any_file_is_not_unfulfilled() {
    let matched = simple("a.rs", &["+    let a_distinctive_matched_line = compute();"]);
    let unmatched = simple("b.rs", &["+    let something_else_entirely = 0;"]);
    let intents = Intents {
        records: vec![
            record(
                "turn-1",
                "a.rs",
                &["    let a_distinctive_matched_line = compute();"],
                1,
            ),
            record("turn-1", "b.rs", &["    let never_written_here = 0;"], 2),
        ],
        labels: vec![label("turn-1", "one intent, two files", LabelSource::Declared)],
    };

    let out = build(&[matched, unmatched], &intents);

    assert!(out.unfulfilled.is_empty());
    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.evidenced, 1);
    assert_eq!(out.scorecard.unmatched, 0);
}

/// A sentence mined from prose is a guess about which words belong to which
/// edit. Reporting it as an unfulfilled claim would compound one guess with a
/// stronger accusation, so inferred labels never become claims.
#[test]
fn an_inferred_label_is_never_reported_unfulfilled() {
    let diff = simple("a.rs", &["+    let unrelated = 1;"]);
    let intents = Intents {
        records: vec![record("turn-1", "a.rs", &["    let recorded_but_absent = 2;"], 1)],
        labels: vec![label(
            "turn-1",
            "the retry limit now comes from configuration",
            LabelSource::Inferred,
        )],
    };

    let out = build(&[diff], &intents);

    assert!(out.unfulfilled.is_empty());
    assert_eq!(out.scorecard.claims, 0);
}

/// A turn whose files are not in the current diff is history, not an unmatched
/// claim: its work may already be committed.
#[test]
fn a_turn_with_no_records_in_this_diff_is_not_in_play() {
    let diff = simple("a.rs", &["+    let a_change = 1;"]);
    let intents = Intents {
        records: vec![record("turn-1", "elsewhere.rs", &["    let x = 2;"], 1)],
        labels: vec![label("turn-1", "worked on another file", LabelSource::Declared)],
    };

    let out = build(&[diff], &intents);

    assert!(out.unfulfilled.is_empty());
    assert_eq!(out.scorecard.claims, 0);
}

/// The claim names the files the turn touched in this diff, so the reviewer
/// knows where to look.
#[test]
fn an_unfulfilled_claim_names_the_files_the_turn_touched() {
    let a = simple("a.rs", &["+    let unrelated_a = 1;"]);
    let b = simple("b.rs", &["+    let unrelated_b = 2;"]);
    let intents = Intents {
        records: vec![
            record("turn-1", "a.rs", &["    let recorded_absent_a = 9;"], 1),
            record("turn-1", "b.rs", &["    let recorded_absent_b = 9;"], 2),
        ],
        labels: vec![label("turn-1", "touched two files", LabelSource::Declared)],
    };

    let out = build(&[a, b], &intents);

    assert_eq!(out.unfulfilled.len(), 1);
    assert_eq!(out.unfulfilled[0].paths, vec!["a.rs", "b.rs"]);
}

// -- scorecard --------------------------------------------------------------

#[test]
fn the_scorecard_sums_unattributed_lines_across_the_tree() {
    // Two files, nothing recorded: every changed line is unattributed.
    let a = simple("a.rs", &["+one", "+two"]);
    let b = simple("b.rs", &["+three"]);
    let out = build(&[a, b], &Intents::default());

    assert_eq!(out.scorecard.unattributed_lines, 3);
    assert_eq!(out.scorecard.hunks, 2);
    assert_eq!(out.scorecard.attributed_hunks, 0);
}

#[test]
fn the_scorecard_counts_changed_hunks_not_files() {
    let mut index = 0;
    let two_hunks = FileDiff {
        path: "a.rs".into(),
        old_path: None,
        hunks: vec![hunk(&["+alpha"], &mut index), hunk(&["+beta"], &mut index)],
        is_binary: false,
    };
    let out = build(&[two_hunks], &Intents::default());

    assert_eq!(out.scorecard.hunks, 2);
}

/// The number the reviewer reads and the list they can open can never disagree.
#[test]
fn unmatched_always_equals_the_unfulfilled_list_length() {
    let a = simple("a.rs", &["+    let unrelated_a = 1;"]);
    let b = simple("b.rs", &["+    let unrelated_b = 2;"]);
    let intents = Intents {
        records: vec![
            record("turn-1", "a.rs", &["    let absent_a = 9;"], 1),
            record("turn-2", "b.rs", &["    let absent_b = 9;"], 2),
        ],
        labels: vec![
            label("turn-1", "first intent", LabelSource::Declared),
            label("turn-2", "second intent", LabelSource::Declared),
        ],
    };

    let out = build(&[a, b], &intents);

    assert_eq!(out.scorecard.unmatched as usize, out.unfulfilled.len());
    assert_eq!(out.scorecard.unmatched, 2);
}

#[test]
fn an_empty_tree_scores_all_zero() {
    let out = build(&[], &Intents::default());

    assert_eq!(out.scorecard.claims, 0);
    assert_eq!(out.scorecard.evidenced, 0);
    assert_eq!(out.scorecard.unmatched, 0);
    assert_eq!(out.scorecard.hunks, 0);
    assert_eq!(out.scorecard.attributed_hunks, 0);
    assert_eq!(out.scorecard.unattributed_lines, 0);
    assert!(out.groups.is_empty());
    assert!(out.unfulfilled.is_empty());
}

/// The forward join still runs: a matched declared intent produces both an
/// evidenced scorecard entry and an intent card.
#[test]
fn a_matched_declared_intent_is_evidenced_and_grouped() {
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
        labels: vec![label("turn-1", "add retry to token refresh", LabelSource::Declared)],
    };

    let out = build(&[diff], &intents);

    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.evidenced, 1);
    assert_eq!(out.scorecard.unmatched, 0);
    assert!(out.unfulfilled.is_empty());
    assert!(out.groups.iter().any(|g| g.kind == crate::git::grouping::GroupKind::Intent));
}

// -- the IPC contract -------------------------------------------------------

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
fn an_intent_review_serialises_with_the_keys_the_ui_reads() {
    let out = IntentReview {
        groups: Vec::new(),
        unfulfilled: Vec::new(),
        scorecard: Scorecard::default(),
    };

    assert_eq!(
        keys(&serde_json::to_value(&out).unwrap()),
        ["groups", "scorecard", "unfulfilled"]
    );
}

#[test]
fn an_unfulfilled_claim_serialises_with_the_keys_the_ui_reads() {
    let claim = UnfulfilledClaim {
        turn_id: "turn-1".into(),
        label: "add retry".into(),
        provider: ProviderId::ClaudeCode,
        paths: vec!["a.rs".into()],
    };

    assert_eq!(
        keys(&serde_json::to_value(&claim).unwrap()),
        ["label", "paths", "provider", "turnId"]
    );
}

#[test]
fn a_scorecard_serialises_with_the_keys_the_ui_reads() {
    assert_eq!(
        keys(&serde_json::to_value(Scorecard::default()).unwrap()),
        [
            "attributedHunks",
            "claims",
            "evidenced",
            "hunks",
            "unattributedLines",
            "unmatched"
        ]
    );
}
