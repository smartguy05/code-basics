use super::super::record::RunKind;
use super::*;

fn rec(pid: u32, program: &str, started_at_ms: u64) -> RunningRecord {
    RunningRecord {
        pid,
        kind: RunKind::Terminal,
        label: "Server".into(),
        root: "/ws".into(),
        key: "sess-1".into(),
        program: program.into(),
        started_at_ms,
    }
}

fn info(name: &str, started_at_ms: u64) -> ProcInfo {
    ProcInfo {
        name: name.into(),
        started_at_ms,
    }
}

#[test]
fn a_dead_pid_is_dropped_silently() {
    let out = classify_orphans(vec![(rec(10, "node.exe", 1000), None)]);
    assert!(out.orphans.is_empty());
    assert!(
        out.warnings.is_empty(),
        "a gone process is expected, not noteworthy"
    );
}

#[test]
fn a_matching_live_pid_is_an_orphan() {
    let out = classify_orphans(vec![(
        rec(10, "node.exe", 1000),
        Some(info("node.exe", 1000)),
    )]);
    assert_eq!(out.orphans.len(), 1);
    assert!(out.warnings.is_empty());
}

#[test]
fn a_reused_pid_with_a_different_name_is_refused_with_a_warning() {
    // The pid is alive but now belongs to a different program — killing it would
    // kill a stranger's process. Never an orphan; a warning instead.
    let out = classify_orphans(vec![(
        rec(10, "node.exe", 1000),
        Some(info("chrome.exe", 999999)),
    )]);
    assert!(out.orphans.is_empty());
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].contains("chrome.exe"));
}

#[test]
fn same_name_but_a_far_start_time_is_reuse_not_the_original() {
    let out = classify_orphans(vec![(
        rec(10, "node.exe", 1000),
        Some(info("node.exe", 60_000)),
    )]);
    assert!(out.orphans.is_empty());
    assert_eq!(out.warnings.len(), 1);
}

#[test]
fn a_start_time_within_tolerance_still_matches() {
    // sysinfo rounds to seconds; a sub-tolerance difference is the same process.
    assert!(identity_matches(
        &rec(10, "node.exe", 1000),
        &info("node.exe", 2500)
    ));
    assert!(!identity_matches(
        &rec(10, "node.exe", 1000),
        &info("node.exe", 3001)
    ));
}

#[test]
fn the_exe_suffix_is_ignored_when_matching_names() {
    assert!(identity_matches(
        &rec(10, "node", 1000),
        &info("node.exe", 1000)
    ));
    assert!(identity_matches(
        &rec(10, "DOTNET.EXE", 1000),
        &info("dotnet", 1000)
    ));
}
