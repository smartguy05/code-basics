//! Tests for collapsing hunks into cards.
//! Included by `grouping.rs` under `#[cfg(test)]`.

use super::*;
use crate::git::attribution::{self, Options};
use crate::git::patch::{DiffLine, FileDiff};
use crate::intents::{IntentEdit, IntentLabel, IntentRecord, Intents, LabelSource, ProviderId};

fn hunk(lines: &[&str], start: &mut u32, header: &str) -> Hunk {
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
        header: header.to_string(),
        lines: built,
    }
}

fn file(path: &str, hunks: Vec<Hunk>) -> FileDiff {
    FileDiff {
        path: path.to_string(),
        old_path: None,
        hunks,
        is_binary: false,
    }
}

fn simple(path: &str, lines: &[&str], header: &str) -> FileDiff {
    let mut index = 0;
    file(path, vec![hunk(lines, &mut index, header)])
}

/// Group with no recorded intent at all, exercising passes 2 and 3.
fn group_without_intent(diffs: &[FileDiff]) -> Vec<IntentGroup> {
    let empty = Intents::default();
    let attributions = attribution::attribute(diffs, &empty, Options::default());
    group(diffs, &attributions, &empty)
}

// -- formatting detection ---------------------------------------------------

#[test]
fn a_reindented_line_is_recognised_as_formatting_only() {
    let d = simple(
        "a.rs",
        &[
            "-    let value = compute(a, b);",
            "+        let value = compute(a, b);",
        ],
        "",
    );

    assert!(is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn collapsing_internal_spacing_is_recognised_as_formatting_only() {
    let d = simple(
        "a.rs",
        &[
            "-let value = compute( a , b );",
            "+let value = compute(a, b);",
        ],
        "",
    );

    assert!(is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn a_line_ending_change_is_recognised_as_formatting_only() {
    let d = simple("a.rs", &["-let value = 1;\r", "+let value = 1;"], "");

    assert!(is_formatting_only(&d.hunks[0], &d.path));
}

/// Reordering identical lines is NOT formatting: the detector cannot tell a
/// cosmetic import shuffle from a statement reorder that changes behaviour, and
/// the abstain rule ("a wrong label is worse than none") means a real change
/// hidden as "whitespace only" is the costlier mistake. So a pure relocation —
/// same characters, different order — is refused the formatting label.
#[test]
fn reordering_lines_without_changing_them_is_not_formatting() {
    let d = simple(
        "a.rs",
        &["-use beta;", "-use alpha;", "+use alpha;", "+use beta;"],
        "",
    );

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

/// Regression: swapping two adjacent statements reorders execution — the
/// assignment now runs before the await, not after — yet both lines are
/// byte-identical, so a multiset compare that ignores order used to call it
/// "whitespace only". It is a logic change.
#[test]
fn swapping_two_statements_is_not_formatting() {
    let d = simple(
        "a.rs",
        &[
            "-        await GetFlightPlanItineraryFullAsync();",
            "-        ShouldReloadFlight = false;",
            "+        ShouldReloadFlight = false;",
            "+        await GetFlightPlanItineraryFullAsync();",
        ],
        "",
    );

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn a_real_change_is_not_formatting_even_when_spacing_also_changed() {
    let d = simple(
        "a.rs",
        &[
            "-let value = compute(a, b);",
            "+let value  =  compute(a, c);",
        ],
        "",
    );

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn a_pure_addition_is_never_formatting() {
    let d = simple("a.rs", &["+let added = 1;"], "");

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn a_pure_deletion_is_never_formatting() {
    let d = simple("a.rs", &["-let removed = 1;"], "");

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

/// The reason comments are not stripped: `//` inside a string is not a
/// comment, and treating it as one would hide a real change.
#[test]
fn a_changed_string_containing_a_comment_marker_is_not_formatting() {
    let d = simple(
        "a.rs",
        &[
            "-let url = \"https://old.example.com\";",
            "+let url = \"https://new.example.com\";",
        ],
        "",
    );

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

/// In Python the indent *is* the syntax: dedenting a line moves it out of the
/// block it was in, which changes what the program does while leaving every
/// token identical. Calling that "formatting" would hide a real change behind
/// a card reviewers are meant to skim.
#[test]
fn changing_indentation_in_python_is_not_treated_as_formatting() {
    let d = simple(
        "script.py",
        &["-        return total", "+    return total"],
        "",
    );

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn changing_indentation_in_yaml_is_not_treated_as_formatting() {
    let d = simple("config.yml", &["-    key: value", "+  key: value"], "");

    assert!(!is_formatting_only(&d.hunks[0], &d.path));
}

/// Whitespace that is not the indent is still just spacing, even in Python.
#[test]
fn respacing_within_a_python_line_is_still_formatting() {
    let d = simple(
        "script.py",
        &[
            "-    total = compute( a , b )",
            "+    total = compute(a, b)",
        ],
        "",
    );

    assert!(is_formatting_only(&d.hunks[0], &d.path));
}

#[test]
fn a_formatting_hunk_is_grouped_and_sorted_last() {
    let diffs = vec![
        simple(
            "a.rs",
            &[
                "-    let value = compute(a, b);",
                "+let value = compute(a, b);",
            ],
            "",
        ),
        simple("b.rs", &["+fn brand_new_function() {}"], ""),
    ];

    let groups = group_without_intent(&diffs);

    assert_eq!(groups.last().unwrap().kind, GroupKind::Formatting);
}

#[test]
fn formatting_across_several_files_collapses_into_one_card() {
    let diffs = vec![
        simple("a.rs", &["-  let x = 1;", "+let x = 1;"], ""),
        simple("b.rs", &["-  let y = 2;", "+let y = 2;"], ""),
        simple("c.rs", &["-  let z = 3;", "+let z = 3;"], ""),
    ];

    let groups = group_without_intent(&diffs);
    let formatting: Vec<_> = groups
        .iter()
        .filter(|g| g.kind == GroupKind::Formatting)
        .collect();

    assert_eq!(formatting.len(), 1);
    assert_eq!(formatting[0].files.len(), 3);
}

// -- enclosing symbol -------------------------------------------------------

#[test]
fn the_symbol_comes_from_the_git_hunk_header_when_there_is_one() {
    let d = simple(
        "a.rs",
        &["+    changed_body();"],
        "fn existing_function(a: u32) -> bool {",
    );

    assert_eq!(
        enclosing_symbol(&d.hunks[0]).as_deref(),
        Some("existing_function")
    );
}

#[test]
fn a_declaration_in_the_hunk_is_used_when_the_header_is_empty() {
    let d = simple("a.rs", &["+fn newly_added_function() {", "+}"], "");

    assert_eq!(
        enclosing_symbol(&d.hunks[0]).as_deref(),
        Some("newly_added_function")
    );
}

#[test]
fn a_csharp_method_declaration_is_recognised() {
    let d = simple(
        "Thing.cs",
        &[
            "+    public decimal EstimateCost(int quantity)",
            "+    {",
            "+    }",
        ],
        "",
    );

    assert_eq!(
        enclosing_symbol(&d.hunks[0]).as_deref(),
        Some("EstimateCost")
    );
}

#[test]
fn a_typescript_export_is_recognised() {
    let d = simple("a.ts", &["+export function buildQuote(input: Input) {"], "");

    assert_eq!(enclosing_symbol(&d.hunks[0]).as_deref(), Some("buildQuote"));
}

#[test]
fn a_class_declaration_is_recognised() {
    let d = simple("a.cs", &["+public sealed class QuoteCalculator", "+{"], "");

    assert_eq!(
        enclosing_symbol(&d.hunks[0]).as_deref(),
        Some("QuoteCalculator")
    );
}

/// The name of a binding is the binding, not the type it was annotated with.
#[test]
fn a_let_binding_names_the_variable_not_its_type() {
    assert_eq!(
        declaration_name("let total: usize = 0;").as_deref(),
        Some("total")
    );
}

#[test]
fn a_static_names_the_item_not_its_type() {
    assert_eq!(
        declaration_name("static COUNTER: AtomicU64 = AtomicU64::new(0);").as_deref(),
        Some("COUNTER")
    );
}

#[test]
fn a_typescript_annotated_const_names_the_binding() {
    assert_eq!(
        declaration_name("const cache: Map<string, number> = new Map();").as_deref(),
        Some("cache")
    );
}

/// A C# property has no colon, so the last-identifier rule still applies.
#[test]
fn a_csharp_property_names_the_property() {
    assert_eq!(
        declaration_name("public Bitness Bitness { get; }").as_deref(),
        Some("Bitness")
    );
}

/// `import type { … }` carries the declaring keyword `type`, but it declares
/// nothing — without this rule every type-only import titles a card "import".
#[test]
fn a_type_only_import_is_not_a_declaration() {
    assert_eq!(
        declaration_name("import type { IntentGroup } from \"./types\";"),
        None
    );
}

/// `pub use …` carries `pub`, but re-exporting is not declaring: the last
/// identifier rule would title the card "use".
#[test]
fn a_re_export_is_not_a_declaration() {
    assert_eq!(
        declaration_name("pub use crate::intents::ProviderId;"),
        None
    );
}

/// An import line is a location nobody wants on a card: "New import" says
/// nothing. Rejecting it lets the hunk fall through to its file.
#[test]
fn an_import_line_in_the_header_is_not_a_symbol() {
    let d = simple(
        "a.ts",
        &["+    doThing();"],
        "import { useState } from \"react\";",
    );

    assert_eq!(enclosing_symbol(&d.hunks[0]), None);
}

#[test]
fn a_use_statement_header_is_not_a_symbol() {
    let d = simple(
        "a.rs",
        &["+    do_thing();"],
        "use std::sync::atomic::AtomicU64;",
    );

    assert_eq!(enclosing_symbol(&d.hunks[0]), None);
}

/// Without requiring a declaring keyword, every assignment would look like a
/// symbol and the grouping would be meaningless.
#[test]
fn an_ordinary_statement_is_not_mistaken_for_a_declaration() {
    let d = simple("a.rs", &["+    result = compute(a, b);"], "");

    assert!(enclosing_symbol(&d.hunks[0]).is_none());
}

#[test]
fn a_comment_line_is_not_mistaken_for_a_declaration() {
    let d = simple("a.rs", &["+// fn not_really_a_function()"], "");

    assert!(enclosing_symbol(&d.hunks[0]).is_none());
}

#[test]
fn a_new_symbol_and_a_changed_one_are_grouped_differently() {
    let new = simple("a.rs", &["+fn brand_new_thing() {", "+}"], "");
    let changed = simple("b.rs", &["+    body_changed();"], "fn existing_thing() {");

    let groups = group_without_intent(&[new, changed]);
    let kinds: Vec<GroupKind> = groups.iter().map(|g| g.kind).collect();

    assert!(kinds.contains(&GroupKind::NewSymbol), "got {kinds:?}");
    assert!(kinds.contains(&GroupKind::ModifiedSymbol), "got {kinds:?}");
}

#[test]
fn hunks_in_the_same_symbol_across_files_collapse_into_one_card() {
    let first = simple("a.rs", &["+    one();"], "fn shared_name() {");
    let second = simple("b.rs", &["+    two();"], "fn shared_name() {");

    let groups = group_without_intent(&[first, second]);
    let matching: Vec<_> = groups
        .iter()
        .filter(|g| g.symbol.as_deref() == Some("shared_name"))
        .collect();

    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].files.len(), 2);
    assert_eq!(matching[0].hunk_count(), 2);
}

#[test]
fn a_hunk_with_no_symbol_falls_back_to_its_file() {
    let d = simple("src/deep/config.json", &["+  \"key\": \"value\","], "");

    let groups = group_without_intent(&[d]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::Other);
    assert!(groups[0].label.contains("config.json"));
}

/// The badge on the card already says New/Changed; repeating the verb in the
/// label just makes it longer.
#[test]
fn a_symbol_card_label_is_the_bare_symbol() {
    let d = simple("a.rs", &["+fn brand_new_thing() {", "+}"], "");

    let groups = group_without_intent(&[d]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::NewSymbol);
    assert_eq!(groups[0].label, "brand_new_thing");
}

// -- collapsing singleton symbol cards --------------------------------------

/// One card per hunk is the failure mode this exists to prevent: a file with a
/// scattering of one-hunk symbols is one decision, not five.
#[test]
fn many_singleton_symbols_in_one_file_collapse_into_one_card_per_file() {
    let mut index = 0;
    let d = file(
        "src/thing.rs",
        vec![
            hunk(&["+fn alpha_one() {}"], &mut index, ""),
            hunk(&["+fn beta_two() {}"], &mut index, ""),
            hunk(&["+fn gamma_three() {}"], &mut index, ""),
        ],
    );

    let groups = group_without_intent(&[d]);

    assert_eq!(
        groups.len(),
        1,
        "got {:?}",
        groups.iter().map(|g| &g.label).collect::<Vec<_>>()
    );
    assert_eq!(groups[0].kind, GroupKind::Other);
    assert!(groups[0].label.contains("thing.rs"), "{}", groups[0].label);
    assert_eq!(groups[0].hunk_count(), 3);
}

/// A symbol touched in two files is a real grouping and must not be dissolved
/// back into per-file buckets.
#[test]
fn a_symbol_spanning_files_survives_the_singleton_collapse() {
    let first = simple("a.rs", &["+    one();"], "fn shared_name() {");
    let second = simple("b.rs", &["+    two();"], "fn shared_name() {");

    let groups = group_without_intent(&[first, second]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].symbol.as_deref(), Some("shared_name"));
    assert_eq!(groups[0].files.len(), 2);
}

/// With only one of them, the symbol name is a better label than the file.
#[test]
fn a_files_single_symbol_card_keeps_its_symbol_label() {
    let d = simple("a.rs", &["+fn only_one_thing() {}"], "");

    let groups = group_without_intent(&[d]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::NewSymbol);
    assert_eq!(groups[0].label, "only_one_thing");
}

/// Two groups may never share an id — stage and revert look a group up by it.
#[test]
fn singleton_collapse_merges_into_an_existing_other_bucket_for_the_file() {
    let mut index = 0;
    let d = file(
        "a.rs",
        vec![
            hunk(&["+fn alpha_one() {}"], &mut index, ""),
            hunk(&["+fn beta_two() {}"], &mut index, ""),
            hunk(&["+    plain_call();"], &mut index, ""),
        ],
    );

    let groups = group_without_intent(&[d]);

    assert_eq!(
        groups.len(),
        1,
        "got {:?}",
        groups.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    assert_eq!(groups[0].id, "other:a.rs");
    assert_eq!(groups[0].kind, GroupKind::Other);
    assert_eq!(groups[0].hunk_count(), 3);
    assert_eq!(groups[0].line_count, 3);
}

// -- recorded intent wins ---------------------------------------------------

fn with_intent(diffs: &[FileDiff], intents: &Intents) -> Vec<IntentGroup> {
    let attributions = attribution::attribute(diffs, intents, Options::default());
    group(diffs, &attributions, intents)
}

fn record_with_label(path: &str, lines: &[&str], label: &str) -> Intents {
    labelled_record(path, lines, label, LabelSource::Declared)
}

fn labelled_record(path: &str, lines: &[&str], label: &str, source: LabelSource) -> Intents {
    let record = IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "turn-1".into(),
        tool_use_id: "tool-1".into(),
        seq: 1,
        path: path.to_string(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: lines.iter().map(|s| s.to_string()).collect(),
            whole_file: false,
        },
        branch: None,
    };
    Intents {
        records: vec![record],
        labels: vec![IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "turn-1".into(),
            label: label.to_string(),
            paths: Vec::new(),
            anchor: None,
            source,
            self_confidence: None,
        }],
    }
}

// -- a card only claims a stated intent when there was one -------------------

/// An inferred label is a sentence mined out of chat prose that happened to sit
/// near the edit. It may well be right, but the agent never offered it as a
/// title, so the card must not present it as one.
#[test]
fn an_inferred_label_does_not_produce_an_intent_card() {
    let d = simple(
        "a.rs",
        &["+    let retry_limit = read_configured_retry_limit();"],
        "fn refresh_token() {",
    );
    let intents = labelled_record(
        "a.rs",
        &["    let retry_limit = read_configured_retry_limit();"],
        "the retry limit now comes from configuration",
        LabelSource::Inferred,
    );

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::SameTurn);
    // The sentence is still shown — it is the best description available — but
    // the kind says where it came from.
    assert_eq!(
        groups[0].label,
        "the retry limit now comes from configuration"
    );
}

#[test]
fn a_declared_label_still_produces_an_intent_card() {
    let d = simple(
        "a.rs",
        &["+    let retry_limit = read_configured_retry_limit();"],
        "fn refresh_token() {",
    );
    let intents = labelled_record(
        "a.rs",
        &["    let retry_limit = read_configured_retry_limit();"],
        "add retry to token refresh",
        LabelSource::Declared,
    );

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups[0].kind, GroupKind::Intent);
}

/// A user note (a `usernote:` turn with a declared label and the highest seq)
/// titles the card, marks it `user_authored`, and — because it outranks the
/// agent record on the same line — overrides the agent's stated intent.
#[test]
fn a_user_note_wins_the_line_and_marks_the_card() {
    let d = simple("a.rs", &["+    let x = compute_the_thing();"], "fn go() {");

    let line = "    let x = compute_the_thing();";
    let agent = IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "t1".into(),
        tool_use_id: "agent".into(),
        seq: 1,
        path: "a.rs".into(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: vec![line.into()],
            whole_file: false,
        },
        branch: None,
    };
    // The user note, as `intents::user` would merge it: same line content, a
    // higher seq so it wins the tie, a `usernote:` turn and a declared label.
    let note = IntentRecord {
        provider: ProviderId::User,
        turn_id: "usernote:u0".into(),
        tool_use_id: "usernote:u0:0".into(),
        seq: 100,
        path: "a.rs".into(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: vec![line.into()],
            whole_file: false,
        },
        branch: None,
    };
    let intents = Intents {
        records: vec![agent, note],
        labels: vec![
            IntentLabel {
                provider: ProviderId::ClaudeCode,
                turn_id: "t1".into(),
                label: "what the agent said".into(),
                paths: Vec::new(),
                anchor: None,
                source: LabelSource::Declared,
                self_confidence: None,
            },
            IntentLabel {
                provider: ProviderId::User,
                turn_id: "usernote:u0".into(),
                label: "what I say it is for".into(),
                paths: vec!["a.rs".into()],
                anchor: None,
                source: LabelSource::Declared,
                self_confidence: None,
            },
        ],
    };

    let groups = with_intent(&[d], &intents);

    let card = groups
        .iter()
        .find(|g| g.user_authored)
        .expect("the card is marked user-authored");
    assert_eq!(card.kind, GroupKind::Intent);
    assert_eq!(card.label, "what I say it is for");
    // The agent's reason did not also survive as its own card: the note took
    // the only changed line.
    assert!(
        groups.iter().all(|g| g.label != "what the agent said"),
        "the user note overrode the agent's stated intent"
    );
}

// -- declared reasons surface even without matched geometry ------------------

/// A declared label scoped to files, with no records at all.
fn scoped_labels(pairs: &[(&str, &str, &[&str])]) -> Intents {
    Intents {
        records: Vec::new(),
        labels: pairs
            .iter()
            .map(|(turn, label, paths)| IntentLabel {
                provider: ProviderId::ClaudeCode,
                turn_id: (*turn).into(),
                label: (*label).into(),
                paths: paths.iter().map(|p| p.to_string()).collect(),
                anchor: None,
                source: LabelSource::Declared,
                self_confidence: None,
            })
            .collect(),
    }
}

/// The reported razor bug: a file changed with no captured geometry (edited by
/// Bash or by hand), but a declared reason names it. The card must show the
/// reason and carry the changed lines so it can still be staged.
#[test]
fn a_declared_reason_titles_a_geometry_less_file() {
    let d = simple("f.rs", &["+    let x = 1;"], "fn go() {");
    let intents = scoped_labels(&[("t1", "fix non-resettable cancellation", &["f.rs"])]);

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::Intent);
    assert_eq!(groups[0].label, "fix non-resettable cancellation");
    assert!(groups[0].candidates.is_empty());
    // Stageable: the whole file's changed lines are on the card.
    assert_eq!(groups[0].files.len(), 1);
    assert!(!groups[0].files[0].line_indices.is_empty());
}

// -- the agent's self-reported confidence reaches its card ------------------

/// A declared reason that named a `[confidence: …]` level carries that level
/// onto its intent card, distinct from the matcher's own `confidence`.
#[test]
fn a_declared_labels_self_confidence_reaches_its_group() {
    let d = simple("f.rs", &["+    let x = compute_bound();"], "fn go() {");
    let record = IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "t1".into(),
        tool_use_id: "tool-1".into(),
        seq: 1,
        path: "f.rs".into(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: vec!["    let x = compute_bound();".into()],
            whole_file: false,
        },
        branch: None,
    };
    let intents = Intents {
        records: vec![record],
        labels: vec![IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "t1".into(),
            label: "rewrite the bound".into(),
            paths: Vec::new(),
            anchor: None,
            source: LabelSource::Declared,
            self_confidence: Some(SelfConfidence::Low),
        }],
    };

    let groups = with_intent(&[d], &intents);

    let card = groups
        .iter()
        .find(|g| g.label == "rewrite the bound")
        .expect("the declared intent card");
    assert_eq!(card.kind, GroupKind::Intent);
    assert_eq!(card.self_confidence, Some(SelfConfidence::Low));
}

/// A declared reason that stated nothing leaves the card abstaining: the field
/// is absent, never a defaulted middle value.
#[test]
fn a_declared_label_without_a_stated_confidence_leaves_the_group_none() {
    let d = simple("f.rs", &["+    let x = 1;"], "fn go() {");
    let intents = scoped_labels(&[("t1", "fix cancellation", &["f.rs"])]);

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups[0].kind, GroupKind::Intent);
    assert_eq!(groups[0].self_confidence, None);
}

/// Two declared claims merging into one card keep the most cautious level.
#[test]
fn a_group_gathering_several_confidences_keeps_the_most_cautious() {
    assert_eq!(
        more_cautious(Some(SelfConfidence::High), Some(SelfConfidence::Low)),
        Some(SelfConfidence::Low)
    );
    // A stated level always wins over silence.
    assert_eq!(
        more_cautious(None, Some(SelfConfidence::Medium)),
        Some(SelfConfidence::Medium)
    );
    assert_eq!(
        more_cautious(Some(SelfConfidence::High), None),
        Some(SelfConfidence::High)
    );
}

/// Two records, each declaring its own reason, both matched within one file.
fn two_declared_records(path: &str, a_lines: &[&str], b_lines: &[&str]) -> Intents {
    let record = |turn: &str, seq: u64, lines: &[&str]| IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        tool_use_id: format!("tool-{turn}"),
        seq,
        path: path.to_string(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: lines.iter().map(|s| s.to_string()).collect(),
            whole_file: false,
        },
        branch: None,
    };
    let label = |turn: &str, reason: &str| IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: reason.into(),
        paths: Vec::new(),
        anchor: None,
        source: LabelSource::Declared,
        self_confidence: None,
    };
    Intents {
        records: vec![record("tA", 1, a_lines), record("tB", 2, b_lines)],
        labels: vec![
            label("tA", "make cohort filter sargable"),
            label("tB", "trust DB for gate"),
        ],
    }
}

/// When attribution ties lines in a single hunk to two distinct declared
/// intents and neither holds a majority, each intent becomes its own card
/// carrying only the lines the matcher gave it — not one "ambiguous" card, and
/// not both intents silently dropped to a location card (which is what happened
/// before, leaving them counted as unmatched).
#[test]
fn two_evidenced_intents_in_one_hunk_split_into_separate_cards() {
    let d = simple(
        "svc.rs",
        &[
            "+    let alpha_one = compute_alpha_one();",
            "+    let alpha_two = compute_alpha_two();",
            "+    let beta_one = compute_beta_one();",
            "+    let beta_two = compute_beta_two();",
        ],
        "fn go() {",
    );
    let intents = two_declared_records(
        "svc.rs",
        &[
            "    let alpha_one = compute_alpha_one();",
            "    let alpha_two = compute_alpha_two();",
        ],
        &[
            "    let beta_one = compute_beta_one();",
            "    let beta_two = compute_beta_two();",
        ],
    );

    let groups = with_intent(&[d], &intents);

    let intents_cards: Vec<_> = groups
        .iter()
        .filter(|g| g.kind == GroupKind::Intent)
        .collect();
    assert_eq!(intents_cards.len(), 2, "one card per declared intent");
    // Neither is an ambiguous candidates card.
    assert!(groups.iter().all(|g| g.candidates.is_empty()));

    let by_label = |label: &str| {
        intents_cards
            .iter()
            .find(|g| g.label == label)
            .unwrap_or_else(|| panic!("missing card for {label}"))
    };
    // Each card carries exactly its own two lines — the split is by evidence,
    // not by duplicating the whole hunk onto both.
    assert_eq!(
        by_label("make cohort filter sargable").files[0].line_indices,
        vec![0, 1]
    );
    assert_eq!(
        by_label("trust DB for gate").files[0].line_indices,
        vec![2, 3]
    );
}

/// Lines the matcher tied to no declared intent, when a hunk is split between
/// intents, are left as their own location card — never re-attached to one of
/// the split intents, which would claim lines it has no evidence for.
#[test]
fn unattributed_remainder_of_a_split_hunk_is_not_claimed_by_either_intent() {
    let d = simple(
        "svc.rs",
        &[
            "+    let alpha_one = compute_alpha_one();",
            "+    let alpha_two = compute_alpha_two();",
            "+    let beta_one = compute_beta_one();",
            "+    let beta_two = compute_beta_two();",
            "+    let orphan = something_unrecorded();",
        ],
        "fn go() {",
    );
    let intents = two_declared_records(
        "svc.rs",
        &[
            "    let alpha_one = compute_alpha_one();",
            "    let alpha_two = compute_alpha_two();",
        ],
        &[
            "    let beta_one = compute_beta_one();",
            "    let beta_two = compute_beta_two();",
        ],
    );

    let groups = with_intent(&[d], &intents);

    // The orphan line (index 4) belongs to neither intent card.
    for group in groups.iter().filter(|g| g.kind == GroupKind::Intent) {
        assert!(!group.files[0].line_indices.contains(&4));
    }
    // It lands on a non-intent card instead.
    assert!(groups.iter().any(
        |g| g.kind != GroupKind::Intent && g.files.iter().any(|f| f.line_indices.contains(&4))
    ));
}

/// A reason that already has an evidenced (bound) card must not also receive
/// unrelated ambiguous lines. Unevidenced plausible reasons each get their own
/// card carrying the ambiguous lines.
#[test]
fn an_evidenced_reason_is_not_repeated_as_an_ambiguous_candidate() {
    // Hunk 1 matches reason R's recorded edit → R binds and gets its own card.
    // Hunk 2 matches nothing, but R, S and T all scope the file by path. R is
    // already evidenced, so hunk 2's ambiguous card lists only S and T.
    let d = file(
        "f.rs",
        vec![
            {
                let mut i = 0;
                hunk(
                    &["+    let bound = compute_bound_value();"],
                    &mut i,
                    "fn one() {",
                )
            },
            {
                let mut i = 10;
                hunk(
                    &["+    let mystery = 1;", "+    let puzzle = 2;"],
                    &mut i,
                    "fn two() {",
                )
            },
        ],
    );

    let record = IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "tR".into(),
        tool_use_id: "tool-R".into(),
        seq: 1,
        path: "f.rs".into(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: vec!["    let bound = compute_bound_value();".into()],
            whole_file: false,
        },
        branch: None,
    };
    let label = |turn: &str, reason: &str| IntentLabel {
        provider: ProviderId::ClaudeCode,
        turn_id: turn.into(),
        label: reason.into(),
        paths: vec!["f.rs".into()],
        anchor: None,
        source: LabelSource::Declared,
        self_confidence: None,
    };
    let intents = Intents {
        records: vec![record],
        labels: vec![
            label("tR", "Remove redundant flight plan fetches"),
            label("tS", "move read tracking to owning page"),
            label("tT", "cancel superseded table reads"),
        ],
    };

    let groups = with_intent(&[d], &intents);

    // R has its own card, with no candidates.
    let bound = groups
        .iter()
        .find(|g| g.label == "Remove redundant flight plan fetches")
        .expect("R should have its own evidenced card");
    assert!(bound.candidates.is_empty());

    assert!(groups.iter().all(|g| g.candidates.is_empty()));
    for reason in [
        "move read tracking to owning page",
        "cancel superseded table reads",
    ] {
        let card = groups
            .iter()
            .find(|g| g.label == reason)
            .expect("missing intent card");
        assert_eq!(card.files[0].line_indices, vec![10, 11]);
    }
    assert_eq!(bound.files[0].line_indices, vec![0]);
}

#[test]
fn identical_declared_intent_text_merges_across_turns() {
    let diffs = [
        simple("a.rs", &["+    let a = 1;"], "fn a() {"),
        simple("b.rs", &["+    let b = 2;"], "fn b() {"),
    ];
    let intents = scoped_labels(&[
        ("turn-a", "support the shared workflow", &["a.rs"]),
        ("turn-b", "support the shared workflow", &["b.rs"]),
    ]);

    let groups = with_intent(&diffs, &intents);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "support the shared workflow");
    assert_eq!(groups[0].files.len(), 2);
}

/// The reported Autocomplete bug: two declared reasons both scope the file's
/// directory, so neither binds uniquely. Each reason remains a separate card,
/// with the genuinely ambiguous line visible in both.
#[test]
fn two_covering_reasons_remain_separate_intent_cards() {
    let d = simple(
        "dir/Autocomplete.razor.cs",
        &["+    private CancellationTokenSource _cts = new();"],
        "class Autocomplete {",
    );
    let intents = scoped_labels(&[
        (
            "a",
            "move per-search cancellation into Autocomplete",
            &["dir"],
        ),
        ("b", "move read tracking to owning page", &["dir"]),
    ]);

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|group| group.kind == GroupKind::Intent));
    assert!(groups.iter().all(|group| group.candidates.is_empty()));
    assert!(groups
        .iter()
        .all(|group| group.files[0].line_indices == vec![0]));
    let labels: BTreeSet<_> = groups.iter().map(|group| group.label.as_str()).collect();
    assert_eq!(
        labels,
        BTreeSet::from([
            "move per-search cancellation into Autocomplete",
            "move read tracking to owning page",
        ])
    );
}

/// The override must not touch a hunk that is pure formatting — that is
/// decidable and a stronger statement than a broad directory reason.
#[test]
fn a_covering_reason_does_not_relabel_formatting() {
    // Same tokens, only indentation changed → formatting.
    let d = simple("dir/x.rs", &["-let a=1;", "+  let a=1;"], "");
    let intents = scoped_labels(&[("t1", "some broad refactor", &["dir"])]);

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups[0].kind, GroupKind::Formatting);
}

/// A reason that scopes a *different* directory must not claim this file.
#[test]
fn a_reason_scoped_elsewhere_does_not_claim_the_file() {
    let d = simple("dir/x.rs", &["+    let a = 1;"], "");
    let intents = scoped_labels(&[("t1", "unrelated reason", &["other"])]);

    let groups = with_intent(&[d], &intents);

    assert_ne!(groups[0].kind, GroupKind::Intent);
    assert!(groups[0].candidates.is_empty());
}

/// The grouping is the valuable part and survives having no reason at all: the
/// files still changed together in one turn, and saying so is honest.
#[test]
fn a_turn_with_no_label_still_groups_its_files_into_one_card() {
    let first = simple(
        "a.rs",
        &["+    let alpha = a_distinctive_first_call();"],
        "",
    );
    let second = simple(
        "b.rs",
        &["+    let beta = a_distinctive_second_call();"],
        "",
    );

    let intents = Intents {
        records: vec![
            unlabelled_record("a.rs", &["    let alpha = a_distinctive_first_call();"], 1),
            unlabelled_record("b.rs", &["    let beta = a_distinctive_second_call();"], 2),
        ],
        labels: Vec::new(),
    };

    let groups = with_intent(&[first, second], &intents);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::SameTurn);
    assert_eq!(groups[0].files.len(), 2);
    // Titled from what changed, never from a reason nobody gave.
    assert!(
        groups[0].label.contains("2 files"),
        "unexpected label: {}",
        groups[0].label
    );
}

#[test]
fn a_turn_with_no_label_in_one_symbol_is_titled_with_that_symbol() {
    let d = simple(
        "a.rs",
        &["+    let retry_limit = read_configured_retry_limit();"],
        "fn refresh_token() {",
    );
    let intents = Intents {
        records: vec![unlabelled_record(
            "a.rs",
            &["    let retry_limit = read_configured_retry_limit();"],
            1,
        )],
        labels: Vec::new(),
    };

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups[0].kind, GroupKind::SameTurn);
    assert_eq!(groups[0].label, "refresh_token");
}

/// A derived title is a description, not a reason, and the card should not look
/// as certain as one the agent explained.
#[test]
fn a_card_titled_from_its_own_changes_is_not_high_confidence() {
    let d = simple(
        "a.rs",
        &["+    let alpha = a_distinctive_first_call();"],
        "",
    );
    let intents = Intents {
        records: vec![unlabelled_record(
            "a.rs",
            &["    let alpha = a_distinctive_first_call();"],
            1,
        )],
        labels: Vec::new(),
    };

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups[0].confidence, Confidence::Low);
}

fn unlabelled_record(path: &str, lines: &[&str], seq: u64) -> IntentRecord {
    IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "turn-1".into(),
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

/// Nothing beats being told why.
#[test]
fn a_recorded_label_is_preferred_over_the_enclosing_symbol() {
    let d = simple(
        "a.rs",
        &[
            "+    let retry_limit = read_configured_retry_limit();",
            "+    let backoff = compute_exponential_backoff(retry_limit);",
        ],
        "fn refresh_token() {",
    );
    let intents = record_with_label(
        "a.rs",
        &[
            "    let retry_limit = read_configured_retry_limit();",
            "    let backoff = compute_exponential_backoff(retry_limit);",
        ],
        "add retry to token refresh",
    );

    let groups = with_intent(&[d], &intents);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, GroupKind::Intent);
    assert_eq!(groups[0].label, "add retry to token refresh");
}

#[test]
fn one_intent_spanning_two_files_becomes_a_single_card() {
    let first = simple(
        "a.rs",
        &["+    let alpha = a_distinctive_first_call();"],
        "",
    );
    let second = simple(
        "b.rs",
        &["+    let beta = a_distinctive_second_call();"],
        "",
    );

    let record = IntentRecord {
        provider: ProviderId::ClaudeCode,
        turn_id: "turn-1".into(),
        tool_use_id: "tool-1".into(),
        seq: 1,
        path: "a.rs".into(),
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: vec!["    let alpha = a_distinctive_first_call();".into()],
            whole_file: false,
        },
        branch: None,
    };
    let mut other = record.clone();
    other.path = "b.rs".into();
    other.tool_use_id = "tool-2".into();
    other.seq = 2;
    other.edit.new_lines = vec!["    let beta = a_distinctive_second_call();".into()];

    let intents = Intents {
        records: vec![record, other],
        labels: vec![IntentLabel {
            provider: ProviderId::ClaudeCode,
            turn_id: "turn-1".into(),
            label: "one change, two files".into(),
            paths: Vec::new(),
            anchor: None,
            source: LabelSource::Declared,
            self_confidence: None,
        }],
    };

    let groups = with_intent(&[first, second], &intents);
    let intent_cards: Vec<_> = groups
        .iter()
        .filter(|g| g.kind == GroupKind::Intent)
        .collect();

    assert_eq!(intent_cards.len(), 1);
    assert_eq!(intent_cards[0].files.len(), 2);
}

#[test]
fn intent_cards_sort_before_locations_and_formatting() {
    let laballed = simple(
        "a.rs",
        &[
            "+    let retry_limit = read_configured_retry_limit();",
            "+    let backoff = compute_exponential_backoff(retry_limit);",
        ],
        "",
    );
    let formatting = simple("b.rs", &["-  let x = 1;", "+let x = 1;"], "");

    let intents = record_with_label(
        "a.rs",
        &[
            "    let retry_limit = read_configured_retry_limit();",
            "    let backoff = compute_exponential_backoff(retry_limit);",
        ],
        "add retry",
    );

    let groups = with_intent(&[laballed, formatting], &intents);

    assert_eq!(groups[0].kind, GroupKind::Intent);
}

/// The reviewer signs off by reading top-down, so the card that carries the
/// most risk — a change nothing accounts for — must lead, not trail. This is
/// the deliberate reversal of the old "intent first" ordering.
#[test]
fn unexplained_cards_sort_to_the_top() {
    // A stated intent and an unexplained hunk in a config file with no symbol.
    let stated = simple(
        "a.rs",
        &["+    let retry_limit = read_configured_retry_limit();"],
        "",
    );
    let unexplained = simple("config.json", &["+  \"unaccounted\": true,"], "");

    let intents = record_with_label(
        "a.rs",
        &["    let retry_limit = read_configured_retry_limit();"],
        "add retry",
    );

    let groups = with_intent(&[stated, unexplained], &intents);

    assert_eq!(groups[0].kind, GroupKind::Other);
    assert!(
        groups.iter().any(|g| g.kind == GroupKind::Intent),
        "the stated intent card should still be present"
    );
}

/// Formatting changed no code, so it stays the one kind safe to skim — last,
/// below even the unexplained cards now promoted above it.
#[test]
fn formatting_still_sorts_last_below_unexplained() {
    let unexplained = simple("config.json", &["+  \"unaccounted\": true,"], "");
    let formatting = simple("b.rs", &["-  let x = 1;", "+let x = 1;"], "");

    let groups = group_without_intent(&[unexplained, formatting]);

    assert_eq!(groups.last().unwrap().kind, GroupKind::Formatting);
    assert_eq!(groups[0].kind, GroupKind::Other);
}

// -- shape of the result ----------------------------------------------------

#[test]
fn a_group_reports_every_changed_line_it_covers() {
    let d = simple("a.rs", &["+fn thing() {", "+    body();", "+}"], "");

    let groups = group_without_intent(&[d]);

    assert_eq!(groups[0].line_count, 3);
    assert_eq!(groups[0].files[0].line_indices, vec![0, 1, 2]);
}

#[test]
fn context_lines_are_never_included_in_a_group() {
    let d = simple("a.rs", &[" context", "+fn thing() {}", " more context"], "");

    let groups = group_without_intent(&[d]);

    assert_eq!(groups[0].files[0].line_indices, vec![1]);
}

#[test]
fn a_diff_with_no_hunks_produces_no_groups() {
    let d = file("a.rs", Vec::new());

    assert!(group_without_intent(&[d]).is_empty());
}

#[test]
fn grouping_is_deterministic() {
    let diffs = vec![
        simple("a.rs", &["+fn alpha_function() {}"], ""),
        simple("b.rs", &["-  let x = 1;", "+let x = 1;"], ""),
        simple("c.rs", &["+    changed();"], "fn beta_function() {"),
    ];

    assert_eq!(group_without_intent(&diffs), group_without_intent(&diffs));
}

// -- the IPC contract -------------------------------------------------------

/// `src/ipc/types.ts` is written by hand against these names, so a rename on
/// this side has to fail somewhere visible rather than surfacing as an
/// undefined value in the UI. See `docs/architecture/ipc-contract.md`.
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
fn an_intent_group_serialises_with_the_keys_the_ui_reads() {
    let group = IntentGroup {
        id: "intent:turn-1".into(),
        kind: GroupKind::Intent,
        label: "add retry".into(),
        candidates: Vec::new(),
        symbol: Some("refresh".into()),
        files: vec![GroupFile {
            path: "a.rs".into(),
            line_indices: vec![0, 1],
            hunks: vec![0],
        }],
        line_count: 2,
        confidence: Confidence::High,
        self_confidence: None,
        user_authored: false,
    };

    let value = serde_json::to_value(&group).unwrap();

    assert_eq!(
        keys(&value),
        [
            "confidence",
            "files",
            "id",
            "kind",
            "label",
            "lineCount",
            "symbol"
        ]
    );
    assert_eq!(keys(&value["files"][0]), ["hunks", "lineIndices", "path"]);
}

/// An absent symbol must be *absent*, not null: the mirror types it optional.
#[test]
fn a_group_without_a_symbol_omits_the_key_rather_than_sending_null() {
    let group = IntentGroup {
        id: "formatting".into(),
        kind: GroupKind::Formatting,
        label: "Whitespace only".into(),
        candidates: Vec::new(),
        symbol: None,
        files: Vec::new(),
        line_count: 0,
        confidence: Confidence::High,
        self_confidence: None,
        user_authored: false,
    };

    assert!(!keys(&serde_json::to_value(&group).unwrap()).contains(&"symbol".to_string()));
}

/// The agent's self-reported confidence crosses IPC under the camelCase key
/// `selfConfidence`, present only when the agent stated one — absent (never
/// null) otherwise, so the optional mirror reads it correctly.
#[test]
fn self_confidence_appears_only_when_the_agent_stated_one() {
    let base = IntentGroup {
        id: "intent:t1:x".into(),
        kind: GroupKind::Intent,
        label: "x".into(),
        candidates: Vec::new(),
        symbol: None,
        files: Vec::new(),
        line_count: 0,
        confidence: Confidence::High,
        self_confidence: None,
        user_authored: false,
    };

    let absent = serde_json::to_value(&base).unwrap();
    assert!(!keys(&absent).contains(&"selfConfidence".to_string()));

    let stated = IntentGroup {
        self_confidence: Some(SelfConfidence::Low),
        ..base
    };
    let value = serde_json::to_value(&stated).unwrap();
    assert!(keys(&value).contains(&"selfConfidence".to_string()));
    assert_eq!(value["selfConfidence"], "low");
}

/// Candidates are omitted when empty (the normal case) and present when the
/// card is ambiguous — the UI reads the key only when it is there.
#[test]
fn candidates_key_appears_only_when_non_empty() {
    let base = IntentGroup {
        id: "x".into(),
        kind: GroupKind::Intent,
        label: String::new(),
        candidates: Vec::new(),
        symbol: None,
        files: Vec::new(),
        line_count: 0,
        confidence: Confidence::Low,
        self_confidence: None,
        user_authored: false,
    };

    let empty = serde_json::to_value(&base).unwrap();
    assert!(!keys(&empty).contains(&"candidates".to_string()));

    let ambiguous = IntentGroup {
        candidates: vec!["reason one".into(), "reason two".into()],
        ..base
    };
    let value = serde_json::to_value(&ambiguous).unwrap();
    assert!(keys(&value).contains(&"candidates".to_string()));
    assert_eq!(value["candidates"][0], "reason one");
}

#[test]
fn group_kinds_serialise_in_camel_case() {
    let json = serde_json::to_string(&[
        GroupKind::Intent,
        GroupKind::SameTurn,
        GroupKind::Formatting,
        GroupKind::NewSymbol,
        GroupKind::ModifiedSymbol,
        GroupKind::Other,
    ])
    .unwrap();

    assert_eq!(
        json,
        r#"["intent","sameTurn","formatting","newSymbol","modifiedSymbol","other"]"#
    );
}

/// `LabelSource` never crosses IPC, but it is persisted in `labels.jsonl` and
/// so has the same compatibility obligation: records written before the field
/// existed must load, and must load as *inferred* — they came overwhelmingly
/// from the first-sentence fallback and must not be promoted to declared.
#[test]
fn a_label_recorded_before_the_source_field_existed_reads_as_inferred() {
    let json = r#"{
        "provider": "claudeCode",
        "turnId": "turn-1",
        "label": "an older record"
    }"#;

    let label: IntentLabel = serde_json::from_str(json).unwrap();

    assert_eq!(label.source, LabelSource::Inferred);
}

#[test]
fn label_sources_serialise_in_camel_case() {
    let json = serde_json::to_string(&[LabelSource::Declared, LabelSource::Inferred]).unwrap();

    assert_eq!(json, r#"["declared","inferred"]"#);
}

#[test]
fn confidence_serialises_in_camel_case() {
    let json =
        serde_json::to_string(&[Confidence::Low, Confidence::Medium, Confidence::High]).unwrap();

    assert_eq!(json, r#"["low","medium","high"]"#);
}

/// Twelve hunks becoming four decisions is the entire point of the feature.
#[test]
fn many_hunks_collapse_into_few_cards() {
    let mut diffs = Vec::new();

    // Four files reformatted.
    for name in ["f1.rs", "f2.rs", "f3.rs", "f4.rs"] {
        diffs.push(simple(name, &["-  let x = 1;", "+let x = 1;"], ""));
    }
    // Four hunks inside one new method, spread over two files.
    for name in ["a.rs", "b.rs"] {
        let mut index = 0;
        diffs.push(file(
            name,
            vec![
                hunk(&["+    one();"], &mut index, "fn estimate_cost() {"),
                hunk(&["+    two();"], &mut index, "fn estimate_cost() {"),
            ],
        ));
    }

    let groups = group_without_intent(&diffs);

    assert_eq!(
        groups.len(),
        2,
        "got {:?}",
        groups.iter().map(|g| &g.label).collect::<Vec<_>>()
    );
    assert_eq!(groups.iter().map(IntentGroup::hunk_count).sum::<usize>(), 8);
}
