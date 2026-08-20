//! Tests for the combined first-open setup plan. The point that matters: the
//! Claude Code settings.json ends up carrying BOTH the intent and quality-gate
//! markers in a single write, so applying the plan installs both without one
//! clobbering the other.

use super::*;
use crate::intents::providers::{apply_writes, claude_code::ClaudeCode, hooks_json};
use crate::qgate;
use std::fs;
use tempfile::tempdir;

fn detected_claude(home: &Path) -> Vec<Box<dyn Provider>> {
    // `detected()` is true when the home dir exists.
    fs::create_dir_all(home).unwrap();
    vec![Box::new(ClaudeCode::with_home(home.to_path_buf())) as Box<dyn Provider>]
}

#[test]
fn combines_both_markers_into_one_settings_write() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let providers = detected_claude(home.path());

    let plan = setup_plan(
        root.path(),
        InstallScope::Project,
        &providers,
        Some(home.path()),
    )
    .unwrap();

    let settings = root.path().join(".claude").join("settings.json");
    let matching: Vec<_> = plan.writes.iter().filter(|w| w.path == settings).collect();
    assert_eq!(
        matching.len(),
        1,
        "exactly one settings.json write, not two"
    );
    let content = &matching[0].content;
    assert!(
        content.contains("code-basics-intent"),
        "carries the intent marker"
    );
    assert!(
        content.contains(qgate::install::MARKER),
        "carries the gate marker"
    );

    // The instruction splice is part of the plan too (proves intent capture is
    // fully included, not just its settings.json entry).
    assert!(
        plan.writes.iter().any(|w| w.path.ends_with("CLAUDE.md")),
        "intent capture's instruction write is included"
    );
}

#[test]
fn applying_the_plan_installs_both_hooks() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let providers = detected_claude(home.path());

    let plan = setup_plan(
        root.path(),
        InstallScope::Project,
        &providers,
        Some(home.path()),
    )
    .unwrap();
    apply_writes(&plan.writes).unwrap();

    let settings = root.path().join(".claude").join("settings.json");
    assert!(
        hooks_json::is_installed(&settings),
        "intent recorder installed"
    );
    assert_eq!(
        qgate::install::status(root.path(), Some(home.path())),
        Some(InstallScope::Project),
        "quality gate installed"
    );
}

#[test]
fn gate_only_when_no_agent_detected() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    // No providers ⇒ intent capture contributes nothing; the gate still installs.
    let plan = setup_plan(root.path(), InstallScope::Project, &[], Some(home.path())).unwrap();

    let settings = root.path().join(".claude").join("settings.json");
    let matching: Vec<_> = plan.writes.iter().filter(|w| w.path == settings).collect();
    assert_eq!(matching.len(), 1);
    assert!(matching[0].content.contains(qgate::install::MARKER));
    assert!(!matching[0].content.contains("code-basics-intent"));
}

#[test]
fn project_caveat_is_not_duplicated() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let providers = detected_claude(home.path());

    let plan = setup_plan(
        root.path(),
        InstallScope::Project,
        &providers,
        Some(home.path()),
    )
    .unwrap();
    // Both installers warn about the shared repo file; the combined plan dedupes.
    let shared = plan
        .caveats
        .iter()
        .filter(|c| c.contains("shared with"))
        .count();
    assert!(
        shared <= 1,
        "shared-file caveat not repeated (was {shared})"
    );
}
