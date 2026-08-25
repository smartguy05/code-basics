//! Tests for the multi-workspace [`AppState`].
//!
//! The governing change from the single-workspace design: opening another
//! workspace no longer *forgets* the first — both stay live in their own slots,
//! and a record is attributed to the slot named by its explicit root rather than
//! refused for not being the active one.

use super::*;

use std::time::Duration;

use cb_core::inspect::{Caps, InspectGraph, InspectTarget, TargetSummary};
use cb_core::lsp::registry::Probe;
use cb_core::model::{TestCase, TestOutcome, TestRunResult, TestSummary};
use cb_core::symbols::index::SymbolIndex;

fn workspace_at(root: &str) -> Workspace {
    Workspace {
        root: PathBuf::from(root),
        name: "w".into(),
        projects: Vec::new(),
        configs: Vec::new(),
        solutions: Vec::new(),
        favorites: Vec::new(),
        order: Vec::new(),
    }
}

fn test_run() -> TestRunResult {
    TestRunResult {
        summary: Default::default(),
        cases: Vec::new(),
        duration_ms: None,
    }
}

/// A finished run with one failed test named after the repository it came from —
/// the name is what "re-run failed" turns into a filter, so a misattributed
/// result is recognisable by the repository it names.
fn failing_run_from(root: &str) -> TestRunResult {
    let case = TestCase {
        id: format!("{root}::Failing"),
        name: "Failing".into(),
        full_name: format!("{root}::Failing"),
        suite: None,
        project: None,
        outcome: TestOutcome::Failed,
        duration_ms: None,
        message: None,
        stack_trace: None,
        stdout: None,
    };
    TestRunResult {
        summary: TestSummary::from_cases(std::slice::from_ref(&case)),
        cases: vec![case],
        duration_ms: None,
    }
}

fn capture_under(root: &str) -> InspectGraph {
    InspectGraph {
        session_id: "s".into(),
        snapshot_id: "snap".into(),
        captured_at: "2026-01-01T00:00:00Z".into(),
        target: TargetSummary {
            target: InspectTarget::Dump {
                path: PathBuf::from(format!("{root}/.code-basics/dumps/Api.exe_1_2.dmp")),
            },
            bitness: None,
            runtime_version: None,
            process_name: None,
        },
        roots: Vec::new(),
        caps: Caps::default(),
        warnings: Vec::new(),
    }
}

fn index_at(root: &str) -> SymbolIndex {
    SymbolIndex {
        root: PathBuf::from(root),
        files: vec![PathBuf::from("a.rs")],
        symbols: Vec::new(),
        truncated: false,
    }
}

// -- Open / active / close ---------------------------------------------------

#[test]
fn opening_another_workspace_keeps_the_first_one_live() {
    // The inversion of the single-workspace rule: A's capture, test results and
    // symbol index survive opening B, because each lives in A's own slot and B
    // gets its own. Only closing A discards them.
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    assert!(state.record_inspect(Path::new("/a"), capture_under("/a")));
    assert!(state.record_test_run(Path::new("/a"), "cfg", test_run()));
    assert!(state.record_symbols(index_at("/a")));

    state.set_workspace(workspace_at("/b")).unwrap();

    // B is active and empty; A's data is untouched in A's slot.
    assert!(
        state.previous_inspect().is_none(),
        "B started with A's capture"
    );
    assert!(state.symbols().is_none(), "B started with A's index");

    state.set_active(Path::new("/a")).unwrap();
    assert!(
        state.previous_inspect().is_some(),
        "A's capture was discarded"
    );
    assert!(
        state.previous_test_run("cfg").is_some(),
        "A's run was discarded"
    );
    assert!(state.symbols().is_some(), "A's index was discarded");
}

#[test]
fn a_record_lands_in_the_workspace_it_names_regardless_of_which_is_active() {
    // A test run started in A that finishes after the user has switched to B must
    // record into A — this is why the record methods take an explicit root.
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap(); // B now active

    assert!(state.record_test_run(Path::new("/a"), "cfg", failing_run_from("/a")));
    assert!(state.record_symbols(index_at("/a")));

    // Nothing leaked into the active workspace B.
    assert!(state.previous_test_run("cfg").is_none());
    assert!(state.symbols().is_none());

    // And A has it.
    state.set_active(Path::new("/a")).unwrap();
    assert_eq!(
        state
            .previous_test_run("cfg")
            .map(|r| r.cases[0].full_name.clone())
            .as_deref(),
        Some("/a::Failing")
    );
}

#[test]
fn two_workspaces_with_the_same_config_id_keep_independent_results() {
    // The collision the per-slot supervisor and per-slot test-run cache exist to
    // fix: `cfg` is the same id in both same-layout repos, but each slot has its
    // own table so B's run does not evict A's.
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap();

    assert!(state.record_test_run(Path::new("/a"), "cfg", failing_run_from("/a")));
    assert!(state.record_test_run(Path::new("/b"), "cfg", failing_run_from("/b")));

    state.set_active(Path::new("/a")).unwrap();
    assert_eq!(
        state.previous_test_run("cfg").unwrap().cases[0].full_name,
        "/a::Failing"
    );
    state.set_active(Path::new("/b")).unwrap();
    assert_eq!(
        state.previous_test_run("cfg").unwrap().cases[0].full_name,
        "/b::Failing"
    );
}

#[test]
fn coverage_is_recorded_per_root_and_read_from_the_active_workspace() {
    use cb_core::testing::changecov::ChangeCoverage;

    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap(); // B now active

    let a_coverage = ChangeCoverage {
        changed_lines: 3,
        covered_lines: 2,
        uncovered_lines: 1,
        ..Default::default()
    };
    // Recorded against A even though B is active — keyed by explicit root.
    assert!(state.record_coverage(Path::new("/a"), "cfg", a_coverage.clone()));

    // B is active and has none; nothing leaked across.
    assert!(state.previous_coverage(Some("cfg")).is_none());

    state.set_active(Path::new("/a")).unwrap();
    assert_eq!(
        state
            .previous_coverage(Some("cfg"))
            .unwrap()
            .uncovered_lines,
        1
    );
    // The config-agnostic read finds the workspace's last coverage too.
    assert_eq!(state.previous_coverage(None).unwrap().changed_lines, 3);

    // Recording into a closed workspace is a harmless no-op.
    state.close(Path::new("/a"));
    assert!(!state.record_coverage(Path::new("/a"), "cfg", a_coverage));
}

#[test]
fn a_per_slot_supervisor_is_a_different_table_per_workspace() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap();
    let a = state.slot(Path::new("/a")).unwrap();
    let b = state.slot(Path::new("/b")).unwrap();
    // Two distinct process tables, so ids cannot collide across workspaces.
    assert!(!std::ptr::eq(
        &a.supervisor as *const _,
        &b.supervisor as *const _
    ));
}

#[test]
fn rescanning_the_active_workspace_keeps_its_live_state() {
    // Saving a configuration re-scans and sets the workspace again under the same
    // root. That must refresh the scanned workspace without dropping the index,
    // the captured tree, or the results "re-run failed" reads.
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    assert!(state.record_symbols(index_at("/a")));
    assert!(state.record_inspect(Path::new("/a"), capture_under("/a")));
    assert!(state.record_test_run(Path::new("/a"), "cfg", test_run()));

    let mut rescanned = workspace_at("/a");
    rescanned.name = "renamed".into();
    state.set_workspace(rescanned).unwrap();

    assert_eq!(
        state.workspace().unwrap().name,
        "renamed",
        "rescan did not refresh"
    );
    assert!(state.symbols().is_some(), "rescan dropped the index");
    assert!(
        state.previous_inspect().is_some(),
        "rescan dropped the capture"
    );
    assert!(
        state.previous_test_run("cfg").is_some(),
        "rescan dropped the results"
    );
}

#[test]
fn closing_a_workspace_removes_only_it_and_repoints_active() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap(); // B active
    assert!(state.record_symbols(index_at("/a")));

    let (removed, new_active) = state.close(Path::new("/b"));
    assert!(
        removed.is_some(),
        "the closed slot was not returned for teardown"
    );
    assert_eq!(
        new_active.as_deref(),
        Some(Path::new("/a")),
        "active did not repoint to A"
    );
    assert!(
        state.slot(Path::new("/b")).is_none(),
        "B's slot survived close"
    );

    // A is intact and now active.
    assert!(state.symbols().is_some());
}

#[test]
fn closing_the_last_workspace_leaves_nothing_active() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();

    let (_removed, new_active) = state.close(Path::new("/a"));
    assert!(new_active.is_none());
    assert!(state.workspace().is_err(), "nothing should be active");
}

#[test]
fn closing_a_background_workspace_leaves_the_active_one_alone() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap(); // B active

    let (_removed, new_active) = state.close(Path::new("/a"));
    assert_eq!(
        new_active.as_deref(),
        Some(Path::new("/b")),
        "closing A moved the active tab"
    );
}

#[test]
fn a_record_for_a_closed_workspace_is_a_harmless_no_op() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.close(Path::new("/a"));

    assert!(!state.record_test_run(Path::new("/a"), "cfg", test_run()));
    assert!(!state.record_symbols(index_at("/a")));
    assert!(!state.record_inspect(Path::new("/a"), capture_under("/a")));
    assert!(!state.update_symbols(Path::new("/a"), |i| i.files.clear()));
}

#[test]
fn set_active_refuses_a_workspace_that_is_not_open() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    assert!(state.set_active(Path::new("/b")).is_err());
    assert_eq!(
        state.workspace().unwrap().root,
        PathBuf::from("/a"),
        "active changed on a failed switch"
    );
}

#[test]
fn open_workspaces_lists_every_open_root() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap();
    let mut roots: Vec<_> = state
        .open_workspaces()
        .into_iter()
        .map(|w| w.root)
        .collect();
    roots.sort();
    assert_eq!(roots, [PathBuf::from("/a"), PathBuf::from("/b")]);
}

// -- Symbols index in-place edit ---------------------------------------------

#[test]
fn editing_the_index_targets_the_named_workspace() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    assert!(state.record_symbols(index_at("/a")));
    state.set_workspace(workspace_at("/b")).unwrap(); // B active

    // A save in A while B is active still splices into A.
    let applied = state.update_symbols(Path::new("/a"), |i| i.files.push(PathBuf::from("b.rs")));
    assert!(applied);

    state.set_active(Path::new("/a")).unwrap();
    assert_eq!(state.symbols().unwrap().files.len(), 2);
}

#[test]
fn editing_the_index_when_there_is_none_creates_nothing() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    assert!(!state.update_symbols(Path::new("/a"), |i| i.files.push(PathBuf::from("b.rs"))));
    assert!(state.symbols().is_none());
}

// -- Symbols build flag ------------------------------------------------------

#[test]
fn a_build_flag_is_per_workspace() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let _building = state.begin_symbols_build(); // for A (active)
    assert!(state.symbols_building());

    state.set_workspace(workspace_at("/b")).unwrap();
    assert!(!state.symbols_building(), "B reported A's build as its own");
}

#[test]
fn a_build_guard_clears_the_flag_when_it_drops() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    {
        let _building = state.begin_symbols_build();
        assert!(state.symbols_building());
    }
    assert!(!state.symbols_building());
}

#[test]
fn a_superseded_build_does_not_clear_the_newer_ones_flag() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let first = state.begin_symbols_build();
    let _second = state.begin_symbols_build();
    drop(first);
    assert!(
        state.symbols_building(),
        "the older build cleared the flag out from under the newer one"
    );
}

// -- Language server ---------------------------------------------------------

/// A machine with no language server installed anywhere, so a real session can be
/// started in a unit test without starting a real server.
struct NothingInstalled;

impl Probe for NothingInstalled {
    fn on_path(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn is_file(&self, _path: &Path) -> bool {
        false
    }
    fn is_dir(&self, _path: &Path) -> bool {
        false
    }
    fn read_dir(&self, _path: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
    fn env(&self, _key: &str) -> Option<String> {
        None
    }
}

/// A live session for `root`, claiming its generation the way `ensure_session`
/// does. The active workspace must be `root` for the generation to match.
fn session_for(state: &AppState, root: &str) -> LspHandle {
    cb_core::lsp::session::start_with_probe(
        PathBuf::from(root),
        None,
        state.begin_lsp_session(),
        Arc::new(NothingInstalled),
    )
}

fn answers(handle: &LspHandle) -> bool {
    !handle.status().servers.is_empty()
}

async fn tears_down(handle: &LspHandle) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while answers(handle) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the session never tore down");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_is_published_for_the_active_workspace() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let handle = session_for(&state, "/a");
    assert!(answers(&handle), "the fake session was inert");
    assert!(state.record_lsp_session(handle).is_ok());
    assert_eq!(
        state.lsp().map(|h| h.root().to_path_buf()),
        Some(PathBuf::from("/a"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_workspaces_hold_independent_sessions() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let a = session_for(&state, "/a");
    state.record_lsp_session(a.clone()).unwrap();

    state.set_workspace(workspace_at("/b")).unwrap();
    let b = session_for(&state, "/b");
    state.record_lsp_session(b.clone()).unwrap();

    // B active → B's session.
    assert_eq!(
        state.lsp().map(|h| h.root().to_path_buf()),
        Some(PathBuf::from("/b"))
    );
    // A still live in its slot.
    state.set_active(Path::new("/a")).unwrap();
    assert_eq!(
        state.lsp().map(|h| h.root().to_path_buf()),
        Some(PathBuf::from("/a"))
    );
    assert!(answers(&a));

    a.request_teardown();
    b.request_teardown();
    tears_down(&a).await;
    tears_down(&b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_workspace_tears_down_its_session_and_leaves_the_other() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let a = session_for(&state, "/a");
    state.record_lsp_session(a.clone()).unwrap();
    state.set_workspace(workspace_at("/b")).unwrap();
    let b = session_for(&state, "/b");
    state.record_lsp_session(b.clone()).unwrap();

    let (removed, _new_active) = state.close(Path::new("/a"));
    // The command tears the removed slot's server down; do here what it does.
    if let Some(slot) = removed {
        if let Some(handle) = slot.take_lsp() {
            handle.request_teardown();
        }
    }
    tears_down(&a).await;
    assert!(answers(&b), "closing A tore down B's session too");

    b.request_teardown();
    tears_down(&b).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rescanning_the_active_workspace_keeps_its_session() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let handle = session_for(&state, "/a");
    state.record_lsp_session(handle.clone()).unwrap();

    state.set_workspace(workspace_at("/a")).unwrap(); // rescan

    assert!(state.lsp().is_some(), "a rescan threw the session away");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(answers(&handle), "a rescan tore the session down");
    handle.request_teardown();
    tears_down(&handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_for_a_non_active_root_is_handed_back_for_teardown() {
    // A session started for A but recorded while B is active is refused and
    // handed back — a running server tree the caller must tear down.
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let handle = session_for(&state, "/a");
    state.set_workspace(workspace_at("/b")).unwrap();

    let Err(rejected) = state.record_lsp_session(handle) else {
        panic!("a session for a non-active root was published");
    };
    assert!(state.lsp().is_none());
    assert!(answers(&rejected), "the handback was not the live session");
    rejected.request_teardown();
    tears_down(&rejected).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_superseded_start_is_refused_rather_than_leaked() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let superseded = session_for(&state, "/a");
    let current = session_for(&state, "/a"); // bumps the generation

    let Err(rejected) = state.record_lsp_session(superseded) else {
        panic!("a superseded start published itself over the current one");
    };
    rejected.request_teardown();
    tears_down(&rejected).await;

    state.record_lsp_session(current.clone()).unwrap();
    assert_eq!(
        state.lsp().map(|h| h.generation()),
        Some(current.generation())
    );
    current.request_teardown();
    tears_down(&current).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_session_for_one_workspace_is_refused_rather_than_leaked() {
    let state = AppState::default();
    state.set_workspace(workspace_at("/a")).unwrap();
    let first = session_for(&state, "/a");
    state.record_lsp_session(first.clone()).unwrap();

    let second = session_for(&state, "/a");
    let Err(rejected) = state.record_lsp_session(second) else {
        panic!("a second session was published over a live one, leaking it");
    };
    rejected.request_teardown();
    tears_down(&rejected).await;
    assert!(answers(&first), "the incumbent was torn down instead");
    first.request_teardown();
    tears_down(&first).await;
}

#[test]
fn with_no_workspace_open_there_is_no_active_slot() {
    let state = AppState::default();
    assert!(state.active_slot().is_err());
    assert!(state.workspace().is_err());
    assert!(state.active_workspace_opt().is_none());
    assert!(state.symbols().is_none());
    assert!(!state.symbols_building());
    assert!(state.lsp().is_none());
}
