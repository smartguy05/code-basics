use super::*;

const WS: &str = r"C:\code\repo";

/// A registered instance with the pipe published and `WS` open: the connectable
/// shape.
fn open(pid: u32) -> EditorInstance {
    EditorInstance {
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

/// Running, but with a *different* repository open.
fn other_repo(pid: u32) -> EditorInstance {
    EditorInstance {
        workspaces: vec![format!(r"C:\code\other-{pid}")],
        ..open(pid)
    }
}

fn file(instances: Vec<EditorInstance>) -> InstancesFile {
    InstancesFile {
        version: 1,
        instances,
    }
}

fn all_alive(_: &EditorInstance) -> bool {
    true
}

fn none_alive(_: &EditorInstance) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Selection: the five refusals, each its own answer
// ---------------------------------------------------------------------------

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
    let probe = |instance: &EditorInstance| {
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
fn workspace_not_open_names_the_running_pids_but_not_their_repositories() {
    // What is open in a window the agent was not scoped to is not this
    // interface's to reveal.
    let error = choose_instance(&file(vec![other_repo(7)]), WS, None, &all_alive).unwrap_err();
    let sentence = error.sentence();
    assert!(sentence.contains('7'), "{sentence}");
    assert!(sentence.contains("repo"), "{sentence}"); // the requested workspace
    assert!(
        !sentence.contains("other-7"),
        "must not leak the other repo: {sentence}"
    );
}

#[test]
fn none_running_and_workspace_not_open_are_different_answers() {
    // One is a setting to change (start the app / open a repo). Collapsing them
    // is the failure this codebase refuses.
    let none = choose_instance(&file(vec![]), WS, None, &all_alive).unwrap_err();
    let not_open = choose_instance(&file(vec![other_repo(1)]), WS, None, &all_alive).unwrap_err();
    assert_ne!(none, not_open);
    assert_ne!(none.code(), not_open.code());
    assert_ne!(none.sentence(), not_open.sentence());
}

#[test]
fn a_protocol_this_build_does_not_speak_is_refused_before_the_workspace() {
    let mismatched = EditorInstance {
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

// ---------------------------------------------------------------------------
// Selection: what is chosen
// ---------------------------------------------------------------------------

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
fn a_dead_window_does_not_make_a_live_one_ambiguous() {
    let registered = file(vec![open(1), open(2)]);
    let only_two = |instance: &EditorInstance| instance.pid == 2;
    assert_eq!(
        choose_instance(&registered, WS, None, &only_two)
            .unwrap()
            .pid,
        2
    );
}

// ---------------------------------------------------------------------------
// The instance hint
// ---------------------------------------------------------------------------

#[test]
fn a_hint_selects_between_two_windows_with_the_workspace_open() {
    let registered = file(vec![open(100), open(200)]);
    let chosen = choose_instance(&registered, WS, Some("200"), &all_alive).unwrap();
    assert_eq!(chosen.pid, 200);
}

#[test]
fn a_hint_naming_nothing_is_none_running_and_says_which_hint_it_was() {
    let registered = file(vec![open(100)]);
    let error = choose_instance(&registered, WS, Some("999"), &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::NoneRunning {
            hint: Some("999".to_string())
        }
    );
    assert!(error.sentence().contains("999"), "{}", error.sentence());
}

#[test]
fn a_hint_naming_a_window_without_the_workspace_reports_that_not_ambiguity() {
    let registered = file(vec![other_repo(1), open(2)]);
    let error = choose_instance(&registered, WS, Some("1"), &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::WorkspaceNotOpen {
            workspace: WS.to_string(),
            running: vec![1]
        }
    );
}

#[test]
fn an_empty_hint_is_no_hint() {
    let registered = file(vec![open(3)]);
    assert_eq!(
        choose_instance(&registered, WS, Some(""), &all_alive)
            .unwrap()
            .pid,
        3
    );
}

// ---------------------------------------------------------------------------
// Every refusal is distinguishable
// ---------------------------------------------------------------------------

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
fn the_version_mismatch_refusal_reports_both_versions() {
    let sentence = InstanceError::VersionMismatch {
        pid: 1,
        protocol: 99,
    }
    .sentence();
    assert!(sentence.contains("99"), "{sentence}");
    assert!(
        sentence.contains(&PROTOCOL_VERSION.to_string()),
        "{sentence}"
    );
}

// ---------------------------------------------------------------------------
// The pipe name is DERIVED from the pid, never read from the file
// ---------------------------------------------------------------------------

#[test]
fn the_pipe_name_is_a_function_of_the_pid() {
    assert_eq!(r"\\.\pipe\code-basics.editor.1234", pipe_name(1234));
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
    let chosen =
        choose_instance(&registered, WS, None, &all_alive).expect("the honest case resolves");
    assert_eq!(1234, chosen.pid);
}

// ---------------------------------------------------------------------------
// The file: tolerant like every other user-global store here
// ---------------------------------------------------------------------------

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
    let listener = object["listener"].as_object().unwrap();
    let mut listener_keys: Vec<&str> = listener.keys().map(String::as_str).collect();
    listener_keys.sort_unstable();
    assert_eq!(listener_keys, vec!["pipe", "token"]);
}

#[test]
fn a_missing_file_loads_as_no_instances() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(&dir.path().join("nothing-here.json"));
    assert_eq!(loaded, InstancesFile::default());
    assert!(loaded.instances.is_empty());
}

#[test]
fn a_corrupt_file_loads_as_no_instances_rather_than_stopping_mcp_starting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("editor-instances.json");
    std::fs::write(&path, "{ this is not json").unwrap();
    assert!(load(&path).instances.is_empty());
}

#[test]
fn a_saved_file_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("editor-instances.json");
    let written = file(vec![open(1), other_repo(2)]);
    save(&path, &written).unwrap();
    assert_eq!(load(&path), written);
}

#[test]
fn saving_leaves_no_temp_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("editor-instances.json");
    save(&path, &file(vec![open(1)])).unwrap();
    let strays: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name != "editor-instances.json")
        .collect();
    assert!(strays.is_empty(), "{strays:?}");
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
fn the_path_is_overridable_the_way_every_other_user_global_store_is() {
    let previous = std::env::var_os("CB_EDITOR_INSTANCES_PATH");
    std::env::set_var("CB_EDITOR_INSTANCES_PATH", r"C:\tmp\somewhere.json");
    assert_eq!(
        instances_path(),
        std::path::PathBuf::from(r"C:\tmp\somewhere.json")
    );
    match previous {
        Some(value) => std::env::set_var("CB_EDITOR_INSTANCES_PATH", value),
        None => std::env::remove_var("CB_EDITOR_INSTANCES_PATH"),
    }
}
