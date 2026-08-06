//! Tests for attributing diff lines to recorded edits.
//! Included by `attribution.rs` under `#[cfg(test)]`.

use super::*;
use crate::git::patch::{DiffLine, Hunk};
use crate::intents::{IntentEdit, IntentLabel, IntentRecord, ProviderId};

// -- builders ---------------------------------------------------------------

/// Build one hunk from a compact description, assigning diff indices in the
/// order libgit2 would. `+` adds, `-` removes, anything else is context.
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

fn diff(path: &str, hunks: Vec<Hunk>) -> FileDiff {
    FileDiff {
        path: path.to_string(),
        old_path: None,
        hunks,
        is_binary: false,
    }
}

fn one_hunk_diff(path: &str, lines: &[&str]) -> FileDiff {
    let mut index = 0;
    diff(path, vec![hunk(lines, &mut index)])
}

fn record(seq: u64, path: &str, old: &[&str], new: &[&str]) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: format!("turn-{seq}"),
        tool_use_id: format!("tool-{seq}"),
        seq,
        path: path.to_string(),
        edit: IntentEdit {
            old_lines: old.iter().map(|s| s.to_string()).collect(),
            new_lines: new.iter().map(|s| s.to_string()).collect(),
            whole_file: false,
        },
        branch: None,
    }
}

fn labelled(record: IntentRecord, text: &str) -> Intents {
    let label = IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: record.turn_id.clone(),
        label: text.to_string(),
        paths: Vec::new(),
        anchor: None,
    };
    Intents {
        records: vec![record],
        labels: vec![label],
    }
}

fn intents(records: Vec<IntentRecord>) -> Intents {
    Intents {
        records,
        labels: Vec::new(),
    }
}

fn run(diff: &FileDiff, intents: &Intents) -> FileAttribution {
    attribute_file(diff, intents, Options::default())
}

/// Diff indices claimed by a hunk, flattened across every span.
fn claimed(attribution: &FileAttribution, hunk: usize) -> Vec<u32> {
    let mut all: Vec<u32> = attribution.hunks[hunk]
        .spans
        .iter()
        .flat_map(|s| s.line_indices.clone())
        .collect();
    all.sort();
    all
}

// -- basic attribution ------------------------------------------------------

#[test]
fn a_single_record_matching_one_hunk_labels_its_changed_lines() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            " fn thing() {",
            "-    let old = compute_original_value();",
            "+    let updated = compute_replacement_value();",
            " }",
        ],
    );
    let i = labelled(
        record(
            1,
            "a.rs",
            &["    let old = compute_original_value();"],
            &["    let updated = compute_replacement_value();"],
        ),
        "swap the computation",
    );

    let attribution = run(&d, &i);

    assert_eq!(claimed(&attribution, 0), vec![1, 2]);
    assert_eq!(attribution.hunks[0].unattributed_lines, 0);
    assert_eq!(
        attribution.hunks[0].spans[0].label.as_deref(),
        Some("swap the computation")
    );
}

#[test]
fn context_lines_are_never_attributed() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            " let untouched = surrounding_context_line();",
            "+let added = a_distinctive_new_statement();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let untouched = surrounding_context_line();",
            "let added = a_distinctive_new_statement();",
        ],
    )]);

    let attribution = run(&d, &i);

    // Index 0 is the context line and must never be claimed.
    assert!(!claimed(&attribution, 0).contains(&0));
}

#[test]
fn a_deletion_matches_the_removed_text_not_the_added_text() {
    let d = one_hunk_diff("a.rs", &["-let removed_original_line = 1;"]);
    // The record's *new* side contains the same text; the old side does not.
    let i = intents(vec![record(
        1,
        "a.rs",
        &["something_entirely_different();"],
        &["let removed_original_line = 1;"],
    )]);

    let attribution = run(&d, &i);

    assert!(claimed(&attribution, 0).is_empty());
}

#[test]
fn records_for_other_files_are_ignored() {
    let d = one_hunk_diff("a.rs", &["+let distinctive_added_line = 1;"]);
    let i = intents(vec![record(1, "b.rs", &[], &["let distinctive_added_line = 1;"])]);

    assert!(run(&d, &i).is_empty());
}

#[test]
fn a_diff_with_no_records_leaves_every_line_unattributed() {
    let d = one_hunk_diff("a.rs", &["+one", "+two"]);

    let attribution = run(&d, &intents(Vec::new()));

    assert_eq!(attribution.hunks[0].unattributed_lines, 2);
    assert!(attribution.hunks[0].spans.is_empty());
}

#[test]
fn a_binary_file_produces_no_attributions() {
    let mut d = one_hunk_diff("a.png", &["+binary content that would otherwise match"]);
    d.is_binary = true;
    let i = intents(vec![record(
        1,
        "a.png",
        &[],
        &["binary content that would otherwise match"],
    )]);

    assert!(run(&d, &i).is_empty());
}

// -- position independence --------------------------------------------------

/// The whole premise: a record's recorded position is irrelevant, because the
/// file has moved underneath it.
#[test]
fn a_record_is_matched_after_the_file_shifted_by_hundreds_of_lines() {
    let mut index = 0;
    let mut lines: Vec<String> = (0..300).map(|n| format!(" filler line number {n}")).collect();
    lines.push("+let relocated = a_very_distinctive_call();".to_string());
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();

    let d = diff("a.rs", vec![hunk(&refs, &mut index)]);
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &["let relocated = a_very_distinctive_call();"],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![300]);
}

// -- normalisation ----------------------------------------------------------

#[test]
fn a_record_whose_lines_were_reformatted_still_matches() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+        let reformatted = compute( a , b );",
            "+        let second = another_distinct_call();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let reformatted = compute(a, b);",
            "let second = another_distinct_call();",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1]);
}

#[test]
fn a_record_reindented_from_tabs_to_spaces_still_matches() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+    let indented = distinctive_function_call();",
            "+    let other = second_distinctive_call();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "\t\tlet indented = distinctive_function_call();",
            "\t\tlet other = second_distinctive_call();",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1]);
}

/// Without stripping the carriage return, a CRLF repository attributes nothing
/// at all — the single most consequential normalisation here.
#[test]
fn crlf_line_endings_in_the_diff_match_lf_lines_in_the_record() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let windows_line = distinctive_call_here();\r",
            "+let second_line = another_distinct_call();\r",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let windows_line = distinctive_call_here();",
            "let second_line = another_distinct_call();",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1]);
}

#[test]
fn a_formatter_added_trailing_comma_still_matches() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+    distinctive_argument_name: some_value,",
            "+    another_distinctive_name: other_value,",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "    distinctive_argument_name: some_value",
            "    another_distinctive_name: other_value",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1]);
}

/// Skeleton normalisation removes whitespace, so it must not be allowed to
/// treat a changed string literal as mere reformatting.
#[test]
fn changing_a_string_literal_is_not_treated_as_a_formatting_change() {
    let d = one_hunk_diff("a.rs", &["+let message = \"goodbye world friend\";"]);
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &["let message = \"hello world friend\";"],
    )]);

    assert!(claimed(&run(&d, &i), 0).is_empty());
}

// -- frame stripping --------------------------------------------------------

/// An edit's before and after text share the context that made it unique.
/// Those lines are not what the record changed.
#[test]
fn unchanged_lines_shared_by_both_sides_are_not_used_as_anchors() {
    let d = one_hunk_diff("a.rs", &["+fn surrounding_frame_function() {"]);
    // Both sides contain the frame line; only the body differs.
    let i = intents(vec![record(
        1,
        "a.rs",
        &["fn surrounding_frame_function() {", "    old_body_call();"],
        &["fn surrounding_frame_function() {", "    new_body_call();"],
    )]);

    assert!(
        claimed(&run(&d, &i), 0).is_empty(),
        "the shared frame line should not anchor"
    );
}

// -- several records, one hunk ----------------------------------------------

#[test]
fn two_records_in_one_hunk_each_label_their_own_lines() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let first_change = distinctive_alpha_call();",
            "+let first_more = distinctive_alpha_second();",
            " context between them",
            "+let second_change = distinctive_beta_call();",
            "+let second_more = distinctive_beta_second();",
        ],
    );
    let i = intents(vec![
        record(
            1,
            "a.rs",
            &[],
            &[
                "let first_change = distinctive_alpha_call();",
                "let first_more = distinctive_alpha_second();",
            ],
        ),
        record(
            2,
            "a.rs",
            &[],
            &[
                "let second_change = distinctive_beta_call();",
                "let second_more = distinctive_beta_second();",
            ],
        ),
    ]);

    let attribution = run(&d, &i);

    assert_eq!(attribution.hunks[0].spans.len(), 2);
    let by_turn: std::collections::HashMap<_, _> = attribution.hunks[0]
        .spans
        .iter()
        .map(|s| (s.turn_id.as_str(), s.line_indices.clone()))
        .collect();
    assert_eq!(by_turn["turn-1"], vec![0, 1]);
    assert_eq!(by_turn["turn-2"], vec![3, 4]);
}

/// With three lines of context, unrelated edits merge into one hunk. An even
/// split has no majority, and the UI must say "mixed" rather than pick one.
#[test]
fn a_hunk_split_evenly_between_records_reports_no_dominant() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let first_change = distinctive_alpha_call();",
            "+let first_more = distinctive_alpha_second();",
            "+let second_change = distinctive_beta_call();",
            "+let second_more = distinctive_beta_second();",
        ],
    );
    let i = intents(vec![
        record(
            1,
            "a.rs",
            &[],
            &[
                "let first_change = distinctive_alpha_call();",
                "let first_more = distinctive_alpha_second();",
            ],
        ),
        record(
            2,
            "a.rs",
            &[],
            &[
                "let second_change = distinctive_beta_call();",
                "let second_more = distinctive_beta_second();",
            ],
        ),
    ]);

    let attribution = run(&d, &i);

    assert_eq!(attribution.hunks[0].spans.len(), 2);
    assert!(attribution.hunks[0].dominant.is_none(), "should be mixed");
}

#[test]
fn a_record_owning_most_of_a_hunk_is_reported_as_dominant() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let owned_one = distinctive_alpha_call();",
            "+let owned_two = distinctive_alpha_second();",
            "+let owned_three = distinctive_alpha_third();",
            "+let unrelated = something_nobody_recorded();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let owned_one = distinctive_alpha_call();",
            "let owned_two = distinctive_alpha_second();",
            "let owned_three = distinctive_alpha_third();",
        ],
    )]);

    let attribution = run(&d, &i);

    assert_eq!(attribution.hunks[0].dominant.as_deref(), Some("turn-1"));
    assert_eq!(attribution.hunks[0].unattributed_lines, 1);
}

#[test]
fn one_record_spanning_two_hunks_is_reported_in_each() {
    let mut index = 0;
    let first = hunk(&["+let alpha = distinctive_first_call();"], &mut index);
    let second = hunk(&["+let beta = distinctive_second_call();"], &mut index);
    let d = diff("a.rs", vec![first, second]);

    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let alpha = distinctive_first_call();",
            "let beta = distinctive_second_call();",
        ],
    )]);

    let attribution = run(&d, &i);

    assert_eq!(attribution.hunks[0].spans.len(), 1);
    assert_eq!(attribution.hunks[1].spans.len(), 1);
}

#[test]
fn hunk_indices_are_never_renumbered() {
    let mut index = 0;
    let first = hunk(&["+let alpha = distinctive_first_call();"], &mut index);
    let second = hunk(&["+let beta = distinctive_second_call();"], &mut index);
    let d = diff("a.rs", vec![first, second]);
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let alpha = distinctive_first_call();",
            "let beta = distinctive_second_call();",
        ],
    )]);

    let attribution = run(&d, &i);

    assert_eq!(attribution.hunks.len(), 2);
    assert_eq!(attribution.hunks[0].hunk, 0);
    assert_eq!(attribution.hunks[1].hunk, 1);
    assert_eq!(claimed(&attribution, 1), vec![1]);
}

// -- overwriting and reverting ----------------------------------------------

#[test]
fn a_later_record_overwriting_an_earlier_one_wins_the_shared_lines() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let shared = the_same_distinctive_line();",
            "+let also = a_second_shared_distinctive();",
        ],
    );
    let i = intents(vec![
        record(
            1,
            "a.rs",
            &[],
            &[
                "let shared = the_same_distinctive_line();",
                "let also = a_second_shared_distinctive();",
            ],
        ),
        record(
            5,
            "a.rs",
            &[],
            &[
                "let shared = the_same_distinctive_line();",
                "let also = a_second_shared_distinctive();",
            ],
        ),
    ]);

    let attribution = run(&d, &i);

    assert_eq!(attribution.hunks[0].spans.len(), 1);
    assert_eq!(attribution.hunks[0].spans[0].turn_id, "turn-5");
}

/// A record the user partly undid still owns whatever survived.
#[test]
fn a_partially_reverted_record_still_labels_its_surviving_lines() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let kept_one = distinctive_surviving_call();",
            "+let kept_two = distinctive_surviving_second();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let kept_one = distinctive_surviving_call();",
            "let kept_two = distinctive_surviving_second();",
            "let removed = the_user_deleted_this_line();",
            "let also_removed = and_this_one_too_here();",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1]);
}

#[test]
fn a_fully_reverted_record_labels_nothing() {
    let d = one_hunk_diff("a.rs", &["+something_the_user_wrote_themselves();"]);
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &["a_line_that_no_longer_exists_anywhere();"],
    )]);

    assert!(claimed(&run(&d, &i), 0).is_empty());
}

// -- weak lines -------------------------------------------------------------

/// `}` is the commonest line in any C-family file. If it could anchor, every
/// record would match every hunk.
#[test]
fn a_closing_brace_is_never_matched_on_its_own_content() {
    let d = one_hunk_diff("a.rs", &["+}"]);
    let i = intents(vec![record(1, "a.rs", &[], &["}"])]);

    assert!(claimed(&run(&d, &i), 0).is_empty());
}

#[test]
fn a_closing_brace_inside_one_records_block_inherits_that_record() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+fn distinctive_new_function_name() {",
            "+}",
            "+let after = another_distinctive_call();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "fn distinctive_new_function_name() {",
            "}",
            "let after = another_distinctive_call();",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1, 2]);
}

#[test]
fn a_brace_between_two_different_records_stays_unattributed() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let alpha_one = distinctive_alpha_call();",
            "+let alpha_two = distinctive_alpha_second();",
            "+}",
            "+let beta_one = distinctive_beta_call();",
            "+let beta_two = distinctive_beta_second();",
        ],
    );
    let i = intents(vec![
        record(
            1,
            "a.rs",
            &[],
            &[
                "let alpha_one = distinctive_alpha_call();",
                "let alpha_two = distinctive_alpha_second();",
                "}",
            ],
        ),
        record(
            2,
            "a.rs",
            &[],
            &[
                "}",
                "let beta_one = distinctive_beta_call();",
                "let beta_two = distinctive_beta_second();",
            ],
        ),
    ]);

    let attribution = run(&d, &i);

    assert!(
        !claimed(&attribution, 0).contains(&2),
        "the brace has two disagreeing neighbours and must stay unlabelled"
    );
}

// -- thresholds -------------------------------------------------------------

#[test]
fn a_one_line_edit_with_long_unique_text_is_labelled() {
    let d = one_hunk_diff(
        "a.rs",
        &["+let configured_retry_limit = read_from_environment();"],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &["let configured_retry_limit = read_from_environment();"],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0]);
}

#[test]
fn a_one_line_edit_of_short_generic_text_is_not_labelled() {
    let d = one_hunk_diff("a.rs", &["+x = 1;"]);
    let i = intents(vec![record(1, "a.rs", &[], &["x = 1;"])]);

    assert!(claimed(&run(&d, &i), 0).is_empty());
}

/// Repeated boilerplate destroys distinctness, and distinctness is what keeps
/// the matcher honest.
#[test]
fn a_line_repeated_throughout_the_file_does_not_label() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+    assert_eq!(result, expected);",
            "+    assert_eq!(result, expected);",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "    assert_eq!(result, expected);",
            "    assert_eq!(result, expected);",
        ],
    )]);

    assert!(claimed(&run(&d, &i), 0).is_empty());
}

#[test]
fn a_run_broken_by_one_hand_edited_line_stays_a_single_run() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let alpha = distinctive_first_call();",
            "+let interloper = the_user_typed_this_here();",
            "+let beta = distinctive_second_call();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let alpha = distinctive_first_call();",
            "let beta = distinctive_second_call();",
        ],
    )]);

    let attribution = run(&d, &i);

    assert_eq!(claimed(&attribution, 0), vec![0, 2]);
    assert_eq!(attribution.hunks[0].unattributed_lines, 1);
}

// -- whole-file writes ------------------------------------------------------

fn write_record(seq: u64, path: &str, lines: &[&str]) -> IntentRecord {
    let mut r = record(seq, path, &[], lines);
    r.edit.whole_file = true;
    r
}

#[test]
fn a_whole_file_write_labels_a_newly_created_file() {
    let d = one_hunk_diff(
        "new.rs",
        &[
            "+fn distinctive_alpha_function() {}",
            "+fn distinctive_beta_function() {}",
            "+fn distinctive_gamma_function() {}",
        ],
    );
    let i = intents(vec![write_record(
        1,
        "new.rs",
        &[
            "fn distinctive_alpha_function() {}",
            "fn distinctive_beta_function() {}",
            "fn distinctive_gamma_function() {}",
        ],
    )]);

    assert_eq!(claimed(&run(&d, &i), 0), vec![0, 1, 2]);
}

/// A write's text is the whole file, so unrestricted it would swallow lines a
/// targeted edit legitimately owns.
#[test]
fn a_whole_file_write_does_not_take_lines_a_targeted_edit_owns() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+fn distinctive_alpha_function() {}",
            "+fn distinctive_beta_function() {}",
            "+fn distinctive_gamma_function() {}",
        ],
    );
    let i = intents(vec![
        record(
            1,
            "a.rs",
            &[],
            &[
                "fn distinctive_alpha_function() {}",
                "fn distinctive_beta_function() {}",
            ],
        ),
        write_record(
            2,
            "a.rs",
            &[
                "fn distinctive_alpha_function() {}",
                "fn distinctive_beta_function() {}",
                "fn distinctive_gamma_function() {}",
            ],
        ),
    ]);

    let attribution = run(&d, &i);
    let owner: std::collections::HashMap<u32, &str> = attribution.hunks[0]
        .spans
        .iter()
        .flat_map(|s| s.line_indices.iter().map(|i| (*i, s.turn_id.as_str())))
        .collect();

    assert_eq!(owner[&0], "turn-1", "the targeted edit must keep its lines");
    assert_eq!(owner[&1], "turn-1");
}

#[test]
fn a_whole_file_write_needs_several_matching_lines_before_it_labels() {
    let d = one_hunk_diff("a.rs", &["+let only_one = distinctive_single_line();"]);
    let i = intents(vec![write_record(
        1,
        "a.rs",
        &["let only_one = distinctive_single_line();"],
    )]);

    assert!(claimed(&run(&d, &i), 0).is_empty());
}

// -- determinism ------------------------------------------------------------

#[test]
fn shuffling_the_record_order_produces_an_identical_result() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let alpha_one = distinctive_alpha_call();",
            "+let alpha_two = distinctive_alpha_second();",
            "+let beta_one = distinctive_beta_call();",
            "+let beta_two = distinctive_beta_second();",
        ],
    );
    let first = record(
        1,
        "a.rs",
        &[],
        &[
            "let alpha_one = distinctive_alpha_call();",
            "let alpha_two = distinctive_alpha_second();",
        ],
    );
    let second = record(
        2,
        "a.rs",
        &[],
        &[
            "let beta_one = distinctive_beta_call();",
            "let beta_two = distinctive_beta_second();",
        ],
    );

    let forwards = run(&d, &intents(vec![first.clone(), second.clone()]));
    let backwards = run(&d, &intents(vec![second, first]));

    assert_eq!(forwards, backwards);
}

#[test]
fn running_attribution_twice_produces_an_identical_result() {
    let d = one_hunk_diff(
        "a.rs",
        &["+let stable = a_distinctive_repeatable_call();"],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &["let stable = a_distinctive_repeatable_call();"],
    )]);

    assert_eq!(run(&d, &i), run(&d, &i));
}

// -- confidence -------------------------------------------------------------

#[test]
fn an_exact_multi_line_match_is_reported_with_high_confidence() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+let alpha = distinctive_first_call();",
            "+let beta = distinctive_second_call();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let alpha = distinctive_first_call();",
            "let beta = distinctive_second_call();",
        ],
    )]);

    assert_eq!(run(&d, &i).hunks[0].spans[0].confidence, Confidence::High);
}

#[test]
fn a_reformatted_match_is_reported_below_high_confidence() {
    let d = one_hunk_diff(
        "a.rs",
        &[
            "+        let alpha   =  distinctive_first_call();",
            "+        let beta    =  distinctive_second_call();",
        ],
    );
    let i = intents(vec![record(
        1,
        "a.rs",
        &[],
        &[
            "let alpha = distinctive_first_call();",
            "let beta = distinctive_second_call();",
        ],
    )]);

    assert!(run(&d, &i).hunks[0].spans[0].confidence < Confidence::High);
}
