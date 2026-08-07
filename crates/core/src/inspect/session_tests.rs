use super::*;

use crate::config::{self, WorkspaceConfig};
use crate::inspect::model::{Caps, InspectorConfig};

fn sidecar_at(dir: &Path, bitness: Bitness) {
    std::fs::write(dir.join(sidecar::sidecar_file_name(bitness)), "").unwrap();
}

// ---------------------------------------------------------------------------
// Session ids
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_started_together_get_different_names() {
    // Expanding a node fires a capture immediately after the previous one, so
    // a timestamp alone would collide and the second capture would overwrite
    // the first one's request while it was still being read.
    let ids: Vec<String> = (0..50).map(|_| new_session_id()).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();

    assert_eq!(unique.len(), ids.len(), "collided: {ids:?}");
}

#[test]
fn a_session_id_is_safe_as_a_directory_name() {
    let id = new_session_id();
    assert!(
        id.bytes().all(|b| b.is_ascii_digit() || b == b'-'),
        "got {id}"
    );
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[test]
fn a_request_uses_the_workspace_caps() {
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                caps: Some(Caps {
                    max_depth: 2,
                    ..Caps::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let request = request_for(
        dir.path(),
        InspectTarget::Live { pid: 7 },
        RootSpec::Exceptions,
        None,
    );

    assert_eq!(request.caps.max_depth, 2);
    assert_eq!(request.target, InspectTarget::Live { pid: 7 });
    assert_eq!(request.schema_version, super::super::SCHEMA_VERSION);
}

#[test]
fn an_unreadable_config_still_produces_a_request() {
    // Broken settings are a reason to go and look at something, not a reason
    // to refuse to.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(config::config_dir(dir.path())).unwrap();
    std::fs::write(config::config_path(dir.path()), "{ not json").unwrap();

    let request = request_for(
        dir.path(),
        InspectTarget::Live { pid: 1 },
        RootSpec::Exceptions,
        None,
    );

    assert_eq!(request.caps, Caps::default());
}

// ---------------------------------------------------------------------------
// Choosing an executable
// ---------------------------------------------------------------------------

#[test]
fn x64_is_preferred_when_both_builds_are_present() {
    let dir = tempfile::tempdir().unwrap();
    sidecar_at(dir.path(), Bitness::X64);
    sidecar_at(dir.path(), Bitness::X86);

    assert_eq!(first_bitness(Some(dir.path())), Some(Bitness::X64));
}

#[test]
fn the_only_installed_build_is_the_one_chosen() {
    let dir = tempfile::tempdir().unwrap();
    sidecar_at(dir.path(), Bitness::X86);

    assert_eq!(first_bitness(Some(dir.path())), Some(Bitness::X86));
}

#[test]
fn nothing_installed_chooses_nothing() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(first_bitness(Some(dir.path())), None);
    assert_eq!(first_bitness(None), None);
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[test]
fn an_absent_sidecar_is_reported_as_unavailable_with_the_command_to_fix_it() {
    let dir = tempfile::tempdir().unwrap();

    let status = status(dir.path(), None);

    assert!(!status.available);
    let reason = status.unavailable_reason.expect("a reason");
    assert!(reason.contains("pnpm sidecar:build"), "got {reason}");
}

/// A fresh checkout has no sidecar — `cargo build` does not produce one — so
/// the message has to read as an ordinary missing component and say exactly
/// what to type, not as a broken installation.
#[test]
fn the_missing_sidecar_message_names_the_command_and_the_override() {
    let reason = missing_sidecar_reason();

    assert!(reason.contains("pnpm sidecar:build"), "got {reason}");
    assert!(reason.contains("CB_INSPECTOR_PATH"), "got {reason}");
    assert!(reason.contains(".NET"), "got {reason}");
}

/// The message names the file the user should expect to find, so it has to be
/// the real one rather than a hard-coded spelling that could drift.
#[test]
fn the_missing_sidecar_message_names_the_binary_it_looked_for() {
    let reason = missing_sidecar_reason();

    assert!(
        reason.contains(sidecar::sidecar_file_name(Bitness::X64)),
        "got {reason}"
    );
}

/// Status reports the same reason the function gives, so the UI and any other
/// caller cannot disagree about why the feature is off.
#[test]
fn the_reason_status_reports_is_the_missing_sidecar_message_itself() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(
        status(dir.path(), None).unavailable_reason,
        Some(missing_sidecar_reason())
    );
}

#[test]
fn an_installed_sidecar_is_available_and_offers_no_reason() {
    let workspace = tempfile::tempdir().unwrap();
    let resources = tempfile::tempdir().unwrap();
    sidecar_at(resources.path(), Bitness::X64);

    let status = status(workspace.path(), Some(resources.path()));

    assert!(status.available);
    assert_eq!(status.unavailable_reason, None);
}

#[test]
fn capture_is_off_until_the_workspace_says_otherwise() {
    let dir = tempfile::tempdir().unwrap();

    assert!(!status(dir.path(), None).dump_capture_enabled);
}

#[test]
fn capture_reads_as_on_once_the_workspace_opts_in() {
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(status(dir.path(), None).dump_capture_enabled);
}

#[test]
fn every_status_warns_that_a_dump_is_a_copy_of_process_memory() {
    // This application manages .NET user secrets, so the processes it runs
    // hold credentials. The warning is not conditional on anything.
    let dir = tempfile::tempdir().unwrap();

    for enabled in [false, true] {
        config::save(
            dir.path(),
            &WorkspaceConfig {
                inspector: Some(InspectorConfig {
                    capture_dumps: enabled,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();

        let status = status(dir.path(), None);
        assert!(
            status
                .caveats
                .iter()
                .any(|c| c.contains("process memory") && c.contains("tokens")),
            "capture_dumps={enabled} produced {:?}",
            status.caveats
        );
    }
}

#[test]
fn status_states_that_nothing_is_executed() {
    // A user arriving from a debugger will otherwise expect to evaluate a
    // property, and quietly misread `_items` as a bug.
    let dir = tempfile::tempdir().unwrap();

    let status = status(dir.path(), None);

    assert!(
        status
            .caveats
            .iter()
            .any(|c| c.contains("no method is called")),
        "got {:?}",
        status.caveats
    );
}

#[test]
fn an_enabled_workspace_is_told_a_caught_exception_leaves_no_dump() {
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let status = status(dir.path(), None);

    assert!(
        status
            .caveats
            .iter()
            .any(|c| c.contains("unhandled exception")),
        "got {:?}",
        status.caveats
    );
}

#[test]
fn dumps_on_disk_are_listed_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let dumps_dir = dumps::dumps_dir(dir.path());
    std::fs::create_dir_all(&dumps_dir).unwrap();
    std::fs::write(dumps_dir.join("Api.exe_10_100.dmp"), "a").unwrap();
    std::fs::write(dumps_dir.join("Api.exe_11_200.dmp"), "bb").unwrap();

    let status = status(dir.path(), None);

    let stamps: Vec<u64> = status.dumps.iter().map(|d| d.captured_at).collect();
    assert_eq!(stamps, vec![200, 100]);
}

// ---------------------------------------------------------------------------
// Pruning
// ---------------------------------------------------------------------------

#[test]
fn pruning_a_workspace_that_has_never_captured_reports_no_problems() {
    let dir = tempfile::tempdir().unwrap();

    assert!(prune(dir.path()).is_empty());
}

#[test]
fn pruning_applies_the_workspace_dump_budget() {
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                keep_dumps: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let dumps_dir = dumps::dumps_dir(dir.path());
    std::fs::create_dir_all(&dumps_dir).unwrap();
    std::fs::write(dumps_dir.join("Api.exe_10_100.dmp"), "a").unwrap();
    std::fs::write(dumps_dir.join("Api.exe_11_200.dmp"), "b").unwrap();

    assert!(prune(dir.path()).is_empty());

    let left = dumps::list(&dumps_dir);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].captured_at, 200);
}

#[test]
fn a_re_read_of_a_capped_branch_asks_for_more_than_stopped_it() {
    // Expanding is this same request with an address root. Issued with the
    // caps that produced the elision it would return the identical truncation,
    // and the user would be told a fresh read had happened for no new data.
    let dir = tempfile::tempdir().unwrap();

    let plain = request_for(
        dir.path(),
        InspectTarget::Dump {
            path: "d.dmp".into(),
        },
        RootSpec::Address {
            address: "0x1234".into(),
        },
        None,
    );
    let widened = request_for(
        dir.path(),
        InspectTarget::Dump {
            path: "d.dmp".into(),
        },
        RootSpec::Address {
            address: "0x1234".into(),
        },
        Some(ElidedReason::ChildLimit),
    );

    assert!(
        widened.caps.max_children > plain.caps.max_children,
        "{:?} vs {:?}",
        widened.caps,
        plain.caps
    );
}

// ---------------------------------------------------------------------------
// Arming a run
// ---------------------------------------------------------------------------

#[test]
fn nothing_is_armed_for_a_workspace_that_did_not_opt_in() {
    let dir = tempfile::tempdir().unwrap();

    assert!(arm_dumps(dir.path()).is_none());
    assert!(!dumps::dumps_dir(dir.path()).exists());
}

#[test]
fn arming_a_run_hands_over_the_workspace_inspector_env() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = BTreeMap::new();
    env.insert("DOTNET_DbgMiniDumpType".to_string(), "4".to_string());

    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                env,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let armed = arm_dumps(dir.path()).expect("capture is on");

    assert_eq!(armed.dir, dumps::dumps_dir(dir.path()));
    assert_eq!(
        armed.env.get("DOTNET_DbgMiniDumpType").map(String::as_str),
        Some("4")
    );
    assert!(armed.dir.exists());
}

#[test]
fn arming_a_run_prunes_what_earlier_crashes_left_behind() {
    // The workspace that most needs a bound on disk use is the one that
    // crashes repeatedly and is never inspected: pruning only after a capture
    // means the budget is applied by users who already came looking.
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                keep_dumps: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let dumps_dir = dumps::dumps_dir(dir.path());
    std::fs::create_dir_all(&dumps_dir).unwrap();
    std::fs::write(dumps_dir.join("Api.exe_10_100.dmp"), "a").unwrap();
    std::fs::write(dumps_dir.join("Api.exe_11_200.dmp"), "b").unwrap();
    std::fs::write(dumps_dir.join("Api.exe_12_300.dmp"), "c").unwrap();

    arm_dumps(dir.path()).expect("capture is on");

    let left = dumps::list(&dumps_dir);
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(left[0].captured_at, 300);
}

#[test]
fn the_dump_budget_also_covers_the_test_hosts_dumps() {
    // VSTest's blame collector writes into the results directory under a name
    // this app cannot decode. They are still memory this app asked to be
    // written, so the workspace's byte budget has to see them.
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                max_dump_megabytes: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let results = config::results_dir(dir.path());
    std::fs::create_dir_all(results.join("host")).unwrap();
    let blame = results.join("host").join("testhost_crashdump.dmp");
    std::fs::write(&blame, vec![0u8; 4096]).unwrap();
    // Not a dump: the sweep must leave a run's report alone.
    let report = results.join("suite.trx");
    std::fs::write(&report, "<TestRun/>").unwrap();

    assert!(prune(dir.path()).is_empty());

    assert!(!blame.exists(), "the blame dump escaped the budget");
    assert!(report.exists(), "pruning deleted a test report");
}

// ---------------------------------------------------------------------------
// Caveats
// ---------------------------------------------------------------------------

#[test]
fn the_caveats_never_claim_a_dump_was_matched_to_a_run() {
    // Nothing performs that matching: the Objects tab lists every dump in the
    // workspace. Claiming an attribution that was not made is exactly the
    // failure this module's abstain rule exists to prevent.
    let dir = tempfile::tempdir().unwrap();
    config::save(
        dir.path(),
        &WorkspaceConfig {
            inspector: Some(InspectorConfig {
                capture_dumps: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .unwrap();

    let status = status(dir.path(), None);

    assert!(
        !status
            .caveats
            .iter()
            .any(|c| c.contains("matched to a run")),
        "got {:?}",
        status.caveats
    );
    // What is true is still said: the executable in the name is how a reader
    // tells their application's dump from its build host's.
    assert!(
        status
            .caveats
            .iter()
            .any(|c| c.contains("executable that wrote it")),
        "got {:?}",
        status.caveats
    );
}

// ---------------------------------------------------------------------------
// Attaching to a running process
// ---------------------------------------------------------------------------

fn config(id: &str, name: &str) -> RunConfig {
    RunConfig::new(
        id,
        name,
        crate::model::RunKind::App,
        "dotnet",
        crate::model::ConfigSource::Detected,
    )
}

/// A published .NET process, named and parented.
fn process(pid: u32, name: &str, parent: Option<u32>) -> DotnetProcess {
    DotnetProcess {
        pid,
        name: name.to_string(),
        path: None,
        parent_pid: parent,
        started_at: None,
    }
}

/// A published .NET process that also reported when it started.
fn process_at(pid: u32, name: &str, parent: Option<u32>, started_at: &str) -> DotnetProcess {
    DotnetProcess {
        started_at: Some(started_at.to_string()),
        ..process(pid, name, parent)
    }
}

/// The entry for a pid, or a readable failure naming what was there instead.
fn entry(found: &[AttachableProcess], pid: u32) -> &AttachableProcess {
    found
        .iter()
        .find(|p| p.pid == pid)
        .unwrap_or_else(|| panic!("no entry for pid {pid} in {found:?}"))
}

#[test]
fn the_pid_the_supervisor_launched_is_attributed_to_its_configuration() {
    // The simple case: a process that is not a launcher. Its pid is what we
    // started, which is the strongest evidence there is.
    let found = attribute(
        &[process(100, "Api", Some(1))],
        &[("api".to_string(), Some(100))],
        &[config("api", "Api")],
    );

    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].attribution, Attribution::Launched);
    assert_eq!(found[0].config_id.as_deref(), Some("api"));
    assert_eq!(found[0].config_name.as_deref(), Some("Api"));
}

#[test]
fn the_child_of_a_dotnet_run_is_attributed_to_the_same_configuration() {
    // The observed shape this whole phase exists for, with the real pids: the
    // supervisor holds 8352, which is the .NET CLI, while every one of the
    // user's objects is in 9960.
    let found = attribute(
        &[
            process(8352, "dotnet", Some(28684)),
            process(9960, "Crasher", Some(8352)),
        ],
        &[("crash".to_string(), Some(8352))],
        &[config("crash", "Crasher")],
    );

    let child = entry(&found, 9960);
    assert_eq!(child.attribution, Attribution::Descendant);
    assert_eq!(child.config_id.as_deref(), Some("crash"));
    // The application itself is never labelled a launcher — that caveat belongs
    // on the pid it does not apply to.
    assert_eq!(child.launcher_caveat, None);

    assert_eq!(entry(&found, 8352).attribution, Attribution::Launched);
}

#[test]
fn a_grandchild_is_still_the_users_process() {
    // A launcher that starts a host that starts the application. Nothing in the
    // rule cares about depth, only that the chain is unbroken and stays inside
    // the processes we were actually given.
    let found = attribute(
        &[
            process(10, "dotnet", None),
            process(11, "apphost", Some(10)),
            process(12, "Api", Some(11)),
        ],
        &[("api".to_string(), Some(10))],
        &[config("api", "Api")],
    );

    assert_eq!(entry(&found, 12).attribution, Attribution::Descendant);
    assert_eq!(entry(&found, 12).config_id.as_deref(), Some("api"));
}

#[test]
fn a_cycle_in_the_parent_links_terminates_and_attributes_nothing() {
    // Parent pids are another process's account of a world that has already
    // moved on, and nothing forbids them contradicting themselves. A walk that
    // trusted them would hang the poll that fills the picker.
    let found = attribute(
        &[
            process(20, "A", Some(21)),
            process(21, "B", Some(22)),
            process(22, "C", Some(20)),
        ],
        &[("api".to_string(), Some(999))],
        &[config("api", "Api")],
    );

    assert_eq!(found.len(), 3, "got {found:?}");
    for p in &found {
        assert_eq!(p.attribution, Attribution::Unrelated, "got {p:?}");
        assert_eq!(p.config_id, None);
    }
}

#[test]
fn a_process_that_is_its_own_parent_terminates_and_attributes_nothing() {
    let found = attribute(
        &[process(30, "Odd", Some(30))],
        &[("api".to_string(), Some(1))],
        &[config("api", "Api")],
    );

    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].attribution, Attribution::Unrelated);
}

#[test]
fn a_chain_that_leaves_the_listed_processes_stops_rather_than_assuming() {
    // 41 is not in the list, so what its parent might have been is not known —
    // and the fact that a launched pid exists somewhere on the machine is not a
    // reason to claim this process descends from it.
    let found = attribute(
        &[process(40, "Stranger", Some(41))],
        &[("api".to_string(), Some(42))],
        &[config("api", "Api")],
    );

    assert_eq!(found[0].attribution, Attribution::Unrelated);
    assert_eq!(found[0].config_name, None);
}

#[test]
fn a_process_with_no_parent_information_is_never_speculatively_attributed() {
    // Missing means unknown, not "has no parent". A process whose parent could
    // not be read might well be the user's application, and claiming it is
    // would put a stranger's heap under their configuration's name.
    let found = attribute(
        &[process(50, "Mystery", None)],
        &[("api".to_string(), Some(51))],
        &[config("api", "Api")],
    );

    assert_eq!(found[0].attribution, Attribution::Unrelated);
    assert_eq!(found[0].config_id, None);
}

#[test]
fn the_nearest_launched_ancestor_wins() {
    // Two configurations where one launched the other's launcher — contrived,
    // but the rule has to be decidable. The process belongs to what started it
    // most recently, not to whatever is furthest up the chain.
    let found = attribute(
        &[
            process(60, "outer", None),
            process(61, "inner", Some(60)),
            process(62, "App", Some(61)),
        ],
        &[
            ("outer".to_string(), Some(60)),
            ("inner".to_string(), Some(61)),
        ],
        &[config("outer", "Outer"), config("inner", "Inner")],
    );

    assert_eq!(entry(&found, 62).config_id.as_deref(), Some("inner"));
}

#[test]
fn two_configurations_running_at_once_keep_their_own_children() {
    // Pids are interleaved deliberately: nothing may be attributed by ordering
    // or by proximity of numbers, only by the links.
    let found = attribute(
        &[
            process(100, "dotnet", None),
            process(101, "dotnet", None),
            process(102, "Api", Some(100)),
            process(103, "Web", Some(101)),
        ],
        &[
            ("api".to_string(), Some(100)),
            ("web".to_string(), Some(101)),
        ],
        &[config("api", "Api"), config("web", "Web")],
    );

    assert_eq!(entry(&found, 102).config_id.as_deref(), Some("api"));
    assert_eq!(entry(&found, 103).config_id.as_deref(), Some("web"));
}

#[test]
fn the_inspectors_own_sidecar_is_never_an_attach_target() {
    // It is a .NET process, so enumerating the machine finds it — including a
    // capture running in another window of this application, which this
    // supervisor knows nothing about. Offering to inspect the inspector is
    // absurd; a build host is merely useless. Both go, and so does anything
    // they started.
    let found = attribute(
        &[
            process(400, "cb-inspector-win-x64", None),
            process(401, "cb-inspector-win-x64", None),
            process(200, "MSBuild", None),
            process(201, "MSBuild", Some(200)),
            process(300, "git", None),
            process(100, "Api", None),
        ],
        &[
            ("api".to_string(), Some(100)),
            ("api:build".to_string(), Some(200)),
            ("git:network".to_string(), Some(300)),
            ("inspect:1770000000-0001".to_string(), Some(400)),
        ],
        &[config("api", "Api")],
    );

    assert_eq!(
        found.iter().map(|p| p.pid).collect::<Vec<_>>(),
        vec![100],
        "got {found:?}"
    );
}

#[test]
fn a_supervisor_id_that_names_no_configuration_attributes_nothing() {
    // The filter is positive: an id has to *be* a configuration's id. A
    // supervisor id invented later is therefore excluded without anyone
    // remembering to exclude it — and its process is dropped rather than shown,
    // because it is this application's own noise.
    assert!(attribute(
        &[process(9, "whatever", None)],
        &[("something:new".to_string(), Some(9))],
        &[]
    )
    .is_empty());
}

#[test]
fn a_compound_configuration_is_not_attributed_but_its_members_are() {
    // A compound launches its members and runs nothing itself; its members are
    // what the supervisor actually holds.
    let mut compound = config("all", "Everything");
    compound.compound = vec!["api".into()];
    let configs = vec![compound, config("api", "Api")];

    let found = attribute(
        &[process(1, "shell", None), process(2, "Api", None)],
        &[("all".to_string(), Some(1)), ("api".to_string(), Some(2))],
        &configs,
    );

    assert_eq!(found.iter().map(|a| a.pid).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn a_configuration_whose_process_has_gone_attributes_nothing() {
    // Spawned and already exited. Attaching to a pid the OS may have reused is
    // the wrong-value failure this feature exists to avoid, and a supervisor
    // entry with no pid cannot claim one.
    let found = attribute(
        &[process(1, "Stranger", None)],
        &[("api".to_string(), None)],
        &[config("api", "Api")],
    );

    assert_eq!(found[0].attribution, Attribution::Unrelated);
}

#[test]
fn the_users_own_processes_are_listed_first_and_in_a_stable_order() {
    // A menu that reshuffles between polls moves the entry under the cursor,
    // and the reason to open this list at all is the user's own application —
    // it must not be below somebody's editor.
    let found = attribute(
        &[
            process(9, "zeta", None),
            process(8, "alpha", None),
            process(2, "Web", Some(1)),
            process(1, "dotnet", None),
        ],
        &[("web".to_string(), Some(1))],
        &[config("web", "Web")],
    );

    assert_eq!(
        found
            .iter()
            .map(|p| (p.name.as_str(), p.pid))
            .collect::<Vec<_>>(),
        vec![("Web", 2), ("dotnet", 1), ("alpha", 8), ("zeta", 9)]
    );
}

#[test]
fn a_crash_exception_cannot_be_asked_of_a_running_process() {
    // Refused before spawning, because finding out afterwards would have cost
    // the user's application a snapshot for nothing.
    let reason = unsupported_reason(&InspectTarget::Live { pid: 7 }, &RootSpec::CrashException)
        .expect("a running process has no crash exception");
    assert!(reason.contains("has not crashed"), "got {reason}");

    // The same root is exactly what a dump is for.
    assert!(unsupported_reason(
        &InspectTarget::Dump {
            path: "a.dmp".into()
        },
        &RootSpec::CrashException
    )
    .is_none());
    // A live root that may simply find nothing is allowed: an empty answer is
    // an honest one, and refusing it would hide the only route to a caught
    // exception.
    assert!(unsupported_reason(&InspectTarget::Live { pid: 7 }, &RootSpec::Exceptions).is_none());
}

#[test]
fn the_cost_of_attaching_is_stated_before_it_is_paid() {
    // A live capture clones the target's process image. The user has to be
    // able to read that where the decision is made, not discover it as a pause
    // in something serving traffic.
    let dir = tempfile::tempdir().unwrap();
    let caveats = status(dir.path(), None).caveats;

    assert!(
        caveats
            .iter()
            .any(|c| c.contains("copies its memory image")),
        "got {caveats:?}"
    );
}

// ---------------------------------------------------------------------------
// Whose process is behind the pid
// ---------------------------------------------------------------------------

#[test]
fn a_launcher_with_a_living_child_is_labelled_and_points_at_the_child() {
    // The strongest form of the warning, and the one this phase made possible:
    // the child is not inferred from the shape of the configuration, it is on
    // the same list, by pid. A capture of the launcher finds none of the user's
    // types and renders an empty tree that reads exactly like "your object is
    // not there" — the one wrong value this feature exists to prevent.
    let found = attribute(
        &[
            process(8352, "dotnet", None),
            process(9960, "Crasher", Some(8352)),
        ],
        &[("crash".to_string(), Some(8352))],
        &[config("crash", "Crasher")],
    );

    let caveat = entry(&found, 8352)
        .launcher_caveat
        .as_deref()
        .expect("a launcher whose child is running has to say so");
    assert!(caveat.contains("child process"), "got {caveat}");
    assert!(
        caveat.contains("not be evidence"),
        "the caveat has to deny the empty result, not just describe the launcher: {caveat}"
    );
    assert!(
        caveat.contains("Crasher") && caveat.contains("9960"),
        "the caveat knows which process to point at, so it must: {caveat}"
    );
}

#[test]
fn a_launcher_with_several_children_names_none_of_them() {
    // A `dotnet run` can leave an MSBuild worker running beside the
    // application. Picking one of them to name would be exactly the guess this
    // module refuses to make, so it says how many there are and what to look
    // for instead.
    let found = attribute(
        &[
            process(10, "dotnet", None),
            process(11, "Api", Some(10)),
            process(12, "MSBuild", Some(10)),
        ],
        &[("api".to_string(), Some(10))],
        &[config("api", "Api")],
    );

    let caveat = entry(&found, 10).launcher_caveat.as_deref().unwrap();
    assert!(caveat.contains('2'), "got {caveat}");
    assert!(
        !caveat.contains("(pid 11)") && !caveat.contains("(pid 12)"),
        "with more than one candidate the caveat must not pick one: {caveat}"
    );
}

#[test]
fn a_dotnet_cli_whose_application_has_not_appeared_yet_still_warns() {
    // Between `dotnet run` starting and the application publishing a
    // diagnostics channel, the only process on the list is the CLI. Saying
    // nothing would let the user capture it and read an empty heap as an
    // answer; the warning is earned by the observed process being `dotnet` for
    // a configuration this application builds as `dotnet run …`.
    let found = attribute(
        &[process(5000, "dotnet", None)],
        &[("api".to_string(), Some(5000))],
        &[config("api", "Api")],
    );

    let caveat = found[0]
        .launcher_caveat
        .as_deref()
        .expect("the .NET CLI is never the application");
    assert!(caveat.contains("still building"), "got {caveat}");
}

#[test]
fn a_launched_process_that_is_the_application_carries_no_caveat() {
    // The point of the change: the old rule warned on every .NET application
    // configuration, including the ones whose pid *was* the application. A
    // caveat that is wrong half the time is one people learn to skip, so it is
    // now only produced where there is evidence for it.
    let found = attribute(
        &[process(5000, "Api", None)],
        &[("api".to_string(), Some(5000))],
        &[config("api", "Api")],
    );

    assert_eq!(
        found[0].launcher_caveat, None,
        "a .NET process that is not the CLI and has no children is the application"
    );
}

#[test]
fn a_configuration_not_known_to_use_a_launcher_is_offered_without_a_caveat() {
    // Abstention runs both ways: a caveat attached to a process that *is* the
    // application would train the reader to ignore it on the one that is not.
    let mut node = config("web", "Web");
    node.ecosystem = "node".into();

    let mut test = config("unit", "Unit tests");
    test.kind = crate::model::RunKind::Test;

    for one in [node, test] {
        // Named `dotnet` on purpose: even that is not enough on its own for a
        // configuration this application does not build as `dotnet run …`.
        let found = attribute(
            &[process(1, "dotnet", None)],
            &[(one.id.clone(), Some(1))],
            std::slice::from_ref(&one),
        );
        assert_eq!(
            found[0].launcher_caveat, None,
            "{} should carry no launcher caveat",
            one.id
        );
    }
}

#[test]
fn a_launched_application_with_a_net_child_of_its_own_is_not_called_a_launcher() {
    // An apphost is the launched pid *and* the application, and applications
    // start .NET children all the time — a worker, an out-of-process plugin
    // host, a `dotnet ef`. Reading "has a child" as "is a launcher" told the
    // user that the process holding their objects had "built the project", and
    // sent them to the worker instead.
    let found = attribute(
        &[
            process(100, "Api", Some(1)),
            process(101, "Worker", Some(100)),
        ],
        &[("api".to_string(), Some(100))],
        &[config("api", "Api")],
    );

    let app = entry(&found, 100);
    assert_eq!(
        app.launcher_caveat, None,
        "the application itself must not be described as a launcher: {app:?}"
    );
    assert!(app.is_application, "got {app:?}");

    // The child is the user's too, but nothing says it is the application.
    let child = entry(&found, 101);
    assert_eq!(child.attribution, Attribution::Descendant);
    assert!(
        !child.is_application,
        "a worker must never be offered as the application: {child:?}"
    );
}

#[test]
fn a_dotnet_cli_whose_only_child_is_a_build_server_is_never_named_as_the_application() {
    // During the build phase of a `dotnet run` the only published descendant is
    // the compiler server or an MSBuild node — they publish diagnostics
    // channels, they outlive the build, and they are not what anyone opened
    // this tab for. Naming one as "the application itself" is the guess the
    // several-children arm was written to refuse, made with more confidence
    // because there happened to be only one.
    let found = attribute(
        &[
            process(8352, "dotnet", None),
            process(9000, "VBCSCompiler", Some(8352)),
        ],
        &[("crash".to_string(), Some(8352))],
        &[config("crash", "Crasher")],
    );

    let caveat = entry(&found, 8352)
        .launcher_caveat
        .as_deref()
        .expect("the CLI still holds none of the user's objects");
    assert!(
        !caveat.contains("The application itself"),
        "a build server must not be presented as the application: {caveat}"
    );
    assert!(
        !caveat.contains("(pid 9000)"),
        "the caveat must not point the user at the compiler server: {caveat}"
    );

    for pid in [8352, 9000] {
        assert!(
            !entry(&found, pid).is_application,
            "nothing here is known to be the application: {:?}",
            entry(&found, pid)
        );
    }
}

#[test]
fn the_child_a_launcher_names_is_the_one_marked_as_the_application() {
    // The caveat and the preselection are one decision, not two: a warning
    // naming `Crasher` beside a capture button aimed at the CLI would be the
    // same disagreement in a new place.
    let found = attribute(
        &[
            process(8352, "dotnet", None),
            process(9960, "Crasher", Some(8352)),
        ],
        &[("crash".to_string(), Some(8352))],
        &[config("crash", "Crasher")],
    );

    assert!(entry(&found, 9960).is_application);
    assert!(
        !entry(&found, 8352).is_application,
        "the launcher holds none of the user's objects"
    );
}

#[test]
fn a_launcher_with_several_children_marks_none_of_them_as_the_application() {
    // The caveat abstains here, so the preselection must too — otherwise the
    // sentence saying "pick the one named after your project" would sit above a
    // target already picked at random.
    let found = attribute(
        &[
            process(10, "dotnet", None),
            process(11, "Api", Some(10)),
            process(12, "MSBuild", Some(10)),
        ],
        &[("api".to_string(), Some(10))],
        &[config("api", "Api")],
    );

    for pid in [10, 11, 12] {
        assert!(
            !entry(&found, pid).is_application,
            "got {:?}",
            entry(&found, pid)
        );
    }
}

#[test]
fn a_recycled_parent_pid_does_not_attribute_a_stranger_to_a_run() {
    // A long-lived service whose real parent exited months ago still reports
    // that dead pid; Windows then hands the number to a `dotnet run` CLI. The
    // parent link now reads as ancestry, and without a start time to check it
    // against, a stranger's heap is rendered under the user's configuration
    // name — the wrong value this module exists to prevent, one link further
    // along than the pid check made before a capture.
    let found = attribute(
        &[
            process_at(8352, "dotnet", None, "2026-08-06T13:00:00.0000000Z"),
            process_at(500, "Service", Some(8352), "2026-05-01T09:00:00.0000000Z"),
        ],
        &[("api".to_string(), Some(8352))],
        &[config("api", "Api")],
    );

    let stranger = entry(&found, 500);
    assert_eq!(
        stranger.attribution,
        Attribution::Unrelated,
        "got {stranger:?}"
    );
    assert_eq!(stranger.config_id, None);
    assert_eq!(stranger.config_name, None);
    assert!(!stranger.is_application);

    // And with nothing of the user's under it, the CLI is not told it has one.
    let caveat = entry(&found, 8352).launcher_caveat.as_deref().unwrap();
    assert!(caveat.contains("still building"), "got {caveat}");
}

#[test]
fn a_recycled_pid_part_way_up_a_chain_stops_the_walk_there() {
    // The check belongs on every link, not only the one that hands out a
    // configuration name: a grandchild reached through a recycled middle pid is
    // exactly as wrongly attributed.
    let found = attribute(
        &[
            process_at(10, "dotnet", None, "2026-08-06T13:00:00.0000000Z"),
            process_at(11, "host", Some(10), "2026-01-01T09:00:00.0000000Z"),
            process_at(12, "App", Some(11), "2026-08-06T13:00:05.0000000Z"),
        ],
        &[("api".to_string(), Some(10))],
        &[config("api", "Api")],
    );

    assert_eq!(entry(&found, 11).attribution, Attribution::Unrelated);
    assert_eq!(entry(&found, 12).attribution, Attribution::Unrelated);
}

#[test]
fn a_child_that_started_after_its_parent_is_still_the_users_process() {
    // The refusal is only ever for a contradiction. The ordinary case — the CLI
    // starts, builds, then starts the application — must survive it, or the
    // feature this phase exists for would be gone.
    let found = attribute(
        &[
            process_at(8352, "dotnet", None, "2026-08-06T13:35:00.0000000Z"),
            process_at(9960, "Crasher", Some(8352), "2026-08-06T13:35:02.1230000Z"),
        ],
        &[("crash".to_string(), Some(8352))],
        &[config("crash", "Crasher")],
    );

    let child = entry(&found, 9960);
    assert_eq!(child.attribution, Attribution::Descendant);
    assert_eq!(child.config_id.as_deref(), Some("crash"));
    assert!(child.is_application);
}

#[test]
fn a_start_time_that_cannot_be_read_is_never_treated_as_a_contradiction() {
    // Unknown is not evidence. A time in a shape this cannot parse — a local
    // offset, a truncated field, a future runtime's format — must leave the link
    // exactly as it was, because refusing on it would throw away the real
    // ancestry of the process the user came here for.
    for (parent_time, child_time) in [
        ("not a time", "2026-08-06T13:35:02.0000000Z"),
        ("2026-08-06T13:35:00.0000000Z", "whenever"),
        ("2026-08-06T13:35:00+01:00", "2026-08-06T12:00:00+01:00"),
    ] {
        let found = attribute(
            &[
                process_at(8352, "dotnet", None, parent_time),
                process_at(9960, "Crasher", Some(8352), child_time),
            ],
            &[("crash".to_string(), Some(8352))],
            &[config("crash", "Crasher")],
        );

        assert_eq!(
            entry(&found, 9960).attribution,
            Attribution::Descendant,
            "({parent_time}, {child_time}) must not refuse the link"
        );
    }
}

#[test]
fn a_launched_process_that_is_the_application_is_the_one_offered() {
    // The plainest case, and the one a rule keyed on "is a descendant" got
    // wrong: with no launcher anywhere, the pid code-basics started is the
    // application and nothing else has to be worked out.
    let found = attribute(
        &[process(5000, "Api", None)],
        &[("api".to_string(), Some(5000))],
        &[config("api", "Api")],
    );

    assert!(found[0].is_application, "got {found:?}");
}

#[test]
fn a_process_code_basics_did_not_start_is_never_marked_as_an_application() {
    let found = attribute(&[process(700, "SomeoneElse", None)], &[], &[]);

    assert_eq!(found[0].attribution, Attribution::Unrelated);
    assert!(!found[0].is_application);
}

#[test]
fn a_live_capture_is_refused_when_the_pid_is_no_longer_one_of_ours() {
    // The picker is refreshed on demand, not continuously, so a pid chosen ten
    // minutes ago can belong to a process that has exited — and Windows hands
    // that number out again readily. The replacement is often another managed
    // process, which attaches happily and would be shown under the user's own
    // configuration name.
    let live = attribute(
        &[process(5000, "Api", None)],
        &[("api".to_string(), Some(5000))],
        &[config("api", "Api")],
    );

    assert!(live_target_reason(&InspectTarget::Live { pid: 5000 }, &live).is_none());

    let reason = live_target_reason(&InspectTarget::Live { pid: 5000 }, &[])
        .expect("a pid nothing is running under must not be attached to");
    assert!(reason.contains("5000"), "got {reason}");
    assert!(
        reason.contains("reused") || reason.contains("given that number"),
        "got {reason}"
    );
}

#[test]
fn a_dump_target_is_never_refused_for_not_being_a_running_process() {
    // A file on disk is whatever it was when it was written; there is no pid to
    // go stale.
    assert!(live_target_reason(
        &InspectTarget::Dump {
            path: "a.dmp".into()
        },
        &[]
    )
    .is_none());
}

// ---------------------------------------------------------------------------
// Attributing a dump to a run
// ---------------------------------------------------------------------------

fn dump(executable: &str, pid: u32, captured_at: u64) -> DumpFile {
    DumpFile {
        path: format!("{executable}_{pid}_{captured_at}.dmp").into(),
        executable: executable.to_string(),
        pid,
        captured_at,
        bytes: 1,
    }
}

#[test]
fn a_dump_carrying_the_runs_own_pid_is_the_only_certain_match() {
    let dumps = vec![dump("Worker.exe", 5200, 101), dump("Api.exe", 5100, 100)];

    let found = dump_for_run(&dumps, Some(5100), 90).expect("the run's own pid is evidence");
    assert!(found.certain);
    assert_eq!(found.dump.executable, "Api.exe");
}

#[test]
fn another_configurations_dump_is_never_claimed_as_this_runs_crash() {
    // Two applications up, both armed — the dump environment is inherited by
    // everything. Api crashed at 100, Worker at 101. Offering Worker's dump as
    // "Api crashed and a dump was captured" would attribute one process's
    // memory to another, which is what `session::caveats` explicitly refuses to
    // do. It may still be offered, but never as a certainty.
    let dumps = vec![dump("Worker.exe", 5200, 101), dump("Api.exe", 5100, 100)];

    let found = dump_for_run(&dumps, Some(5000), 90).expect("a candidate is still offered");
    assert!(
        !found.certain,
        "a dump that does not carry this run's pid is a candidate, not its crash"
    );
    assert_eq!(found.dump.executable, "Worker.exe");
}

#[test]
fn a_run_that_reported_no_pid_never_gets_a_certain_dump() {
    // A test run is the case: the crashing process is the test host, a
    // grandchild nobody here recorded. Everything it can offer is a candidate.
    let dumps = vec![dump("testhost.exe", 4000, 100)];

    let found = dump_for_run(&dumps, None, 90).expect("a candidate is still offered");
    assert!(!found.certain);
}

#[test]
fn a_dump_older_than_the_run_is_never_offered() {
    // Nothing written before the process existed can have come out of it — the
    // previous run's dump is the trap this closes.
    let dumps = vec![dump("Api.exe", 5100, 50)];

    assert_eq!(dump_for_run(&dumps, Some(5100), 90), None);
    assert_eq!(dump_for_run(&dumps, None, 90), None);
}

#[test]
fn a_run_with_no_dumps_on_disk_offers_nothing() {
    assert_eq!(dump_for_run(&[], Some(1), 0), None);
}

#[test]
fn the_cost_of_attaching_is_offered_beside_the_button_that_pays_it() {
    // `caveats` carries these too, but only the Objects tab shows that list —
    // and the Run tab's attach buttons commit to the snapshot before that tab
    // is ever rendered. They need the same sentences on their own.
    let dir = tempfile::tempdir().unwrap();
    let status = status(dir.path(), None);

    assert!(
        status
            .attach_caveats
            .iter()
            .any(|c| c.contains("copies its memory image")),
        "got {:?}",
        status.attach_caveats
    );
    for caveat in &status.attach_caveats {
        assert!(
            status.caveats.contains(caveat),
            "the tab that shows every caveat must still show this one: {caveat}"
        );
    }
}

// ---------------------------------------------------------------------------
// Retrying with the other architecture
// ---------------------------------------------------------------------------

fn failure(code: sidecar::FailureCode) -> sidecar::SidecarFailure {
    sidecar::SidecarFailure {
        code,
        message: "the sidecar's own sentence".to_string(),
    }
}

#[test]
fn a_bitness_mismatch_retries_with_the_build_that_can_read_the_target() {
    // The whole reason a second attempt exists: an x86 target reports a
    // mismatch to the x64 reader, and the x86 build is sitting right there.
    let outcome = AttemptOutcome::Failed(failure(sidecar::FailureCode::BitnessMismatch));

    assert_eq!(
        retry_bitness(&outcome, Bitness::X64, true),
        Some(Bitness::X86)
    );
}

#[test]
fn the_build_a_retry_would_use_is_never_the_one_that_just_failed() {
    // What the caller has to resolve on disk before the decision can be made.
    assert_eq!(other_bitness(Bitness::X64), Bitness::X86);
    assert_eq!(other_bitness(Bitness::X86), Bitness::X64);
}

#[test]
fn a_bitness_mismatch_with_no_other_build_present_surfaces_the_failure() {
    // Retrying with a binary that is not on disk would fail to resolve and
    // lose the sidecar's own mismatch message, which is the most useful thing
    // the user can be shown here.
    let outcome = AttemptOutcome::Failed(failure(sidecar::FailureCode::BitnessMismatch));

    assert_eq!(retry_bitness(&outcome, Bitness::X64, false), None);
}

#[test]
fn a_capture_that_worked_is_never_retried() {
    assert_eq!(
        retry_bitness(&AttemptOutcome::Captured, Bitness::X64, true),
        None
    );
    assert_eq!(
        retry_bitness(&AttemptOutcome::Captured, Bitness::X86, true),
        None
    );
}

#[test]
fn a_failure_that_is_not_a_mismatch_is_never_retried() {
    // Every one of these fails identically the second time: retrying doubles
    // the wait and buries the real message behind a duplicate of itself.
    for code in [
        sidecar::FailureCode::NotManaged,
        sidecar::FailureCode::AccessDenied,
        sidecar::FailureCode::TargetGone,
        sidecar::FailureCode::Other,
    ] {
        let outcome = AttemptOutcome::Failed(failure(code));
        assert_eq!(
            retry_bitness(&outcome, Bitness::X64, true),
            None,
            "{code:?} should not earn a second attempt"
        );
    }
}

#[test]
fn the_fallback_build_is_the_last_attempt() {
    // x86 is already the narrower reader; a mismatch reported by it has
    // nowhere left to go, and a loop that tried anyway would not terminate.
    let outcome = AttemptOutcome::Failed(failure(sidecar::FailureCode::BitnessMismatch));

    assert_eq!(retry_bitness(&outcome, Bitness::X86, true), None);
}

#[test]
fn a_result_that_was_never_written_is_not_retried() {
    // The sidecar died before answering. That is not a mismatch, and running
    // it again with the other architecture would only produce the same
    // silence — the parse error afterwards is what names the problem.
    assert_eq!(
        retry_bitness(&AttemptOutcome::NoResult, Bitness::X64, true),
        None
    );
}

#[test]
fn the_outcome_of_an_attempt_is_read_from_the_result_the_sidecar_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.json");
    let document = |extra: &str| {
        format!(
            r#"{{
                "schemaVersion": {},
                "snapshotId": "n1",
                "capturedAt": "2026-08-06T14:32:07Z",
                "target": {{ "target": {{ "kind": "live", "pid": 4242 }} }},
                "caps": {{ "maxDepth": 5, "maxChildren": 100, "maxStringLength": 512, "maxNodes": 5000 }},
                "nodes": []{extra}
            }}"#,
            crate::inspect::model::SCHEMA_VERSION
        )
    };

    // Nothing there at all.
    assert_eq!(attempt_outcome(&path), AttemptOutcome::NoResult);

    // Something there that this build cannot read is equally not a failure it
    // can act on: the code is what a retry is decided from, never the prose.
    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(attempt_outcome(&path), AttemptOutcome::NoResult);

    std::fs::write(&path, document("")).unwrap();
    assert_eq!(attempt_outcome(&path), AttemptOutcome::Captured);

    std::fs::write(
        &path,
        document(r#", "failure": "target is 32-bit", "failureCode": "bitnessMismatch""#),
    )
    .unwrap();
    match attempt_outcome(&path) {
        AttemptOutcome::Failed(f) => {
            assert_eq!(f.code, sidecar::FailureCode::BitnessMismatch);
            assert_eq!(f.message, "target is 32-bit");
        }
        other => panic!("expected a reported failure, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Enumeration: could not look is not the same as nothing there
// ---------------------------------------------------------------------------

#[test]
fn an_empty_machine_is_a_normal_answer() {
    // Nothing managed is running. That is data, and the picker shows it as an
    // empty list rather than as a problem.
    let listed = enumeration_outcome(Ok(()), || Ok(sidecar::ProcessList::default()));

    assert_eq!(listed, Ok(sidecar::ProcessList::default()));
}

#[test]
fn an_enumerator_that_could_not_be_run_never_reads_a_result_file() {
    // The gate this exists for. A stale listing from an earlier poll would be
    // read as this poll's answer, and `inspect_capture` refuses a live target
    // that is absent from the list — so a failure that read as data would tell
    // the user their process had exited and its pid may have been reused.
    let mut read = false;
    let listed = enumeration_outcome(Err("Access is denied. (os error 5)".to_string()), || {
        read = true;
        Ok(sidecar::ProcessList::default())
    });

    assert!(!read, "a failed enumeration must not fall back to a file");
    let reason = listed.expect_err("a failure must not read as an empty machine");
    assert!(reason.contains("could not be run"), "got {reason}");
    assert!(reason.contains("Access is denied"), "got {reason}");
}

#[test]
fn a_listing_that_could_not_be_read_is_a_failure_and_not_an_empty_machine() {
    let listed = enumeration_outcome(Ok(()), || {
        Err("the inspector did not produce a process list".to_string())
    });

    let reason = listed.expect_err("an unreadable listing is not an empty machine");
    assert!(reason.contains("did not produce"), "got {reason}");
}
