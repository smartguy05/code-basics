use super::*;
use crate::inspect::model::{Caps, InspectTarget, RootSpec};

fn failure(code: &str) -> SidecarFailure {
    SidecarFailure {
        code: failure_code(code),
        message: "something went wrong".into(),
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

#[test]
fn a_session_keeps_its_request_and_result_together() {
    let root = Path::new("/repo");

    assert_eq!(sessions_dir(root), Path::new("/repo/.code-basics/inspect"));
    assert_eq!(
        request_path(root, "s-1"),
        Path::new("/repo/.code-basics/inspect/s-1/request.json")
    );
    assert_eq!(
        result_path(root, "s-1"),
        Path::new("/repo/.code-basics/inspect/s-1/result.json")
    );

    // One directory per capture is what makes pruning a listing rather than a
    // bookkeeping file that could disagree with the disk.
    assert_eq!(
        request_path(root, "s-1").parent(),
        result_path(root, "s-1").parent()
    );
}

#[test]
fn captures_live_inside_the_config_directory() {
    // `.code-basics` is already in the workspace scanner's SKIP_DIRS, so a
    // capture can never be mistaken for a project.
    assert!(sessions_dir(Path::new("/repo")).starts_with("/repo/.code-basics"));
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

#[test]
fn the_command_line_names_both_files() {
    let args = command_args(Path::new("/a/request.json"), Path::new("/a/result.json"));

    assert_eq!(
        args,
        ["--request", "/a/request.json", "--result", "/a/result.json"]
    );
}

#[test]
fn listing_processes_asks_for_a_result_and_nothing_else() {
    // There is no request file because the question has no parameters, and the
    // flag says what mode it is rather than being implied by the absence of an
    // argument — the same reason the capture arguments are named.
    let args = list_command_args(Path::new("/a/processes.json"));

    assert_eq!(args, ["--list-processes", "--result", "/a/processes.json"]);
}

#[test]
fn two_listings_never_share_a_file() {
    // The picker polls while a capture revalidates, so two enumerations overlap
    // routinely. Reading a file another run is halfway through writing would
    // produce a shorter list that looks complete.
    assert_ne!(process_list_path("s-1"), process_list_path("s-2"));
    // Not under `.code-basics/`: it is a poll's answer, not state, and it must
    // not compete with real captures for the retention budget.
    assert!(!process_list_path("s-1")
        .to_string_lossy()
        .contains(".code-basics"));
}

// ---------------------------------------------------------------------------
// Reading a process listing
// ---------------------------------------------------------------------------

#[test]
fn a_process_listing_is_read_with_everything_it_knew() {
    let list = parse_process_list(
        r#"{
            "schemaVersion": 1,
            "processes": [
              { "pid": 9960, "name": "Crasher", "path": "C:\\src\\Crasher.exe",
                "parentPid": 8352, "startedAt": "2026-08-06T13:35:02.1230000Z" }
            ],
            "warnings": ["one process could not be described"]
        }"#,
    )
    .unwrap();

    assert_eq!(list.processes.len(), 1);
    let p = &list.processes[0];
    assert_eq!(p.pid, 9960);
    assert_eq!(p.name, "Crasher");
    assert_eq!(p.path.as_deref(), Some(Path::new(r"C:\src\Crasher.exe")));
    assert_eq!(p.parent_pid, Some(8352));
    assert_eq!(
        p.started_at.as_deref(),
        Some("2026-08-06T13:35:02.1230000Z")
    );
    assert_eq!(list.warnings, ["one process could not be described"]);
}

#[test]
fn a_process_the_enumerator_could_only_half_describe_keeps_what_it_had() {
    // Everything but the pid and the name is read through APIs that fail on a
    // process owned by another user. Absent is the honest answer; a zero parent
    // would be attributed to whatever is running as pid zero.
    let list = parse_process_list(
        r#"{ "schemaVersion": 1, "processes": [{ "pid": 4, "name": "Locked" }] }"#,
    )
    .unwrap();

    assert_eq!(list.processes.len(), 1);
    assert_eq!(list.processes[0].parent_pid, None);
    assert_eq!(list.processes[0].path, None);
    assert!(list.warnings.is_empty());
}

#[test]
fn an_unusable_entry_costs_that_entry_and_not_the_list() {
    // The same rule the node reader follows: a sidecar bug may cost the user a
    // value, never the ninety-nine it got right. Each loss is said out loud.
    let list = parse_process_list(
        r#"{
            "schemaVersion": 1,
            "processes": [
              { "name": "no pid" },
              { "pid": 0, "name": "idle" },
              { "pid": 7, "name": "   " },
              { "pid": 8, "name": "Real" }
            ]
        }"#,
    )
    .unwrap();

    assert_eq!(
        list.processes.iter().map(|p| p.pid).collect::<Vec<_>>(),
        vec![8]
    );
    assert_eq!(list.warnings.len(), 1, "got {:?}", list.warnings);
    assert!(list.warnings[0].contains('3'), "got {:?}", list.warnings);
}

#[test]
fn a_process_reported_as_its_own_parent_loses_the_parent_not_itself() {
    // A self-parent cannot be true, so it is discarded at the door and no walk
    // downstream has to defend against it. The process is still a real attach
    // target and is still listed.
    let list = parse_process_list(
        r#"{ "schemaVersion": 1, "processes": [{ "pid": 30, "name": "Odd", "parentPid": 30 }] }"#,
    )
    .unwrap();

    assert_eq!(list.processes[0].pid, 30);
    assert_eq!(list.processes[0].parent_pid, None);
}

#[test]
fn a_process_listing_from_a_stale_sidecar_is_refused_by_version() {
    let error = parse_process_list(r#"{ "schemaVersion": 99, "processes": [] }"#)
        .expect_err("a version this build does not read must not be guessed at");
    let message = format!("{error:#}");
    assert!(message.contains("99"), "got {message}");
    assert!(message.contains("rebuilding"), "got {message}");
}

#[test]
fn the_command_line_is_readable_when_echoed_to_the_console() {
    // It is shown to the user like every other command this app runs, and two
    // bare paths would not say which was which.
    let args = command_args(Path::new("/a/request.json"), Path::new("/a/result.json"));
    assert!(args.iter().any(|a| a == "--request"));
    assert!(args.iter().any(|a| a == "--result"));
}

// ---------------------------------------------------------------------------
// Writing the request
// ---------------------------------------------------------------------------

fn a_request() -> InspectRequest {
    InspectRequest::new(
        InspectTarget::Live { pid: 4242 },
        RootSpec::Exceptions,
        Caps::default(),
    )
}

#[test]
fn writing_a_request_creates_the_session_and_returns_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let path = write_request(root, "s-1", &a_request()).unwrap();

    assert_eq!(path, request_path(root, "s-1"));
    assert!(path.exists());

    let written: InspectRequest =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written, a_request());
}

/// Captures hold a verbatim copy of whatever the process was holding. They
/// must not reach a shared history, and the ignore file has to exist by the
/// time the first one is written — not whenever a config happens to be saved.
#[test]
fn writing_a_request_ignores_the_capture_directory() {
    let dir = tempfile::tempdir().unwrap();
    write_request(dir.path(), "s-1", &a_request()).unwrap();

    let ignore = std::fs::read_to_string(crate::config::config_dir(dir.path()).join(".gitignore"))
        .expect("a .gitignore should exist beside the capture");

    assert!(ignore.contains("inspect/"), "got {ignore}");
    assert!(ignore.contains("dumps/"), "got {ignore}");
}

// ---------------------------------------------------------------------------
// Which build to run
// ---------------------------------------------------------------------------

#[test]
fn each_architecture_has_its_own_binary() {
    assert_eq!(sidecar_file_name(Bitness::X64), "cb-inspector-win-x64.exe");
    assert_eq!(sidecar_file_name(Bitness::X86), "cb-inspector-win-x86.exe");
    assert_ne!(
        sidecar_file_name(Bitness::X64),
        sidecar_file_name(Bitness::X86),
        "the two builds must not overwrite each other in the bundle"
    );
}

/// x64 is tried first because almost everything is. A second attempt is only
/// earned by a *reported* bitness mismatch: retrying on a generic failure
/// would double the wait for every real problem and bury the first message
/// behind an identical second one.
#[test]
fn only_a_bitness_mismatch_earns_a_second_attempt() {
    assert_eq!(
        next_attempt(Bitness::X64, &failure("bitnessMismatch")),
        Some(Bitness::X86)
    );

    for code in ["accessDenied", "targetGone", "notManaged", "somethingElse"] {
        assert_eq!(
            next_attempt(Bitness::X64, &failure(code)),
            None,
            "`{code}` should not be retried"
        );
    }
}

#[test]
fn the_fallback_build_never_retries() {
    // There is nothing narrower to try, and a loop here would be unbounded.
    for code in ["bitnessMismatch", "accessDenied", "targetGone"] {
        assert_eq!(next_attempt(Bitness::X86, &failure(code)), None);
    }
}

#[test]
fn an_unrecognised_failure_code_is_not_treated_as_a_mismatch() {
    // A newer sidecar naming a cause this build predates must not be guessed
    // into a retry.
    assert_eq!(failure_code("somethingNewer"), FailureCode::Other);
    assert_eq!(next_attempt(Bitness::X64, &failure("somethingNewer")), None);
}

#[test]
fn a_failure_keeps_the_sidecars_own_wording() {
    let raw: RawResult =
        serde_json::from_str(include_str!("../../fixtures/inspect/attach-failed.json")).unwrap();

    let failure = failure_of(&raw).expect("the fixture carries a failure");
    assert!(failure.message.contains("32-bit"));
    // The fixture predates failure codes, so it must degrade rather than break.
    assert_eq!(failure.code, FailureCode::Other);
}

#[test]
fn a_successful_result_carries_no_failure() {
    let raw: RawResult =
        serde_json::from_str(include_str!("../../fixtures/inspect/crash-exception.json")).unwrap();

    assert!(failure_of(&raw).is_none());
}

// ---------------------------------------------------------------------------
// Resolving the executable
// ---------------------------------------------------------------------------

#[test]
fn a_missing_sidecar_resolves_to_nothing_rather_than_a_bad_path() {
    let dir = tempfile::tempdir().unwrap();

    // Returning a path that does not exist would turn a clear "the inspector
    // is not installed" into a confusing spawn failure.
    assert_eq!(resolve(Some(dir.path()), Bitness::X64), None);
    assert_eq!(resolve(None, Bitness::X64), None);
}

#[test]
fn a_bundled_sidecar_is_found_by_its_architecture() {
    let dir = tempfile::tempdir().unwrap();
    let x64 = dir.path().join(sidecar_file_name(Bitness::X64));
    std::fs::write(&x64, "").unwrap();

    assert_eq!(resolve(Some(dir.path()), Bitness::X64), Some(x64));
    // The x86 build is genuinely absent, and saying so beats running the wrong
    // one against a 32-bit target.
    assert_eq!(resolve(Some(dir.path()), Bitness::X86), None);
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

fn session(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join(RESULT_FILE), "{}").unwrap();
    path
}

#[test]
fn pruning_keeps_the_newest_sessions_and_removes_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let mut created = Vec::new();
    for i in 0..5 {
        created.push(session(root, &format!("s-{i}")));
        // Coarse filesystem timestamps need a gap to order reliably.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let removed = retain_newest(root, 2).unwrap();

    assert_eq!(removed.len(), 3);
    assert!(!created[0].exists(), "the oldest should be gone");
    assert!(!created[2].exists());
    assert!(created[3].exists(), "the newest should be kept");
    assert!(created[4].exists());
}

#[test]
fn pruning_below_the_limit_removes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    session(dir.path(), "s-1");

    assert!(retain_newest(dir.path(), DEFAULT_KEEP_SESSIONS)
        .unwrap()
        .is_empty());
    assert!(dir.path().join("s-1").exists());
}

#[test]
fn pruning_a_directory_that_does_not_exist_yet_is_harmless() {
    // The first capture in a workspace prunes before anything has been written.
    let dir = tempfile::tempdir().unwrap();
    assert!(retain_newest(&dir.path().join("never-created"), 5)
        .unwrap()
        .is_empty());
}

#[test]
fn pruning_ignores_stray_files_beside_the_sessions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hand-written").unwrap();
    session(dir.path(), "s-1");

    // Deleting something a person put there by hand would be unforgivable.
    assert!(retain_newest(dir.path(), 0).unwrap().len() == 1);
    assert!(dir.path().join("notes.txt").exists());
}
