use super::*;

/// Write a dump-shaped file of a given size.
fn write_dump(dir: &Path, executable: &str, pid: u32, captured_at: u64, bytes: usize) -> PathBuf {
    let path = dir.join(format!("{executable}_{pid}_{captured_at}.dmp"));
    std::fs::write(&path, vec![0u8; bytes]).unwrap();
    path
}

// ---------------------------------------------------------------------------
// Where dumps live
// ---------------------------------------------------------------------------

#[test]
fn dumps_live_inside_the_config_directory() {
    // `.code-basics` is in the workspace scanner's SKIP_DIRS and in the
    // generated `.gitignore`, so a dump can never be scanned as a project nor
    // committed.
    assert_eq!(
        dumps_dir(Path::new("/repo")),
        Path::new("/repo/.code-basics/dumps")
    );
}

// ---------------------------------------------------------------------------
// Arming a run
// ---------------------------------------------------------------------------

#[test]
fn the_environment_asks_the_runtime_for_a_heap_dump() {
    let env = dump_env(Path::new("/repo/.code-basics/dumps"));

    assert_eq!(env.get("DOTNET_DbgEnableMiniDump").unwrap(), "1");
    // 2 is Heap. Mini and Triage omit the object data the inspector exists to
    // show, so this value is load-bearing rather than incidental.
    assert_eq!(env.get("DOTNET_DbgMiniDumpType").unwrap(), "2");
    assert!(env
        .get("DOTNET_DbgMiniDumpName")
        .unwrap()
        .ends_with("%e_%p_%t.dmp"));
    assert_eq!(env.len(), 3, "nothing else should be set on the target");
}

#[test]
fn the_template_names_the_directory_it_was_given() {
    let env = dump_env(Path::new("/repo/.code-basics/dumps"));
    let template = env.get("DOTNET_DbgMiniDumpName").unwrap();

    assert!(
        template.starts_with(&Path::new("/repo/.code-basics/dumps").display().to_string()),
        "{template}"
    );
}

// ---------------------------------------------------------------------------
// The template and its inverse
// ---------------------------------------------------------------------------

#[test]
fn a_real_dump_name_round_trips() {
    // Observed verbatim from a genuine capture: `%e` includes the extension.
    let parsed = parse_dump_name("Crasher.exe_25764_1786044924.dmp").unwrap();

    assert_eq!(parsed.executable, "Crasher.exe");
    assert_eq!(parsed.pid, 25764);
    assert_eq!(parsed.captured_at, 1786044924);
}

#[test]
fn an_executable_with_underscores_and_dots_still_resolves() {
    // Parsed from the right, so the underscores inside the name cannot be
    // mistaken for the field separators.
    let parsed = parse_dump_name("My_App.exe_123_456.dmp").unwrap();

    assert_eq!(parsed.executable, "My_App.exe");
    assert_eq!(parsed.pid, 123);
    assert_eq!(parsed.captured_at, 456);
}

#[test]
fn anything_that_is_not_ours_is_not_claimed() {
    // A wrong value is worse than no value, and a file this module did not
    // write is a file it must not delete or attribute to a run.
    for name in [
        "notes.txt",
        "Crasher.exe_25764_1786044924.dmp.bak",
        "Crasher.exe.dmp",
        "Crasher.exe_25764.dmp",
        "Crasher.exe_abc_1786044924.dmp",
        "Crasher.exe_25764_later.dmp",
        "_25764_1786044924.dmp",
        "Crasher.exe_+1_2.dmp",
        "core.12345",
    ] {
        assert!(parse_dump_name(name).is_none(), "{name} was claimed");
    }
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

#[test]
fn dumps_are_listed_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    // Written oldest-last so the order cannot come from creation order or from
    // mtime — it must come from the timestamp in the name.
    write_dump(dir.path(), "Api.exe", 1, 200, 10);
    write_dump(dir.path(), "Api.exe", 2, 300, 10);
    write_dump(dir.path(), "Api.exe", 3, 100, 10);

    let listed = list(dir.path());

    assert_eq!(
        listed.iter().map(|d| d.captured_at).collect::<Vec<_>>(),
        [300, 200, 100]
    );
    assert_eq!(listed[0].pid, 2);
    assert_eq!(listed[0].bytes, 10);
}

#[test]
fn listing_ignores_files_it_did_not_write() {
    let dir = tempfile::tempdir().unwrap();
    write_dump(dir.path(), "Api.exe", 1, 100, 4);
    std::fs::write(dir.path().join("readme.txt"), "hand placed").unwrap();

    assert_eq!(list(dir.path()).len(), 1);
}

#[test]
fn listing_a_directory_that_does_not_exist_is_empty_not_an_error() {
    // Nothing has crashed yet is the normal state, not a problem to report.
    assert!(list(Path::new("/definitely/not/here")).is_empty());
}

// ---------------------------------------------------------------------------
// Matching a dump to a run
// ---------------------------------------------------------------------------

#[test]
fn a_project_name_matches_an_executable_name() {
    let dir = tempfile::tempdir().unwrap();
    write_dump(dir.path(), "MyApi.exe", 1, 100, 4);

    // The caller holds a project name; the file holds an executable name.
    assert!(newest_for(dir.path(), "MyApi", None).is_some());
    assert!(newest_for(dir.path(), "MyApi.exe", None).is_some());
    assert!(newest_for(dir.path(), "myapi.EXE", None).is_some());
    assert!(newest_for(dir.path(), "MyApi.Tests", None).is_none());
}

#[test]
fn matching_picks_the_newest_and_ignores_other_processes() {
    let dir = tempfile::tempdir().unwrap();
    write_dump(dir.path(), "MyApi.exe", 1, 100, 4);
    write_dump(dir.path(), "MyApi.exe", 2, 300, 4);
    // `dotnet run` arms its build host too, which is exactly why the name is
    // in the filename.
    write_dump(dir.path(), "MSBuild.exe", 3, 400, 4);

    let found = newest_for(dir.path(), "MyApi", None).unwrap();

    assert_eq!(found.pid, 2);
}

#[test]
fn a_dump_older_than_the_run_is_not_attributed_to_it() {
    let dir = tempfile::tempdir().unwrap();
    write_dump(dir.path(), "MyApi.exe", 1, 100, 4);

    assert!(newest_for(dir.path(), "MyApi", Some(200)).is_none());
    // At the start instant counts: the crash cannot precede the process.
    assert!(newest_for(dir.path(), "MyApi", Some(100)).is_some());
}

// ---------------------------------------------------------------------------
// Pruning
// ---------------------------------------------------------------------------

#[test]
fn pruning_keeps_the_newest_by_count() {
    let dir = tempfile::tempdir().unwrap();
    let oldest = write_dump(dir.path(), "Api.exe", 1, 100, 4);
    let middle = write_dump(dir.path(), "Api.exe", 2, 200, 4);
    let newest = write_dump(dir.path(), "Api.exe", 3, 300, 4);

    let removed = prune(dir.path(), 1, None).unwrap();

    // Oldest first, so an interruption leaves the most useful dumps behind.
    assert_eq!(removed, [oldest.clone(), middle.clone()]);
    assert!(!oldest.exists() && !middle.exists());
    assert!(newest.exists());
}

#[test]
fn pruning_applies_the_byte_budget_as_well_as_the_count() {
    let dir = tempfile::tempdir().unwrap();
    let oldest = write_dump(dir.path(), "Api.exe", 1, 100, 100);
    let middle = write_dump(dir.path(), "Api.exe", 2, 200, 100);
    let newest = write_dump(dir.path(), "Api.exe", 3, 300, 100);

    // The count alone would keep all three; the budget is what binds.
    let removed = prune(dir.path(), 10, Some(250)).unwrap();

    assert_eq!(removed, [oldest]);
    assert!(middle.exists() && newest.exists());
}

#[test]
fn the_newest_dump_survives_a_budget_it_alone_exceeds() {
    let dir = tempfile::tempdir().unwrap();
    let older = write_dump(dir.path(), "Api.exe", 1, 100, 500);
    let newest = write_dump(dir.path(), "Api.exe", 2, 200, 500);

    // A limit that leaves nothing to inspect has quietly turned the feature
    // off, which is worse than being slightly over budget.
    let removed = prune(dir.path(), 10, Some(1)).unwrap();

    assert_eq!(removed, [older]);
    assert!(newest.exists());
}

#[test]
fn pruning_never_touches_a_file_it_did_not_write() {
    let dir = tempfile::tempdir().unwrap();
    write_dump(dir.path(), "Api.exe", 1, 100, 4);
    let stray = dir.path().join("keep-me.dmp.bak");
    std::fs::write(&stray, "hand placed").unwrap();

    let removed = prune(dir.path(), 0, Some(0)).unwrap();

    assert_eq!(removed.len(), 1);
    assert!(stray.exists(), "a hand-placed file is not ours to delete");
}

#[test]
fn pruning_a_directory_that_does_not_exist_yet_does_nothing() {
    let dir = tempfile::tempdir().unwrap();

    assert!(prune(&dir.path().join("dumps"), 3, Some(10))
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------------
// Dumps this app asked for but did not name
// ---------------------------------------------------------------------------

#[test]
fn a_collectors_dump_is_pruned_even_though_its_name_cannot_be_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("host");
    std::fs::create_dir_all(&nested).unwrap();

    let old = nested.join("testhost_crashdump.dmp");
    std::fs::write(&old, vec![0u8; 2048]).unwrap();

    let removed = prune_unnamed(dir.path(), 0).unwrap();

    assert_eq!(removed, vec![old.clone()]);
    assert!(!old.exists());
}

#[test]
fn a_collectors_dump_within_the_budget_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("testhost_crashdump.dmp");
    std::fs::write(&kept, vec![0u8; 100]).unwrap();

    assert!(prune_unnamed(dir.path(), 1024).unwrap().is_empty());
    assert!(kept.exists());
}

#[test]
fn sweeping_a_collectors_dumps_never_touches_anything_else() {
    // The results directory holds the reports a run is parsed from. Deleting
    // one would turn a passing run into "the tests ran but nothing appeared".
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("suite.trx");
    let log = dir.path().join("host.log");
    std::fs::write(&report, "<TestRun/>").unwrap();
    std::fs::write(&log, "output").unwrap();

    assert!(prune_unnamed(dir.path(), 0).unwrap().is_empty());
    assert!(report.exists());
    assert!(log.exists());
}

#[test]
fn sweeping_a_directory_that_does_not_exist_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();

    assert!(prune_unnamed(&dir.path().join("nope"), 0)
        .unwrap()
        .is_empty());
}
