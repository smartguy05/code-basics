//! Tests for the intent-coverage reverse pass and scorecard.
//! Included by `coverage.rs` under `#[cfg(test)]`.

use super::*;
use crate::git::attribution::{self, Confidence, Options};
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

/// A path-scoped label — the shape a declared `Intent(paths): …` line records.
fn plabel(turn: &str, text: &str, paths: &[&str], source: LabelSource) -> IntentLabel {
    IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: text.to_string(),
        paths: paths.iter().map(|s| s.to_string()).collect(),
        anchor: None,
        source,
    }
}

use crate::git::grouping::GroupKind;

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
        labels: vec![label(
            "turn-1",
            "add retry to token refresh",
            LabelSource::Declared,
        )],
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
    let matched = simple(
        "a.rs",
        &["+    let a_distinctive_matched_line = compute();"],
    );
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
        labels: vec![label(
            "turn-1",
            "one intent, two files",
            LabelSource::Declared,
        )],
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
        records: vec![record(
            "turn-1",
            "a.rs",
            &["    let recorded_but_absent = 2;"],
            1,
        )],
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
        labels: vec![label(
            "turn-1",
            "worked on another file",
            LabelSource::Declared,
        )],
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
        labels: vec![label(
            "turn-1",
            "add retry to token refresh",
            LabelSource::Declared,
        )],
    };

    let out = build(&[diff], &intents);

    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.evidenced, 1);
    assert_eq!(out.scorecard.unmatched, 0);
    assert!(out.unfulfilled.is_empty());
    assert!(out
        .groups
        .iter()
        .any(|g| g.kind == crate::git::grouping::GroupKind::Intent));
}

// -- cross-turn intent binding ----------------------------------------------

/// The workflow/subagent case: geometry recorded under one turn with no reason,
/// the reason declared under a later turn, correctly path-scoped. The declared
/// label must bind across turns to the geometry in its named file, coherently
/// across both the card and the scorecard.
#[test]
fn a_path_scoped_label_binds_across_turns_to_geometry_in_its_file() {
    let lines = [
        "    let a_distinctive_bound_alpha = compute_alpha();",
        "    let a_distinctive_bound_beta = compute_beta();",
        "    let a_distinctive_bound_gamma = compute_gamma();",
    ];
    let added: Vec<String> = lines.iter().map(|l| format!("+{l}")).collect();
    let diff = simple(
        "f.rs",
        &added.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let intents = Intents {
        records: vec![record("turnA", "f.rs", &lines, 1)],
        labels: vec![plabel(
            "turnB",
            "the declared reason",
            &["f.rs"],
            LabelSource::Declared,
        )],
    };

    let out = build(&[diff], &intents);

    assert!(
        out.groups
            .iter()
            .any(|g| g.kind == GroupKind::Intent && g.label == "the declared reason"),
        "expected an Intent card titled from the cross-turn label, got {:?}",
        out.groups
            .iter()
            .map(|g| (&g.kind, &g.label))
            .collect::<Vec<_>>()
    );
    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.evidenced, 1);
    assert_eq!(out.scorecard.unmatched, 0);
    assert!(out.unfulfilled.is_empty());
    // A path-scoped reason is author-declared for these files, so its card keeps
    // the geometry's confidence — it is not downgraded like the bare bridge.
    let card = out
        .groups
        .iter()
        .find(|g| g.kind == GroupKind::Intent)
        .expect("intent card");
    assert_ne!(card.confidence, Confidence::Low);
}

/// The exact screenshot scenario: one orphan geometry turn spanning five files,
/// covered by two differently-scoped declared labels from a later turn. It must
/// split into two distinct Intent cards, both evidenced, nothing unmatched.
#[test]
fn one_orphan_geometry_turn_splits_into_two_declared_intent_cards() {
    let d1 = simple(
        "review.rs",
        &["+    let a_distinctive_review_line = one_call();"],
    );
    let d2 = simple(
        "review_tests.rs",
        &["+    let a_distinctive_review_test = two_call();"],
    );
    let d3 = simple(
        "commands/review.rs",
        &["+    let a_distinctive_command_line = three_call();"],
    );
    let d4 = simple(
        "src/components/x.ts",
        &["+    const aDistinctiveComponentLine = four_call();"],
    );
    let d5 = simple(
        "styles.css",
        &["+    .a-distinctive-selector { color: rebeccapurple; }"],
    );

    let intents = Intents {
        records: vec![
            record(
                "turnA",
                "review.rs",
                &["    let a_distinctive_review_line = one_call();"],
                1,
            ),
            record(
                "turnA",
                "review_tests.rs",
                &["    let a_distinctive_review_test = two_call();"],
                2,
            ),
            record(
                "turnA",
                "commands/review.rs",
                &["    let a_distinctive_command_line = three_call();"],
                3,
            ),
            record(
                "turnA",
                "src/components/x.ts",
                &["    const aDistinctiveComponentLine = four_call();"],
                4,
            ),
            record(
                "turnA",
                "styles.css",
                &["    .a-distinctive-selector { color: rebeccapurple; }"],
                5,
            ),
        ],
        labels: vec![
            plabel(
                "turnB",
                "Codex model selection",
                &["review.rs", "review_tests.rs", "commands/review.rs"],
                LabelSource::Declared,
            ),
            plabel(
                "turnB",
                "Draggable resizable review panel",
                &["src/components", "styles.css"],
                LabelSource::Declared,
            ),
        ],
    };

    let out = build(&[d1, d2, d3, d4, d5], &intents);

    let intent_labels: Vec<&str> = out
        .groups
        .iter()
        .filter(|g| g.kind == GroupKind::Intent)
        .map(|g| g.label.as_str())
        .collect();

    assert!(
        intent_labels.contains(&"Codex model selection"),
        "got {intent_labels:?}"
    );
    assert!(
        intent_labels.contains(&"Draggable resizable review panel"),
        "got {intent_labels:?}"
    );
    assert_eq!(intent_labels.len(), 2, "got {intent_labels:?}");
    assert_eq!(out.scorecard.claims, 2);
    assert_eq!(out.scorecard.evidenced, 2);
    assert_eq!(out.scorecard.unmatched, 0);
    assert!(out.unfulfilled.is_empty());
}

/// Two declared labels from different turns both scope the same file: the bind
/// is ambiguous, so nothing is mislabeled. The geometry stays a same-turn card
/// and both reasons remain honest, unevidenced claims rather than a guess.
#[test]
fn two_declared_labels_covering_one_file_do_not_cross_turn_bind() {
    let diff = simple(
        "f.rs",
        &["+    let an_ambiguous_distinctive_line = go_now();"],
    );
    let intents = Intents {
        records: vec![record(
            "turnA",
            "f.rs",
            &["    let an_ambiguous_distinctive_line = go_now();"],
            1,
        )],
        labels: vec![
            plabel("turnB", "first reason", &["f.rs"], LabelSource::Declared),
            plabel("turnC", "second reason", &["f.rs"], LabelSource::Declared),
        ],
    };

    let out = build(&[diff], &intents);

    assert!(
        !out.groups.iter().any(|g| g.kind == GroupKind::Intent),
        "no reason should title an Intent card when the bind is ambiguous"
    );
    assert!(out.groups.iter().any(|g| g.kind == GroupKind::SameTurn));
    assert_eq!(out.scorecard.evidenced, 0);
    assert_eq!(out.scorecard.claims, 2);
    assert_eq!(out.scorecard.unmatched, 2);
}

/// A single bare declared label (no paths) binds to the one orphan geometry
/// turn in the diff, titling one Intent card that spans its files.
#[test]
fn a_single_bare_declared_label_binds_to_the_single_orphan_turn() {
    let alpha = [
        "    let a_distinctive_alpha_one = alpha_one();",
        "    let a_distinctive_alpha_two = alpha_two();",
        "    let a_distinctive_alpha_three = alpha_three();",
    ];
    let beta = [
        "    let a_distinctive_beta_one = beta_one();",
        "    let a_distinctive_beta_two = beta_two();",
        "    let a_distinctive_beta_three = beta_three();",
    ];
    let a_added: Vec<String> = alpha.iter().map(|l| format!("+{l}")).collect();
    let b_added: Vec<String> = beta.iter().map(|l| format!("+{l}")).collect();
    let d1 = simple(
        "a.rs",
        &a_added.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let d2 = simple(
        "b.rs",
        &b_added.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let intents = Intents {
        records: vec![
            record("turnA", "a.rs", &alpha, 1),
            record("turnA", "b.rs", &beta, 2),
        ],
        labels: vec![label(
            "turnB",
            "the bare declared reason",
            LabelSource::Declared,
        )],
    };

    let out = build(&[d1, d2], &intents);

    let intent: Vec<_> = out
        .groups
        .iter()
        .filter(|g| g.kind == GroupKind::Intent)
        .collect();
    assert_eq!(
        intent.len(),
        1,
        "got {:?}",
        out.groups
            .iter()
            .map(|g| (&g.kind, &g.label))
            .collect::<Vec<_>>()
    );
    assert_eq!(intent[0].label, "the bare declared reason");
    assert_eq!(intent[0].files.len(), 2);
    // The bare bridge rests on count-uniqueness alone, so the card is flagged
    // low-confidence — shown, but as a heuristic association, not asserted.
    assert_eq!(intent[0].confidence, Confidence::Low);
    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.evidenced, 1);
    assert_eq!(out.scorecard.unmatched, 0);
}

/// Two orphan geometry turns are present, so a bare label cannot say which it
/// belongs to. It abstains, and both turns stay their own untitled cards.
#[test]
fn a_bare_label_abstains_when_two_orphan_turns_are_present() {
    let d1 = simple("a.rs", &["+    let a_distinctive_alpha = alpha_call();"]);
    let d2 = simple("b.rs", &["+    let a_distinctive_beta = beta_call();"]);
    let intents = Intents {
        records: vec![
            record(
                "turnA1",
                "a.rs",
                &["    let a_distinctive_alpha = alpha_call();"],
                1,
            ),
            record(
                "turnA2",
                "b.rs",
                &["    let a_distinctive_beta = beta_call();"],
                2,
            ),
        ],
        labels: vec![label("turnB", "the bare reason", LabelSource::Declared)],
    };

    let out = build(&[d1, d2], &intents);

    assert!(!out.groups.iter().any(|g| g.kind == GroupKind::Intent));
    assert_eq!(
        out.groups
            .iter()
            .filter(|g| g.kind == GroupKind::SameTurn)
            .count(),
        2
    );
    assert_eq!(out.scorecard.claims, 0);
    assert_eq!(out.scorecard.evidenced, 0);
}

/// One orphan turn, but two candidate bare reasons: which one the edits belong
/// to is unknowable, so the bare-bridge abstains rather than guess. (Guards the
/// `candidates.next().is_some()` branch.)
#[test]
fn a_bare_label_abstains_when_two_candidate_reasons_exist() {
    let d1 = simple("a.rs", &["+    let a_distinctive_alpha = alpha_call();"]);
    let d2 = simple("b.rs", &["+    let a_distinctive_beta = beta_call();"]);
    let intents = Intents {
        records: vec![
            record(
                "turnA",
                "a.rs",
                &["    let a_distinctive_alpha = alpha_call();"],
                1,
            ),
            record(
                "turnA",
                "b.rs",
                &["    let a_distinctive_beta = beta_call();"],
                2,
            ),
        ],
        labels: vec![
            label("turnB", "the first bare reason", LabelSource::Declared),
            label("turnC", "the second bare reason", LabelSource::Declared),
        ],
    };

    let out = build(&[d1, d2], &intents);

    assert!(!out.groups.iter().any(|g| g.kind == GroupKind::Intent));
    assert_eq!(out.scorecard.claims, 0);
    assert_eq!(out.scorecard.evidenced, 0);
}

/// A bare reason whose own turn made edits in this diff is spent on that turn
/// (the same-turn fallback), so it must not *also* be stamped onto an unrelated
/// orphan turn — the `!already` guard. The bound card stays scoped to its own
/// file, and the orphan turn is left unclaimed.
#[test]
fn a_bare_label_spent_on_its_own_turn_does_not_also_bind_an_orphan() {
    let d1 = simple("a.rs", &["+    let a_distinctive_alpha = alpha_call();"]);
    let d2 = simple("b.rs", &["+    let a_distinctive_beta = beta_call();"]);
    let intents = Intents {
        records: vec![
            // Orphan geometry: turnA, no reason of its own.
            record(
                "turnA",
                "a.rs",
                &["    let a_distinctive_alpha = alpha_call();"],
                1,
            ),
            // turnB made b.rs and carries its own bare reason.
            record(
                "turnB",
                "b.rs",
                &["    let a_distinctive_beta = beta_call();"],
                2,
            ),
        ],
        labels: vec![label("turnB", "turn B's own reason", LabelSource::Declared)],
    };

    let out = build(&[d1, d2], &intents);

    let intent: Vec<_> = out
        .groups
        .iter()
        .filter(|g| g.kind == GroupKind::Intent)
        .collect();
    assert_eq!(intent.len(), 1);
    assert_eq!(intent[0].label, "turn B's own reason");
    // Bound to its OWN file only — the orphan's a.rs is not swept in.
    let files: Vec<&str> = intent[0].files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, vec!["b.rs"]);
    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.unmatched, 0);
}

/// An inferred label is a guess mined from prose; it must never cross a turn,
/// however cleanly its (mined) scope covers the file.
#[test]
fn an_inferred_path_scoped_label_never_crosses_a_turn() {
    let diff = simple(
        "f.rs",
        &["+    let a_distinctive_inferred_line = go_now();"],
    );
    let intents = Intents {
        records: vec![record(
            "turnA",
            "f.rs",
            &["    let a_distinctive_inferred_line = go_now();"],
            1,
        )],
        labels: vec![plabel(
            "turnB",
            "a mined sentence",
            &["f.rs"],
            LabelSource::Inferred,
        )],
    };

    let out = build(&[diff], &intents);

    assert!(!out.groups.iter().any(|g| g.kind == GroupKind::Intent));
    assert_eq!(out.scorecard.claims, 0);
}

/// A path-scoped declared label whose file is in the diff but whose recorded
/// work is nowhere in it stays an unmatched claim — the same honest report the
/// same-turn path applies today, now reachable across turns.
#[test]
fn a_path_scoped_claim_whose_content_is_absent_is_unmatched() {
    let diff = simple("f.rs", &["+    let something_the_user_typed_by_hand = 1;"]);
    let intents = Intents {
        records: Vec::new(),
        labels: vec![plabel(
            "turnB",
            "the scoped reason",
            &["f.rs"],
            LabelSource::Declared,
        )],
    };

    let out = build(&[diff], &intents);

    assert_eq!(out.scorecard.claims, 1);
    assert_eq!(out.scorecard.evidenced, 0);
    assert_eq!(out.scorecard.unmatched, 1);
    assert_eq!(out.unfulfilled.len(), 1);
    assert_eq!(out.unfulfilled[0].label, "the scoped reason");
    assert_eq!(out.unfulfilled[0].turn_id, "turnB");
    assert_eq!(out.unfulfilled[0].paths, vec!["f.rs"]);
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
