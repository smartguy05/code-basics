//! Tests for rule-doc parsing and loading.
//! Included by `mod.rs` under `#[cfg(test)]`.

use super::*;
use std::path::Path;

const WITH_FRONT_MATTER: &str = "\
---
id: money-minor-units
title: Money is stored in minor units
---
Every monetary amount is an integer count of the currency's minor unit
(cents), never a float.
";

#[test]
fn parses_id_title_and_body_from_front_matter() {
    let doc = parse_rule_doc(WITH_FRONT_MATTER, "fallback");

    assert_eq!(doc.id, "money-minor-units");
    assert_eq!(doc.title, "Money is stored in minor units");
    assert!(doc.body.starts_with("Every monetary amount"));
    // The front-matter fence is stripped from the body.
    assert!(!doc.body.contains("---"));
    assert!(!doc.body.contains("id:"));
}

#[test]
fn a_file_with_no_front_matter_uses_the_stem_as_id_and_the_first_heading_as_title() {
    let text = "# Authorise every endpoint\n\nEvery public endpoint checks the caller.\n";
    let doc = parse_rule_doc(text, "authz");

    assert_eq!(doc.id, "authz", "the stem is the abstain-safe id");
    assert_eq!(
        doc.title, "Authorise every endpoint",
        "the first heading titles it"
    );
    assert!(doc.body.contains("Every public endpoint"));
}

#[test]
fn a_file_with_neither_front_matter_nor_a_heading_falls_back_to_the_stem_for_both() {
    let doc = parse_rule_doc("just some prose, no heading at all\n", "invariants");

    assert_eq!(doc.id, "invariants");
    assert_eq!(
        doc.title, "invariants",
        "nothing to title it, so the stem stands in"
    );
    assert_eq!(doc.body, "just some prose, no heading at all");
}

#[test]
fn a_blank_id_or_title_in_front_matter_falls_back_rather_than_being_taken() {
    let text = "\
---
id:
title:
---
# The Heading
Body.
";
    let doc = parse_rule_doc(text, "stem");
    assert_eq!(doc.id, "stem", "a blank id is not taken");
    assert_eq!(
        doc.title, "The Heading",
        "a blank title falls through to the heading"
    );
}

#[test]
fn rule_doc_serialises_with_the_keys_the_ui_reads() {
    let doc = RuleDoc {
        id: "id".into(),
        title: "title".into(),
        body: "body".into(),
    };
    let value = serde_json::to_value(&doc).unwrap();
    let mut keys: Vec<String> = value
        .as_object()
        .expect("a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();

    assert_eq!(keys, ["body", "id", "title"]);
}

#[test]
fn rules_report_serialises_with_the_keys_the_ui_reads() {
    let report = RulesReport {
        rules: Vec::new(),
        warnings: Vec::new(),
    };
    let value = serde_json::to_value(&report).unwrap();
    let mut keys: Vec<String> = value
        .as_object()
        .expect("a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();

    assert_eq!(keys, ["rules", "warnings"]);
}

#[test]
fn rules_dir_is_under_the_config_directory() {
    let root = Path::new("/repo");
    assert_eq!(rules_dir(root), Path::new("/repo/.code-basics/rules"));
}

#[test]
fn a_missing_directory_loads_as_empty_with_no_warning() {
    let dir = tempfile::tempdir().unwrap();
    let (rules, warnings) = load_rules(&dir.path().join("does-not-exist"));

    assert!(rules.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn loads_every_markdown_file_sorted_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "# Beta\nsecond\n").unwrap();
    std::fs::write(dir.path().join("a.md"), "# Alpha\nfirst\n").unwrap();
    // A non-markdown file is ignored.
    std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

    let (rules, warnings) = load_rules(dir.path());

    assert!(warnings.is_empty());
    let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"], "sorted by path, deterministic");
    assert_eq!(rules[0].title, "Alpha");
    assert_eq!(rules[1].title, "Beta");
}
