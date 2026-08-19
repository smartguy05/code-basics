//! Tests for erosion rule loading and the built-in set.
//! Included by `rules.rs` under `#[cfg(test)]`.

use super::*;
use std::path::Path;

const CUSTOM: &str = r#"
[[rule]]
id = "no-fire-and-forget"
category = "widenedCatch"
side = "added"
pattern = 'Task\.Run\('
message = "Fire-and-forget Task.Run swallows failures."
extensions = [".cs"]

[[rule]]
id = "no-todo"
category = "leftoverStub"
side = "added"
pattern = '\bTODO\b'
message = "TODO left in the diff."
prodOnly = true
"#;

#[test]
fn parses_rules() {
    let rules = parse(CUSTOM).unwrap();

    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].id, "no-fire-and-forget");
    assert_eq!(rules[0].category, ErosionCategory::WidenedCatch);
    assert_eq!(rules[0].side, RuleSide::Added);
    assert_eq!(rules[0].extensions, vec![".cs"]);
    assert!(!rules[0].prod_only);
    assert!(rules[1].prod_only);
}

#[test]
fn a_rule_without_an_id_or_pattern_is_rejected() {
    assert!(parse(
        r#"[[rule]]
id = ""
category = "leftoverStub"
side = "added"
pattern = "x"
message = "m""#
    )
    .is_err());

    assert!(parse(
        r#"[[rule]]
id = "x"
category = "leftoverStub"
side = "added"
pattern = ""
message = "m""#
    )
    .is_err());
}

#[test]
fn load_dir_reports_bad_ones() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("custom.toml"), CUSTOM).unwrap();
    std::fs::write(dir.path().join("broken.toml"), "[[rule]]\nid = ").unwrap();
    std::fs::write(dir.path().join("notes.md"), "ignored").unwrap();

    let (rules, errors) = load_dir(dir.path());

    assert_eq!(rules.len(), 2, "the valid file must still load");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("broken.toml"));
}

#[test]
fn a_missing_directory_loads_nothing_without_erroring() {
    let (rules, errors) = load_dir(Path::new("/nonexistent/erosion"));
    assert!(rules.is_empty());
    assert!(errors.is_empty());
}

/// Rules are per-workspace, like declarative adapters — a rule is part of the
/// repository that needs it.
#[test]
fn rules_read_from_the_workspaces_own_config_directory() {
    let root = Path::new("/repo");
    assert_eq!(rules_dir(root), Path::new("/repo/.code-basics/erosion"));
    assert!(rules_dir(root).starts_with(crate::config::config_dir(root)));
}

#[test]
fn builtin_rules_cover_each_ecosystem() {
    let rules = builtin_rules();
    let covers = |ext: &str| rules.iter().any(|r| r.extensions.iter().any(|e| e == ext));
    assert!(covers(".cs"), "expected a .NET rule");
    assert!(covers(".ts"), "expected a TS rule");
    assert!(covers(".rs"), "expected a Rust rule");
}

/// A user rule extends the built-ins rather than replacing them.
#[test]
fn all_rules_appends_user_rules_to_the_builtins() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("custom.toml"), CUSTOM).unwrap();

    // `all_rules` reads `<root>/.code-basics/erosion`, so lay the file out there.
    let root = dir.path();
    let erosion = rules_dir(root);
    std::fs::create_dir_all(&erosion).unwrap();
    std::fs::write(erosion.join("custom.toml"), CUSTOM).unwrap();

    let (rules, errors) = all_rules(root);

    assert!(errors.is_empty());
    assert!(rules.len() > builtin_rules().len());
    assert!(rules.iter().any(|r| r.id == "no-fire-and-forget"));
}

#[test]
fn a_bad_regex_becomes_a_warning_not_a_panic() {
    let rules = vec![ErosionRule {
        id: "bad".into(),
        category: ErosionCategory::UnsafeCast,
        side: RuleSide::Added,
        pattern: "(".into(),
        message: "m".into(),
        extensions: Vec::new(),
        prod_only: false,
    }];

    let (compiled, warnings) = compile(&rules);

    assert!(compiled.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("bad"));
}

#[test]
fn categories_serialise_in_camel_case() {
    let json = serde_json::to_string(&[
        ErosionCategory::DeletedAssertion,
        ErosionCategory::IgnoredTest,
        ErosionCategory::WidenedCatch,
        ErosionCategory::RemovedNullCheck,
        ErosionCategory::UnsafeCast,
        ErosionCategory::LeftoverStub,
        ErosionCategory::RemovedSafeguard,
        ErosionCategory::DroppedLog,
        ErosionCategory::Secret,
    ])
    .unwrap();

    assert_eq!(
        json,
        r#"["deletedAssertion","ignoredTest","widenedCatch","removedNullCheck","unsafeCast","leftoverStub","removedSafeguard","droppedLog","secret"]"#
    );
}

/// Secret detection ships out of the box; each rule reads the added side, since
/// removing a leaked key is not a new leak.
#[test]
fn builtin_rules_include_secret_detectors() {
    let rules = builtin_rules();
    let secret: Vec<&ErosionRule> = rules
        .iter()
        .filter(|r| r.category == ErosionCategory::Secret)
        .collect();

    assert!(!secret.is_empty(), "expected built-in secret rules");
    assert!(
        secret.iter().all(|r| r.side == RuleSide::Added),
        "secret rules read the added side"
    );
    // A secret is a leak wherever it lands, including a test fixture.
    assert!(
        secret.iter().all(|r| !r.prod_only),
        "secret rules are not prod-only"
    );
}
