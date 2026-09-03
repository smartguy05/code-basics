use super::*;

fn sample() -> Launchable {
    Launchable {
        id: "id-1".into(),
        command: "docker compose up".into(),
        cwd: PathBuf::from("/repo"),
        env: BTreeMap::new(),
        label: None,
        shell: false,
        pinned: true,
        shortcut: true,
        persistent: true,
        headless: true,
        last_run_ms: 42,
        run_count: 3,
    }
}

#[test]
fn launchable_serialises_with_camel_case_keys() {
    let json = serde_json::to_value(sample()).unwrap();
    let object = json.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "command",
            "cwd",
            "env",
            "headless",
            "id",
            "label",
            "lastRunMs",
            "persistent",
            "pinned",
            "runCount",
            "shell",
            "shortcut",
        ]
    );
}

#[test]
fn an_unnamed_entry_crosses_as_an_explicit_null() {
    // Not `skip_serializing_if`: "never renamed" must be visible on the wire, so
    // the frontend never has to guess whether the key was simply omitted.
    let json = serde_json::to_value(sample()).unwrap();
    assert!(json.get("label").unwrap().is_null());
}

#[test]
fn groups_serialise_with_camel_case_keys() {
    let json = serde_json::to_value(LauncherGroups::default()).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["global", "thisCodebase"]);
}

#[test]
fn a_file_with_no_version_reads_as_version_one() {
    let file: LauncherFile = serde_json::from_str(r#"{"entries":[]}"#).unwrap();
    assert_eq!(file.version, 1);
    assert!(file.entries.is_empty());
}

#[test]
fn an_empty_object_reads_as_an_empty_file() {
    let file: LauncherFile = serde_json::from_str("{}").unwrap();
    assert_eq!(file, LauncherFile::default());
}

#[test]
fn an_entry_written_before_the_shortcut_flags_existed_still_loads() {
    // The back-compat guarantee itself, not the `serde(default)` attribute that
    // implements it: a `launchers.json` from a build that predates these fields
    // must load, with every new flag off. Anything else silently turns an old
    // recent into a shortcut, or refuses to open the picker at all.
    let old = r#"{
        "version": 1,
        "entries": [
            {
                "id": "id-1",
                "command": "docker compose up",
                "cwd": "/repo",
                "env": {},
                "label": null,
                "shell": false,
                "pinned": true,
                "lastRunMs": 42,
                "runCount": 3
            }
        ]
    }"#;
    let file: LauncherFile = serde_json::from_str(old).unwrap();
    let entry = &file.entries[0];
    assert!(entry.pinned, "the fields that did exist are unchanged");
    assert!(!entry.shortcut);
    assert!(!entry.persistent);
    assert!(!entry.headless);
}
