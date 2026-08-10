//! Tests for collapsing hunks into cards.
//! Included by `grouping.rs` under `#[cfg(test)]`.

use super::*;
use crate::git::attribution::{self, Options};
use crate::git::patch::{DiffLine, FileDiff};
use crate::intents::{IntentEdit, IntentLabel, IntentRecord, Intents, ProviderId};

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
    group(diffs, &attributions)
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

#[test]
fn reordering_lines_without_changing_them_is_formatting_only() {
    let d = simple(
        "a.rs",
        &["-use beta;", "-use alpha;", "+use alpha;", "+use beta;"],
        "",
    );

    assert!(is_formatting_only(&d.hunks[0], &d.path));
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
    group(diffs, &attributions)
}

fn record_with_label(path: &str, lines: &[&str], label: &str) -> Intents {
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
        }],
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
fn intent_cards_sort_before_everything_else() {
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
        symbol: Some("refresh".into()),
        files: vec![GroupFile {
            path: "a.rs".into(),
            line_indices: vec![0, 1],
            hunks: vec![0],
        }],
        line_count: 2,
        confidence: Confidence::High,
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
        symbol: None,
        files: Vec::new(),
        line_count: 0,
        confidence: Confidence::High,
    };

    assert!(!keys(&serde_json::to_value(&group).unwrap()).contains(&"symbol".to_string()));
}

#[test]
fn group_kinds_serialise_in_camel_case() {
    let json = serde_json::to_string(&[
        GroupKind::Intent,
        GroupKind::Formatting,
        GroupKind::NewSymbol,
        GroupKind::ModifiedSymbol,
        GroupKind::Other,
    ])
    .unwrap();

    assert_eq!(
        json,
        r#"["intent","formatting","newSymbol","modifiedSymbol","other"]"#
    );
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
