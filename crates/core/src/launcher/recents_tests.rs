use super::*;
use std::path::{Path, PathBuf};

fn file_with(entries: Vec<Launchable>) -> LauncherFile {
    LauncherFile {
        version: 1,
        entries,
    }
}

fn entry(command: &str, cwd: &str, last_run_ms: u64, pinned: bool) -> Launchable {
    Launchable {
        id: format!("id-{command}-{cwd}"),
        command: command.into(),
        cwd: PathBuf::from(cwd),
        env: std::collections::BTreeMap::new(),
        label: None,
        shell: false,
        pinned,
        last_run_ms,
        run_count: 1,
    }
}

#[test]
fn recording_a_new_command_adds_one_entry() {
    let mut file = LauncherFile::default();
    let id = record_run(
        &mut file,
        "docker compose up",
        Path::new("/repo"),
        false,
        1_000,
    );
    assert_eq!(file.entries.len(), 1);
    let recorded = &file.entries[0];
    assert_eq!(recorded.id, id);
    assert_eq!(recorded.command, "docker compose up");
    assert_eq!(recorded.cwd, PathBuf::from("/repo"));
    assert_eq!(recorded.last_run_ms, 1_000);
    assert_eq!(recorded.run_count, 1);
    assert!(!recorded.pinned);
}

#[test]
fn rerunning_the_same_command_in_the_same_cwd_updates_rather_than_duplicates() {
    let mut file = LauncherFile::default();
    let first = record_run(&mut file, "npm run dev", Path::new("/repo"), false, 1_000);
    let second = record_run(&mut file, "npm run dev", Path::new("/repo"), false, 2_000);
    assert_eq!(first, second, "the same command keeps its id");
    assert_eq!(file.entries.len(), 1);
    assert_eq!(file.entries[0].last_run_ms, 2_000);
    assert_eq!(file.entries[0].run_count, 2);
}

#[test]
fn the_same_command_in_a_different_cwd_is_a_different_entry() {
    let mut file = LauncherFile::default();
    record_run(&mut file, "npm run dev", Path::new("/a"), false, 1);
    record_run(&mut file, "npm run dev", Path::new("/b"), false, 2);
    assert_eq!(file.entries.len(), 2);
}

#[test]
fn rerunning_preserves_the_pin_and_the_rename() {
    let mut file = file_with(vec![Launchable {
        label: Some("Redis".into()),
        pinned: true,
        ..entry("redis-server", "/repo", 1, true)
    }]);
    record_run(&mut file, "redis-server", Path::new("/repo"), false, 9_000);
    assert_eq!(file.entries.len(), 1);
    assert_eq!(file.entries[0].label.as_deref(), Some("Redis"));
    assert!(file.entries[0].pinned);
    assert_eq!(file.entries[0].last_run_ms, 9_000);
}

#[test]
fn rerunning_updates_the_shell_flag_to_what_actually_ran() {
    let mut file = LauncherFile::default();
    record_run(&mut file, "a | b", Path::new("/repo"), true, 1);
    assert!(file.entries[0].shell);
    record_run(&mut file, "a | b", Path::new("/repo"), false, 2);
    assert!(!file.entries[0].shell);
}

#[test]
fn unpinned_recents_are_capped_and_the_oldest_goes_first() {
    let mut file = LauncherFile::default();
    for i in 0..MAX_UNPINNED as u64 + 5 {
        record_run(
            &mut file,
            &format!("cmd{i}"),
            Path::new("/repo"),
            false,
            i + 1,
        );
    }
    assert_eq!(file.entries.len(), MAX_UNPINNED);
    let commands: Vec<&str> = file.entries.iter().map(|e| e.command.as_str()).collect();
    assert!(!commands.contains(&"cmd0"), "oldest should be evicted");
    assert!(commands.contains(&"cmd34"), "newest should survive");
}

#[test]
fn pinned_entries_are_never_evicted_and_do_not_count_against_the_cap() {
    let mut file = file_with(vec![entry("pinned-old", "/repo", 1, true)]);
    for i in 0..MAX_UNPINNED as u64 + 5 {
        record_run(
            &mut file,
            &format!("cmd{i}"),
            Path::new("/repo"),
            false,
            i + 10,
        );
    }
    assert_eq!(file.entries.len(), MAX_UNPINNED + 1);
    assert!(file.entries.iter().any(|e| e.command == "pinned-old"));
}

#[test]
fn grouping_puts_the_active_codebase_first_and_the_rest_in_global() {
    let entries = vec![
        entry("a", "/repo/sub", 1, false),
        entry("b", "/elsewhere", 2, false),
        entry("c", "/repo", 3, false),
    ];
    let groups = group(&entries, Some(Path::new("/repo")));
    let here: Vec<&str> = groups
        .this_codebase
        .iter()
        .map(|e| e.command.as_str())
        .collect();
    assert_eq!(here, vec!["c", "a"], "most recent first");
    let elsewhere: Vec<&str> = groups.global.iter().map(|e| e.command.as_str()).collect();
    assert_eq!(elsewhere, vec!["b"]);
}

#[test]
fn pinned_entries_sort_ahead_of_more_recent_unpinned_ones() {
    let entries = vec![
        entry("recent", "/repo", 100, false),
        entry("pinned", "/repo", 1, true),
    ];
    let groups = group(&entries, Some(Path::new("/repo")));
    let here: Vec<&str> = groups
        .this_codebase
        .iter()
        .map(|e| e.command.as_str())
        .collect();
    assert_eq!(here, vec!["pinned", "recent"]);
}

#[test]
fn with_no_workspace_open_everything_is_global() {
    let entries = vec![entry("a", "/repo", 1, false)];
    let groups = group(&entries, None);
    assert!(groups.this_codebase.is_empty());
    assert_eq!(groups.global.len(), 1);
}

#[test]
fn a_sibling_directory_with_a_shared_prefix_is_not_inside_the_root() {
    assert!(!within_root(Path::new("/repo"), Path::new("/repo2")));
    assert!(within_root(Path::new("/repo"), Path::new("/repo")));
    assert!(within_root(Path::new("/repo"), Path::new("/repo/sub")));
}

#[test]
fn containment_tolerates_separator_and_trailing_slash_differences() {
    assert!(within_root(
        Path::new(r"C:\repo\"),
        Path::new("C:/repo/src")
    ));
}

#[cfg(windows)]
#[test]
fn containment_is_case_insensitive_on_windows() {
    assert!(within_root(
        Path::new(r"C:\Repo"),
        Path::new(r"c:\repo\src")
    ));
}

#[test]
fn an_empty_root_contains_nothing() {
    assert!(!within_root(Path::new(""), Path::new("/repo")));
}

#[test]
fn renaming_sets_and_clears_the_label() {
    let mut file = file_with(vec![entry("a", "/repo", 1, false)]);
    let id = file.entries[0].id.clone();
    assert!(rename(&mut file, &id, Some("  Redis  ")));
    assert_eq!(file.entries[0].label.as_deref(), Some("Redis"));
    assert!(rename(&mut file, &id, Some("   ")));
    assert_eq!(
        file.entries[0].label, None,
        "a blank name clears the rename"
    );
    assert!(!rename(&mut file, "nope", Some("x")));
}

#[test]
fn pinning_and_removing_address_entries_by_id() {
    let mut file = file_with(vec![entry("a", "/repo", 1, false)]);
    let id = file.entries[0].id.clone();
    assert!(set_pinned(&mut file, &id, true));
    assert!(file.entries[0].pinned);
    assert!(!set_pinned(&mut file, "nope", true));
    assert!(remove(&mut file, &id));
    assert!(file.entries.is_empty());
    assert!(
        !remove(&mut file, &id),
        "removing twice reports nothing done"
    );
}
