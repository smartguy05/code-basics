use super::*;
use std::path::PathBuf;

use crate::launcher::Launchable;

use tempfile::tempdir;

fn one(command: &str) -> Launchable {
    Launchable {
        id: "id-1".into(),
        command: command.into(),
        cwd: PathBuf::from("/repo"),
        env: std::collections::BTreeMap::new(),
        label: None,
        shell: false,
        pinned: false,
        last_run_ms: 1,
        run_count: 1,
    }
}

#[test]
fn a_missing_file_reads_as_an_empty_store() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nope").join(LAUNCHERS_FILE);
    assert_eq!(load(&path), LauncherFile::default());
}

#[test]
fn a_corrupt_file_reads_as_an_empty_store_rather_than_erroring() {
    // Same rule as `notes`: a bad file must not stop the picker opening.
    let dir = tempdir().unwrap();
    let path = dir.path().join(LAUNCHERS_FILE);
    std::fs::write(&path, "{ this is not json").unwrap();
    assert_eq!(load(&path), LauncherFile::default());
}

#[test]
fn a_saved_store_round_trips() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested").join(LAUNCHERS_FILE);
    let file = LauncherFile {
        version: 1,
        entries: vec![one("docker compose up")],
    };
    save(&path, &file).unwrap();
    assert_eq!(load(&path), file);
}

#[test]
fn emptying_a_non_empty_store_leaves_a_backup() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(LAUNCHERS_FILE);
    save(
        &path,
        &LauncherFile {
            version: 1,
            entries: vec![one("keep me")],
        },
    )
    .unwrap();
    save(&path, &LauncherFile::default()).unwrap();

    let backup = path.with_file_name(format!("{LAUNCHERS_FILE}.bak"));
    let text = std::fs::read_to_string(backup).unwrap();
    assert!(text.contains("keep me"));
    assert!(load(&path).entries.is_empty());
}

#[test]
fn no_temp_file_is_left_behind() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(LAUNCHERS_FILE);
    save(&path, &LauncherFile::default()).unwrap();
    assert!(!path
        .with_file_name(format!("{LAUNCHERS_FILE}.tmp"))
        .exists());
}

#[test]
fn the_default_path_sits_beside_the_other_user_global_stores() {
    // `launchers_path` reads process-wide environment, so this asserts only what
    // holds however the suite is invoked: either the override is honoured, or the
    // default lands in the shared `code-basics` config directory.
    let path = launchers_path();
    match std::env::var_os("CB_LAUNCHERS_PATH") {
        Some(override_path) => assert_eq!(path, PathBuf::from(override_path)),
        None => assert!(path.ends_with(PathBuf::from("code-basics").join(LAUNCHERS_FILE))),
    }
}
