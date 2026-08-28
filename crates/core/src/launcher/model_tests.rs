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
            "id",
            "label",
            "lastRunMs",
            "pinned",
            "runCount",
            "shell",
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
