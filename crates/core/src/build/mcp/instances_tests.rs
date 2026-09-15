use super::*;

const WS: &str = r"C:\code\repo";

fn open(pid: u32) -> BuildInstance {
    BuildInstance {
        pid,
        exe: r"C:\apps\code-basics\cb-app.exe".to_string(),
        protocol: PROTOCOL_VERSION,
        listener: Listener {
            pipe: pipe_name(pid),
            token: "0".repeat(64),
        },
        workspaces: vec![WS.to_string()],
    }
}

fn other_repo(pid: u32) -> BuildInstance {
    BuildInstance {
        workspaces: vec![format!(r"C:\code\other-{pid}")],
        ..open(pid)
    }
}

fn file(instances: Vec<BuildInstance>) -> InstancesFile {
    InstancesFile {
        version: 1,
        instances,
    }
}

fn all_alive(_: &BuildInstance) -> bool {
    true
}

fn none_alive(_: &BuildInstance) -> bool {
    false
}

#[test]
fn an_empty_registry_is_none_running() {
    let error = choose_instance(&file(vec![]), WS, None, &all_alive).unwrap_err();
    assert_eq!(error, InstanceError::NoneRunning { hint: None });
}

#[test]
fn a_dead_pid_is_none_running_and_never_selected() {
    let error = choose_instance(&file(vec![open(4242)]), WS, None, &none_alive).unwrap_err();
    assert_eq!(error, InstanceError::NoneRunning { hint: None });
}

#[test]
fn the_liveness_probe_sees_the_whole_entry_so_a_recycled_pid_can_be_refused() {
    let registered = file(vec![open(1000)]);
    let seen: std::cell::RefCell<Vec<(u32, String)>> = std::cell::RefCell::new(Vec::new());
    let probe = |instance: &BuildInstance| {
        seen.borrow_mut().push((instance.pid, instance.exe.clone()));
        false
    };
    let error = choose_instance(&registered, WS, None, &probe).unwrap_err();
    assert_eq!(error, InstanceError::NoneRunning { hint: None });
    assert_eq!(
        seen.into_inner(),
        vec![(1000, r"C:\apps\code-basics\cb-app.exe".to_string())],
        "the probe must receive the exe, or a recycled pid is indistinguishable"
    );
}

#[test]
fn a_running_app_without_the_workspace_open_is_its_own_answer() {
    let error = choose_instance(&file(vec![other_repo(7)]), WS, None, &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::WorkspaceNotOpen {
            workspace: WS.to_string(),
            running: vec![7]
        }
    );
}

#[test]
fn none_running_and_workspace_not_open_are_different_answers() {
    let none = choose_instance(&file(vec![]), WS, None, &all_alive).unwrap_err();
    let not_open = choose_instance(&file(vec![other_repo(1)]), WS, None, &all_alive).unwrap_err();
    assert_ne!(none, not_open);
    assert_ne!(none.code(), not_open.code());
    assert_ne!(none.sentence(), not_open.sentence());
}

#[test]
fn a_protocol_this_build_does_not_speak_is_refused_before_the_workspace() {
    let mismatched = BuildInstance {
        protocol: PROTOCOL_VERSION + 5,
        ..open(11)
    };
    let error = choose_instance(&file(vec![mismatched]), WS, None, &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::VersionMismatch {
            pid: 11,
            protocol: PROTOCOL_VERSION + 5
        }
    );
}

#[test]
fn two_windows_with_the_workspace_open_are_refused_and_never_picked() {
    let error =
        choose_instance(&file(vec![open(100), open(200)]), WS, None, &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::Ambiguous {
            workspace: WS.to_string(),
            candidates: vec![100, 200]
        }
    );
}

#[test]
fn the_ambiguity_refusal_names_the_pids_and_the_way_out() {
    let error =
        choose_instance(&file(vec![open(100), open(200)]), WS, None, &all_alive).unwrap_err();
    let sentence = error.sentence();
    assert!(
        sentence.contains("100") && sentence.contains("200"),
        "{sentence}"
    );
    assert!(sentence.contains("--instance"), "{sentence}");
}

#[test]
fn the_one_window_with_the_workspace_is_chosen() {
    let registered = file(vec![open(55)]);
    let chosen = choose_instance(&registered, WS, None, &all_alive).unwrap();
    assert_eq!(chosen.pid, 55);
}

#[test]
fn a_window_with_another_repo_does_not_make_the_matching_one_ambiguous() {
    let registered = file(vec![other_repo(1), open(2), other_repo(3)]);
    let chosen = choose_instance(&registered, WS, None, &all_alive).unwrap();
    assert_eq!(chosen.pid, 2);
}

#[test]
fn the_workspace_matches_case_insensitively_and_across_separators() {
    let registered = file(vec![open(9)]);
    let chosen = choose_instance(&registered, r"c:/code/repo/", None, &all_alive).unwrap();
    assert_eq!(chosen.pid, 9);
}

#[test]
fn a_hint_selects_between_two_windows_with_the_workspace_open() {
    let registered = file(vec![open(100), open(200)]);
    let chosen = choose_instance(&registered, WS, Some("200"), &all_alive).unwrap();
    assert_eq!(chosen.pid, 200);
}

#[test]
fn a_hint_naming_nothing_is_none_running_and_says_which_hint_it_was() {
    let error = choose_instance(&file(vec![open(100)]), WS, Some("999"), &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::NoneRunning {
            hint: Some("999".to_string())
        }
    );
    assert!(error.sentence().contains("999"), "{}", error.sentence());
}

#[test]
fn an_empty_hint_is_no_hint() {
    assert_eq!(
        choose_instance(&file(vec![open(3)]), WS, Some(""), &all_alive)
            .unwrap()
            .pid,
        3
    );
}

#[test]
fn the_five_refusals_have_five_codes_and_five_sentences() {
    let errors = [
        InstanceError::NoneRunning { hint: None },
        InstanceError::WorkspaceNotOpen {
            workspace: WS.to_string(),
            running: vec![1],
        },
        InstanceError::Ambiguous {
            workspace: WS.to_string(),
            candidates: vec![1, 2],
        },
        InstanceError::VersionMismatch {
            pid: 1,
            protocol: 99,
        },
        InstanceError::PipeNameMismatch {
            pid: 1,
            stated: r"\\.\pipe\forged".to_string(),
        },
    ];
    let codes: std::collections::BTreeSet<&str> = errors.iter().map(|e| e.code()).collect();
    assert_eq!(codes.len(), 5, "a shared code would pass every other test");
    let sentences: std::collections::BTreeSet<String> =
        errors.iter().map(|e| e.sentence()).collect();
    assert_eq!(sentences.len(), 5);
    for error in &errors {
        assert!(
            !error.sentence().is_empty(),
            "{error:?} must explain itself"
        );
    }
}

#[test]
fn the_pipe_name_is_a_function_of_the_pid() {
    assert_eq!(r"\\.\pipe\code-basics.build.1234", pipe_name(1234));
    assert_ne!(pipe_name(1234), pipe_name(1235));
}

#[test]
fn a_registry_naming_a_pipe_the_pid_does_not_imply_is_refused_as_tampering() {
    let mut instance = open(1234);
    instance.listener = Listener {
        pipe: r"\\.\pipe\attacker-owned".to_string(),
        token: "t".to_string(),
    };
    match choose_instance(&file(vec![instance]), WS, None, &all_alive) {
        Err(InstanceError::PipeNameMismatch { pid, stated }) => {
            assert_eq!(1234, pid);
            assert_eq!(r"\\.\pipe\attacker-owned", stated);
        }
        other => panic!("a forged pipe name must be refused, got {other:?}"),
    }
}

#[test]
fn a_registry_naming_the_derived_pipe_is_accepted() {
    let registered = file(vec![open(1234)]);
    let chosen = choose_instance(&registered, WS, None, &all_alive).expect("honest case");
    assert_eq!(1234, chosen.pid);
}

#[test]
fn the_wire_keys_are_camel_case() {
    let json = serde_json::to_value(open(5)).unwrap();
    let object = json.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["exe", "listener", "pid", "protocol", "workspaces"]
    );
}

#[test]
fn a_missing_file_loads_as_no_instances() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(&dir.path().join("nothing-here.json"));
    assert_eq!(loaded, InstancesFile::default());
}

#[test]
fn a_corrupt_file_loads_as_no_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("build-instances.json");
    std::fs::write(&path, "{ this is not json").unwrap();
    assert!(load(&path).instances.is_empty());
}

#[test]
fn a_saved_file_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("build-instances.json");
    let written = file(vec![open(1), other_repo(2)]);
    save(&path, &written).unwrap();
    assert_eq!(load(&path), written);
}

#[test]
fn upsert_replaces_the_entry_for_the_same_pid_rather_than_appending() {
    let mut registered = file(vec![other_repo(1)]);
    upsert(&mut registered, open(1));
    assert_eq!(registered.instances.len(), 1);
    assert!(registered.instances[0].has_workspace(WS));
}

#[test]
fn remove_drops_only_the_named_pid() {
    let mut registered = file(vec![open(1), open(2)]);
    remove(&mut registered, 1);
    assert_eq!(
        registered
            .instances
            .iter()
            .map(|i| i.pid)
            .collect::<Vec<_>>(),
        vec![2]
    );
}

#[test]
fn the_path_is_overridable() {
    let previous = std::env::var_os("CB_BUILD_INSTANCES_PATH");
    std::env::set_var("CB_BUILD_INSTANCES_PATH", r"C:\tmp\somewhere.json");
    assert_eq!(
        instances_path(),
        std::path::PathBuf::from(r"C:\tmp\somewhere.json")
    );
    match previous {
        Some(value) => std::env::set_var("CB_BUILD_INSTANCES_PATH", value),
        None => std::env::remove_var("CB_BUILD_INSTANCES_PATH"),
    }
}
