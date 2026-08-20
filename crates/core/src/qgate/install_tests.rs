//! Installer tests for the quality-gate hook. The behaviour that matters most
//! is coexistence: the gate's `Stop` entry must sit alongside the intent
//! recorder's without either removing the other.

use super::*;
use crate::intents::providers::{apply_writes, hooks_json};
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("valid JSON")
}

#[test]
fn project_plan_writes_repo_settings_with_subcommand_and_pin() {
    let dir = tempdir().unwrap();
    let plan = install_plan(dir.path(), InstallScope::Project, None).unwrap();

    assert_eq!(plan.writes.len(), 1);
    let write = &plan.writes[0];
    assert_eq!(write.path, dir.path().join(".claude").join("settings.json"));
    assert!(!write.merges_existing, "no file existed yet");
    assert!(write.content.contains(MARKER));
    assert!(write.content.contains(super::super::SUBCOMMAND));
    assert!(
        write.content.contains("--workspace"),
        "project scope pins the workspace"
    );
    // Only the Stop event is managed.
    let value = parse(&write.content);
    assert!(value["hooks"]["Stop"].is_array());
    assert!(value["hooks"]["PostToolUse"].is_null());
}

#[test]
fn user_plan_uses_home_and_does_not_pin() {
    let home = tempdir().unwrap();
    let root = tempdir().unwrap();
    let plan = install_plan(root.path(), InstallScope::User, Some(home.path())).unwrap();

    let write = &plan.writes[0];
    assert_eq!(write.path, home.path().join("settings.json"));
    assert!(write.content.contains(MARKER));
    assert!(
        !write.content.contains("--workspace"),
        "user scope must not pin a single workspace"
    );
}

#[test]
fn reinstall_is_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".claude").join("settings.json");

    let first = install_plan(dir.path(), InstallScope::Project, None).unwrap();
    apply_writes(&first.writes).unwrap();

    // Re-plan against the now-written file and apply again.
    let second = install_plan(dir.path(), InstallScope::Project, None).unwrap();
    assert!(second.writes[0].merges_existing);
    apply_writes(&second.writes).unwrap();

    let value = parse(&fs::read_to_string(&path).unwrap());
    let stop = value["hooks"]["Stop"].as_array().unwrap();
    let marked = stop
        .iter()
        .filter(|e| settings_merge::contains_marker(e, MARKER))
        .count();
    assert_eq!(marked, 1, "re-install replaces rather than duplicates");
}

#[test]
fn coexists_with_the_intent_recorder_stop_entry() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".claude").join("settings.json");

    // First install the intent recorder (which owns Stop with its own marker).
    let (intent_text, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &intent_text).unwrap();
    assert!(hooks_json::is_installed(&path), "intent hook installed");

    // Now install the quality gate into the same file.
    let plan = install_plan(dir.path(), InstallScope::Project, None).unwrap();
    apply_writes(&plan.writes).unwrap();

    let value = parse(&fs::read_to_string(&path).unwrap());
    let stop = value["hooks"]["Stop"].as_array().unwrap();
    // Both markers present in the Stop array.
    assert!(stop
        .iter()
        .any(|e| settings_merge::contains_marker(e, MARKER)));
    assert!(stop
        .iter()
        .any(|e| settings_merge::contains_marker(e, hooks_json::MARKER)));
    // The recorder is still considered installed (all its events survive).
    assert!(
        hooks_json::is_installed(&path),
        "installing the gate must not remove the recorder"
    );
}

#[test]
fn status_reflects_project_install_and_backs_up_on_merge() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".claude").join("settings.json");

    assert_eq!(status(dir.path(), None), None);

    let plan = install_plan(dir.path(), InstallScope::Project, None).unwrap();
    apply_writes(&plan.writes).unwrap();
    assert_eq!(status(dir.path(), None), Some(InstallScope::Project));

    // A second apply (merges_existing) leaves a .bak of the prior file.
    let second = install_plan(dir.path(), InstallScope::Project, None).unwrap();
    apply_writes(&second.writes).unwrap();
    assert!(
        path.with_extension("json.bak").exists(),
        "merge backs up first"
    );
}

#[test]
fn preserves_unrelated_settings() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".claude").join("settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{"model":"claude-opus","hooks":{"SessionStart":[{"hooks":[]}]}}"#,
    )
    .unwrap();

    let plan = install_plan(dir.path(), InstallScope::Project, None).unwrap();
    apply_writes(&plan.writes).unwrap();

    let value = parse(&fs::read_to_string(&path).unwrap());
    assert_eq!(value["model"], "claude-opus", "unrelated keys untouched");
    assert!(
        value["hooks"]["SessionStart"].is_array(),
        "other events untouched"
    );
    assert!(value["hooks"]["Stop"].is_array(), "gate added");
}
