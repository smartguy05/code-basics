//! Tests for the per-workspace run-once record. Pure filesystem work over a
//! tempdir, with the clock passed in for reproducibility.

use super::*;
use std::time::Duration;

fn at(ms: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
}

#[test]
fn a_missing_file_reads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_runs(dir.path()).is_empty());
}

#[test]
fn a_corrupt_file_reads_as_empty_rather_than_erroring() {
    let dir = tempfile::tempdir().unwrap();
    let path = runs_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not json").unwrap();
    assert!(read_runs(dir.path()).is_empty());
}

#[test]
fn a_recorded_run_reads_back_with_its_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    record_run(dir.path(), "setup", at(1_700_000_000_000)).unwrap();

    let runs = read_runs(dir.path());
    assert_eq!(runs.len(), 1);
    assert_eq!(runs["setup"].last_run_at_ms, 1_700_000_000_000);
}

#[test]
fn recording_creates_the_code_basics_directory() {
    let dir = tempfile::tempdir().unwrap();
    // No .code-basics yet.
    assert!(!dir.path().join(".code-basics").exists());
    record_run(dir.path(), "setup", at(1)).unwrap();
    assert!(runs_path(dir.path()).exists());
}

#[test]
fn the_record_is_gitignored_so_it_stays_local() {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::config_dir(dir.path());
    std::fs::create_dir_all(&config).unwrap();
    crate::config::ensure_gitignore(&config).unwrap();

    let ignore = std::fs::read_to_string(config.join(".gitignore")).unwrap();
    assert!(
        ignore.lines().any(|line| line.trim() == RUNS_FILE),
        "agent-runs.json must be gitignored: {ignore}"
    );
}

#[test]
fn re_running_overwrites_the_previous_stamp_and_keeps_others() {
    let dir = tempfile::tempdir().unwrap();
    record_run(dir.path(), "setup", at(100)).unwrap();
    record_run(dir.path(), "graph", at(200)).unwrap();
    record_run(dir.path(), "setup", at(300)).unwrap();

    let runs = read_runs(dir.path());
    assert_eq!(runs["setup"].last_run_at_ms, 300, "overwritten");
    assert_eq!(runs["graph"].last_run_at_ms, 200, "untouched");
}
