use super::*;

use std::fs;

use crate::tool_gate::ServerId;

/// A temp file path of this test's own, cleared before use so a leftover from a
/// failed run does not leak in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-tool-gate-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("mcp-tools.json")
}

#[test]
fn a_missing_file_loads_as_all_enabled() {
    let path = scratch("missing").with_file_name("does-not-exist.json");
    let gate = load(&path);
    assert_eq!(gate, ToolGateFile::default());
    // The permissive default: nothing said, so every tool is on.
    assert!(gate.is_enabled(ServerId::Sql, "sql.query"));
}

#[test]
fn a_corrupt_file_loads_as_all_enabled_rather_than_erroring() {
    let path = scratch("corrupt");
    fs::write(&path, "not json at all {{{").unwrap();
    let gate = load(&path);
    assert_eq!(gate, ToolGateFile::default());
    assert!(
        gate.is_enabled(ServerId::Browser, "browser_navigate"),
        "a broken preferences file must never turn into 'no tools'"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn load_existing_tells_no_store_apart_from_an_empty_one() {
    let path = scratch("existing");
    assert_eq!(load_existing(&path), None, "no file yet");

    save(&path, &ToolGateFile::default()).unwrap();
    assert_eq!(
        load_existing(&path),
        Some(ToolGateFile::default()),
        "a written-but-empty store is not the same as no store"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_then_load_round_trips() {
    let path = scratch("round-trip");
    let mut gate = ToolGateFile::default();
    gate.set(ServerId::Sql, "sql.query", false);
    gate.set(ServerId::Tasks, "delete_task", false);
    save(&path, &gate).unwrap();
    assert_eq!(load(&path), gate);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_unknown_key_survives_a_round_trip() {
    // A downgrade must not silently discard a choice a later build made about a
    // tool this build has never heard of.
    let path = scratch("unknown-key");
    let mut gate = ToolGateFile::default();
    gate.set(ServerId::Sql, "sql.query", false);
    gate.enabled
        .insert("redis/redis.set_string".to_string(), false);
    save(&path, &gate).unwrap();

    let loaded = load(&path);
    assert_eq!(loaded.enabled.get("redis/redis.set_string"), Some(&false));
    assert!(!loaded.is_enabled(ServerId::Sql, "sql.query"));
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_leaves_no_temp_file_behind() {
    let path = scratch("no-temp");
    save(&path, &ToolGateFile::default()).unwrap();
    let left: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["mcp-tools.json".to_string()]);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_creates_the_parent_directory() {
    let path = scratch("parent")
        .with_file_name("nested")
        .join("mcp-tools.json");
    save(&path, &ToolGateFile::default()).unwrap();
    assert!(path.exists());
    let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}
