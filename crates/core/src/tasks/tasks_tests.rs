use super::*;

use std::fs;
use std::path::PathBuf;

/// A temp file path of this test's own, cleared before use so a leftover from a
/// failed run does not leak in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-tasks-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("tasks.json")
}

fn task(id: &str, title: &str) -> Task {
    Task {
        id: id.into(),
        title: title.into(),
        body: "body".into(),
        status: TaskStatus::Open,
        owner: TaskOwner::Me,
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
    }
}

#[test]
fn a_missing_file_loads_as_the_empty_default() {
    let path = scratch("missing").with_file_name("does-not-exist.json");
    let loaded = load(&path);
    assert_eq!(loaded, TasksFile::default());
    assert_eq!(loaded.version, 1);
    assert!(loaded.tasks.is_empty());
}

#[test]
fn a_corrupt_file_loads_as_the_empty_default_rather_than_erroring() {
    let path = scratch("corrupt");
    fs::write(&path, "this is not json {{{").unwrap();
    assert_eq!(load(&path), TasksFile::default());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_then_load_round_trips() {
    let path = scratch("round-trip");
    let file = TasksFile {
        version: 1,
        tasks: vec![task("t1", "First"), task("t2", "Second")],
    };
    save(&path, &file).unwrap();
    assert_eq!(load(&path), file);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_leaves_no_temp_file_behind() {
    let path = scratch("no-temp");
    let file = TasksFile {
        version: 1,
        tasks: vec![task("t1", "First")],
    };
    save(&path, &file).unwrap();
    let dir = path.parent().unwrap();
    let leftovers: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "tasks.json")
        .collect();
    assert!(leftovers.is_empty(), "unexpected files: {leftovers:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn overwriting_nonempty_tasks_with_empty_backs_up_the_previous_content() {
    let path = scratch("empty-guard");
    let full = TasksFile {
        version: 1,
        tasks: vec![task("t1", "Keep")],
    };
    save(&path, &full).unwrap();

    save(&path, &TasksFile::default()).unwrap();
    assert!(
        load(&path).tasks.is_empty(),
        "the empty save must still apply"
    );

    let bak = path.with_file_name("tasks.json.bak");
    assert!(bak.exists(), "expected a .bak of the previous content");
    assert_eq!(load(&bak), full);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn overwriting_with_more_tasks_does_not_create_a_backup() {
    let path = scratch("no-bak");
    save(
        &path,
        &TasksFile {
            version: 1,
            tasks: vec![task("t1", "One")],
        },
    )
    .unwrap();
    save(
        &path,
        &TasksFile {
            version: 1,
            tasks: vec![task("t1", "One"), task("t2", "Two")],
        },
    )
    .unwrap();
    assert!(!path.with_file_name("tasks.json.bak").exists());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_creates_the_parent_directory() {
    let dir = std::env::temp_dir().join("cb-tasks-mkdir/nested/deeper");
    let _ = fs::remove_dir_all(std::env::temp_dir().join("cb-tasks-mkdir"));
    let path = dir.join("tasks.json");
    assert!(!dir.exists());
    save(&path, &TasksFile::default()).unwrap();
    assert!(path.exists());
    let _ = fs::remove_dir_all(std::env::temp_dir().join("cb-tasks-mkdir"));
}

#[test]
fn tasks_path_lives_under_the_workspace_config_dir() {
    // With no override, the store sits under `<root>/.code-basics/tasks.json`.
    let prev = std::env::var_os("CB_TASKS_PATH");
    std::env::remove_var("CB_TASKS_PATH");
    let root = PathBuf::from("/some/workspace");
    let path = tasks_path(&root);
    assert_eq!(path, crate::config::config_dir(&root).join("tasks.json"));
    if let Some(prev) = prev {
        std::env::set_var("CB_TASKS_PATH", prev);
    }
}

#[test]
fn create_appends_an_open_task_with_matching_timestamps() {
    let mut file = TasksFile::default();
    let created = file.create("t1", "Title", "Body", TaskOwner::Me, 42);
    assert_eq!(created.id, "t1");
    assert_eq!(created.status, TaskStatus::Open);
    assert_eq!(created.owner, TaskOwner::Me);
    assert_eq!(created.created_at_ms, 42);
    assert_eq!(created.updated_at_ms, 42);
    assert_eq!(file.tasks.len(), 1);
}

#[test]
fn update_overwrites_title_and_body_and_stamps_updated() {
    let mut file = TasksFile::default();
    file.create("t1", "Old", "old body", TaskOwner::Me, 1);
    assert!(file.update("t1", "New", "new body", 99));
    let t = &file.tasks[0];
    assert_eq!(t.title, "New");
    assert_eq!(t.body, "new body");
    assert_eq!(t.created_at_ms, 1, "creation time is preserved");
    assert_eq!(t.updated_at_ms, 99);
    assert!(
        !file.update("missing", "x", "y", 100),
        "unknown id is a no-op"
    );
}

#[test]
fn set_status_and_set_owner_stamp_updated() {
    let mut file = TasksFile::default();
    file.create("t1", "Title", "Body", TaskOwner::Me, 1);
    assert!(file.set_status("t1", TaskStatus::Done, 50));
    assert_eq!(file.tasks[0].status, TaskStatus::Done);
    assert_eq!(file.tasks[0].updated_at_ms, 50);
    assert!(file.set_owner("t1", TaskOwner::Ai, 60));
    assert_eq!(file.tasks[0].owner, TaskOwner::Ai);
    assert_eq!(file.tasks[0].updated_at_ms, 60);
    assert!(!file.set_status("missing", TaskStatus::Done, 70));
    assert!(!file.set_owner("missing", TaskOwner::Ai, 70));
}

#[test]
fn remove_drops_only_the_named_task() {
    let mut file = TasksFile::default();
    file.create("t1", "One", "a", TaskOwner::Me, 1);
    file.create("t2", "Two", "b", TaskOwner::Me, 1);
    assert!(file.remove("t1"));
    assert_eq!(file.tasks.len(), 1);
    assert_eq!(file.tasks[0].id, "t2");
    assert!(!file.remove("t1"), "removing again is a no-op");
}

#[test]
fn a_tasks_key_defaults_when_absent() {
    let path = scratch("partial");
    fs::write(&path, r#"{"version":1}"#).unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.version, 1);
    assert!(loaded.tasks.is_empty());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

/// The IPC contract: the exact camelCase keys the TypeScript side reads. Its
/// counterpart lives in `src/ipc/types.ts`; changing one without the other
/// breaks the wire.
#[test]
fn serialisation_shape_pins_the_wire_keys() {
    let mut file = TasksFile::default();
    file.tasks.push(Task {
        id: "t1".into(),
        title: "Title".into(),
        body: "Body".into(),
        status: TaskStatus::Open,
        owner: TaskOwner::Ai,
        created_at_ms: 1,
        updated_at_ms: 2,
    });
    let json = serde_json::to_value(&file).unwrap();

    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("version"));
    assert!(obj.contains_key("tasks"));

    let t = json["tasks"][0].as_object().unwrap();
    let mut keys: Vec<&str> = t.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "body",
            "createdAtMs",
            "id",
            "owner",
            "status",
            "title",
            "updatedAtMs"
        ]
    );

    // The enums serialise as camelCase strings.
    assert_eq!(json["tasks"][0]["status"], serde_json::json!("open"));
    assert_eq!(json["tasks"][0]["owner"], serde_json::json!("ai"));
}
