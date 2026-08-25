use super::*;

use std::fs;

/// A temp file path of this test's own, cleared before use so a leftover from a
/// failed run does not leak in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-notes-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("notes.json")
}

fn note(id: &str, title: &str, body: &str) -> Note {
    Note {
        id: id.into(),
        title: title.into(),
        body: body.into(),
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
    }
}

#[test]
fn a_missing_file_loads_as_the_empty_default() {
    let path = scratch("missing").with_file_name("does-not-exist.json");
    let loaded = load(&path);
    assert_eq!(loaded, NotesFile::default());
    assert_eq!(loaded.version, 1);
    assert!(loaded.notes.is_empty());
}

#[test]
fn a_corrupt_file_loads_as_the_empty_default_rather_than_erroring() {
    let path = scratch("corrupt");
    fs::write(&path, "this is not json {{{").unwrap();
    assert_eq!(load(&path), NotesFile::default());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_then_load_round_trips() {
    let path = scratch("round-trip");
    let file = NotesFile {
        version: 1,
        notes: vec![
            note("n1", "Scratch", "hello"),
            note("n2", "Deploy", "steps"),
        ],
    };
    save(&path, &file).unwrap();
    assert_eq!(load(&path), file);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_creates_the_parent_directory() {
    let dir = std::env::temp_dir().join("cb-notes-mkdir/nested/deeper");
    let _ = fs::remove_dir_all(std::env::temp_dir().join("cb-notes-mkdir"));
    let path = dir.join("notes.json");
    assert!(!dir.exists());
    save(&path, &NotesFile::default()).unwrap();
    assert!(path.exists());
    let _ = fs::remove_dir_all(std::env::temp_dir().join("cb-notes-mkdir"));
}

/// The IPC contract: the exact camelCase keys the TypeScript side reads. Its
/// counterpart lives in `src/ipc/types.ts`; changing one without the other
/// breaks the wire.
#[test]
fn serialisation_shape_pins_the_wire_keys() {
    let file = NotesFile {
        version: 1,
        notes: vec![note("n1", "Scratch", "hello")],
    };
    let json = serde_json::to_value(&file).unwrap();

    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("version"));
    assert!(obj.contains_key("notes"));

    let n = &json["notes"][0];
    let n = n.as_object().unwrap();
    let mut keys: Vec<&str> = n.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["body", "createdAtMs", "id", "title", "updatedAtMs"]);
}

#[test]
fn an_empty_notes_key_defaults_when_absent() {
    // A hand-written file with only a version still loads.
    let path = scratch("partial");
    fs::write(&path, r#"{"version":1}"#).unwrap();
    let loaded = load(&path);
    assert_eq!(loaded.version, 1);
    assert!(loaded.notes.is_empty());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}
