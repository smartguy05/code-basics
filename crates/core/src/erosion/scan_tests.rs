//! Tests for the erosion diff scan.
//! Included by `scan.rs` under `#[cfg(test)]`.

use super::*;
use crate::erosion::rules::{ErosionCategory, ErosionRule, RuleSide};
use crate::git::patch::{DiffLine, FileDiff, Hunk, LineOrigin};

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

fn file(path: &str, lines: &[&str]) -> FileDiff {
    let mut index = 0;
    FileDiff {
        path: path.to_string(),
        old_path: None,
        hunks: vec![hunk(lines, &mut index)],
        is_binary: false,
    }
}

fn rule(id: &str, category: ErosionCategory, side: RuleSide, pattern: &str, exts: &[&str]) -> ErosionRule {
    ErosionRule {
        id: id.into(),
        category,
        side,
        pattern: pattern.into(),
        message: format!("{id} fired"),
        extensions: exts.iter().map(|s| s.to_string()).collect(),
        prod_only: false,
    }
}

// -- side discipline --------------------------------------------------------

#[test]
fn a_deleted_assertion_is_flagged() {
    let diff = file("Foo.cs", &["-    Assert.Equal(expected, actual);", "+    // gone"]);
    let rules = vec![rule(
        "deleted-assert",
        ErosionCategory::DeletedAssertion,
        RuleSide::Removed,
        r"Assert\.",
        &[".cs"],
    )];

    let report = scan_diffs(&[diff], &rules);

    assert_eq!(report.flags.len(), 1);
    let flag = &report.flags[0];
    assert_eq!(flag.category, ErosionCategory::DeletedAssertion);
    assert_eq!(flag.rule_id, "deleted-assert");
    assert_eq!(flag.origin, LineOrigin::Deletion);
    // A removed line is cited by its old line number.
    assert_eq!(flag.line, 1);
}

#[test]
fn an_introduced_unwrap_is_flagged() {
    let diff = file("a.rs", &["+    let value = thing.unwrap();"]);
    let rules = vec![rule(
        "no-unwrap",
        ErosionCategory::UnsafeCast,
        RuleSide::Added,
        r"\.unwrap\(\)",
        &[".rs"],
    )];

    let report = scan_diffs(&[diff], &rules);

    assert_eq!(report.flags.len(), 1);
    assert_eq!(report.flags[0].origin, LineOrigin::Addition);
    assert_eq!(report.flags[0].line, 1);
    // The click target is the diff-line index, so the panel can highlight it.
    assert_eq!(report.flags[0].index, 0);
}

#[test]
fn a_matching_context_line_is_never_flagged() {
    let diff = file("a.rs", &[" let value = thing.unwrap();", "+let added = 1;"]);
    let rules = vec![rule(
        "no-unwrap",
        ErosionCategory::UnsafeCast,
        RuleSide::Added,
        r"\.unwrap\(\)",
        &[".rs"],
    )];

    let report = scan_diffs(&[diff], &rules);

    assert!(report.flags.is_empty(), "context lines are not changes");
}

#[test]
fn a_removed_line_is_not_matched_by_an_added_side_rule() {
    // Removing an `.unwrap()` is the opposite of introducing one.
    let diff = file("a.rs", &["-    let value = thing.unwrap();"]);
    let rules = vec![rule(
        "no-unwrap",
        ErosionCategory::UnsafeCast,
        RuleSide::Added,
        r"\.unwrap\(\)",
        &[".rs"],
    )];

    let report = scan_diffs(&[diff], &rules);

    assert!(report.flags.is_empty());
}

// -- secrets ----------------------------------------------------------------

#[test]
fn an_introduced_secret_is_flagged() {
    let diff = file("config.rs", &[r#"+    let key = "AKIAIOSFODNN7EXAMPLE";"#]);
    let rules = vec![rule(
        "aws-access-key",
        ErosionCategory::Secret,
        RuleSide::Added,
        r"AKIA[0-9A-Z]{16}",
        &[],
    )];

    let report = scan_diffs(&[diff], &rules);

    assert_eq!(report.flags.len(), 1);
    assert_eq!(report.flags[0].category, ErosionCategory::Secret);
    assert_eq!(report.flags[0].origin, LineOrigin::Addition);
    assert_eq!(report.flags[0].line, 1);
    // The click target is the diff-line index, so the panel can highlight it.
    assert_eq!(report.flags[0].index, 0);
}

#[test]
fn the_same_secret_on_a_removed_or_context_line_is_not_flagged() {
    // Removing a leaked key, or a key sitting on an unchanged line, is not a
    // newly introduced secret. Secret rules read the added side only.
    let removed = file("config.rs", &[r#"-    let key = "AKIAIOSFODNN7EXAMPLE";"#]);
    let context = file(
        "config.rs",
        &[
            r#"    let key = "AKIAIOSFODNN7EXAMPLE";"#,
            "+    let unrelated = 1;",
        ],
    );
    let rules = vec![rule(
        "aws-access-key",
        ErosionCategory::Secret,
        RuleSide::Added,
        r"AKIA[0-9A-Z]{16}",
        &[],
    )];

    let report = scan_diffs(&[removed, context], &rules);

    assert!(report.flags.is_empty());
}

/// The SHIPPED secret rules (not a test stand-in) must catch real leaks yet stay
/// silent on the placeholders and env-var indirections that fill config
/// templates and fixtures — a false secret flag is worse than none.
#[test]
fn the_builtin_secret_rules_flag_leaks_but_not_placeholders() {
    let secret_rules: Vec<ErosionRule> = crate::erosion::builtin_rules()
        .into_iter()
        .filter(|r| r.category == ErosionCategory::Secret)
        .collect();
    assert!(!secret_rules.is_empty(), "there should be built-in secret rules");

    // Real leaks on added lines — each distinct line should be flagged.
    let leaks = file(
        "config.rs",
        &[
            r#"+    let k = "AKIAIOSFODNN7EXAMPLE";"#,
            r#"+    let gh = "ghp_0123456789abcdefghijklmnopqrstuvwx";"#,
            r#"+    apiKey = "sk-live-0123456789abcdef";"#,
            r#"+    conn = "Server=db;Password=P@ssw0rd123;";"#,
        ],
    );
    let leak_report = scan_diffs(&[leaks], &secret_rules);
    assert_eq!(
        leak_report.flags.len(),
        4,
        "each planted leak should be flagged once, got {:?}",
        leak_report.flags
    );

    // Placeholders and env-var / template indirections on added lines — none of
    // these is a hardcoded credential, so none must fire.
    let innocuous = file(
        "config.rs",
        &[
            r#"+    password = "changeme";"#,
            r#"+    token = "placeholder";"#,
            r#"+    password: "required field","#,
            r#"+    conn = "Password=${DB_PASSWORD};";"#,
            r#"+    conn = "Password=<your-password>;";"#,
            r#"+    conn = "pwd=%DB_PWD%;";"#,
        ],
    );
    let innocuous_report = scan_diffs(&[innocuous], &secret_rules);
    assert!(
        innocuous_report.flags.is_empty(),
        "no placeholder/indirection should be flagged, got {:?}",
        innocuous_report.flags
    );
}

// -- path scoping -----------------------------------------------------------

#[test]
fn a_rule_only_applies_to_its_declared_extensions() {
    let cs = file("Foo.cs", &["+    // TODO later"]);
    let rs = file("a.rs", &["+    // TODO later"]);
    let rules = vec![rule(
        "cs-only",
        ErosionCategory::LeftoverStub,
        RuleSide::Added,
        r"\bTODO\b",
        &[".cs"],
    )];

    let report = scan_diffs(&[cs, rs], &rules);

    assert_eq!(report.flags.len(), 1);
    assert_eq!(report.flags[0].path, "Foo.cs");
}

#[test]
fn a_prod_only_rule_skips_test_files_but_fires_in_production() {
    let mut prod_rule = rule(
        "no-todo",
        ErosionCategory::LeftoverStub,
        RuleSide::Added,
        r"\bTODO\b",
        &[],
    );
    prod_rule.prod_only = true;

    let prod = file("src/service.ts", &["+  // TODO handle this"]);
    let test = file("src/service.test.ts", &["+  // TODO handle this"]);

    let report = scan_diffs(&[test, prod], &[prod_rule]);

    assert_eq!(report.flags.len(), 1);
    assert_eq!(report.flags[0].path, "src/service.ts");
}

// -- deduplication ----------------------------------------------------------

#[test]
fn two_rules_of_one_category_do_not_double_flag_a_line() {
    let diff = file("a.rs", &["+    let value = thing.unwrap();"]);
    let rules = vec![
        rule("a", ErosionCategory::UnsafeCast, RuleSide::Added, r"\.unwrap", &[".rs"]),
        rule("b", ErosionCategory::UnsafeCast, RuleSide::Added, r"unwrap\(\)", &[".rs"]),
    ];

    let report = scan_diffs(&[diff], &rules);

    assert_eq!(report.flags.len(), 1, "one category, one flag per line");
}

// -- warnings ---------------------------------------------------------------

#[test]
fn a_bad_regex_is_reported_in_the_reports_warnings() {
    let bad = rule("bad", ErosionCategory::UnsafeCast, RuleSide::Added, "(", &[]);
    let diff = file("a.rs", &["+something"]);

    let report = scan_diffs(&[diff], &[bad]);

    assert!(report.flags.is_empty());
    assert_eq!(report.warnings.len(), 1);
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
fn an_erosion_flag_serialises_with_the_keys_the_ui_reads() {
    let flag = ErosionFlag {
        path: "a.rs".into(),
        line: 12,
        index: 3,
        origin: LineOrigin::Addition,
        category: ErosionCategory::UnsafeCast,
        rule_id: "no-unwrap".into(),
        message: "m".into(),
        content: "let x = y.unwrap();".into(),
    };

    assert_eq!(
        keys(&serde_json::to_value(&flag).unwrap()),
        ["category", "content", "index", "line", "message", "origin", "path", "ruleId"]
    );
}

#[test]
fn an_erosion_report_serialises_with_the_keys_the_ui_reads() {
    let report = ErosionReport {
        flags: Vec::new(),
        warnings: Vec::new(),
    };

    assert_eq!(
        keys(&serde_json::to_value(&report).unwrap()),
        ["flags", "warnings"]
    );
}
