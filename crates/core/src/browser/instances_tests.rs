use super::*;

/// A registered instance with the panel open: the only shape that is
/// connectable.
fn open(pid: u32) -> BrowserInstance {
    BrowserInstance {
        pid,
        exe: r"C:\apps\code-basics\cb-app.exe".to_string(),
        protocol: PROTOCOL_VERSION,
        browser_feature: true,
        listener: Some(Listener {
            pipe: format!(r"\\.\pipe\code-basics.browser.{pid}"),
            token: "0".repeat(64),
        }),
        workspaces: vec![format!(r"C:\code\repo-{pid}")],
    }
}

fn plugin_off(pid: u32) -> BrowserInstance {
    BrowserInstance {
        browser_feature: false,
        listener: None,
        ..open(pid)
    }
}

fn panel_closed(pid: u32) -> BrowserInstance {
    BrowserInstance {
        listener: None,
        ..open(pid)
    }
}

fn file(instances: Vec<BrowserInstance>) -> InstancesFile {
    InstancesFile {
        version: 1,
        instances,
    }
}

fn all_alive(_: &BrowserInstance) -> bool {
    true
}

fn none_alive(_: &BrowserInstance) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Selection: the five refusals, each its own answer
// ---------------------------------------------------------------------------

#[test]
fn an_empty_registry_is_none_running() {
    let error = choose_instance(&file(vec![]), None, &all_alive).unwrap_err();
    assert_eq!(error, InstanceError::NoneRunning { hint: None });
}

#[test]
fn a_dead_pid_is_none_running_and_never_selected() {
    // The registry outlives the process that wrote it — a crash leaves the
    // entry behind — so liveness is re-probed and never read from the file.
    let error = choose_instance(&file(vec![open(4242)]), None, &none_alive).unwrap_err();
    assert_eq!(error, InstanceError::NoneRunning { hint: None });
}

#[test]
fn the_liveness_probe_sees_the_whole_entry_so_a_recycled_pid_can_be_refused() {
    // The reasoning `inspect::session` uses before attaching: a pid alone is
    // not identity. The probe is handed the entry so it can compare the
    // executable path, and a probe that rejects on the exe must be able to.
    let registered = file(vec![open(1000)]);
    let seen: std::cell::RefCell<Vec<(u32, String)>> = std::cell::RefCell::new(Vec::new());
    let probe = |instance: &BrowserInstance| {
        seen.borrow_mut().push((instance.pid, instance.exe.clone()));
        false
    };
    let error = choose_instance(&registered, None, &probe).unwrap_err();
    assert_eq!(error, InstanceError::NoneRunning { hint: None });
    assert_eq!(
        seen.into_inner(),
        vec![(1000, r"C:\apps\code-basics\cb-app.exe".to_string())],
        "the probe must receive the exe, or a recycled pid is indistinguishable"
    );
}

#[test]
fn a_running_app_with_the_plugin_switched_off_is_its_own_answer() {
    let error = choose_instance(&file(vec![plugin_off(7)]), None, &all_alive).unwrap_err();
    assert_eq!(error, InstanceError::PluginDisabled { pid: 7 });
}

#[test]
fn a_running_app_with_the_panel_closed_is_its_own_answer() {
    let error = choose_instance(&file(vec![panel_closed(9)]), None, &all_alive).unwrap_err();
    assert_eq!(error, InstanceError::PanelClosed { pid: 9 });
}

#[test]
fn plugin_off_and_panel_closed_are_not_the_same_answer() {
    // One is a setting to change, the other is one click away. Collapsing them
    // is the failure this codebase refuses.
    let off = choose_instance(&file(vec![plugin_off(1)]), None, &all_alive).unwrap_err();
    let closed = choose_instance(&file(vec![panel_closed(1)]), None, &all_alive).unwrap_err();
    assert_ne!(off, closed);
    assert_ne!(off.code(), closed.code());
    assert_ne!(off.sentence(), closed.sentence());
}

#[test]
fn a_protocol_this_build_does_not_speak_is_refused_before_anything_else() {
    let mismatched = BrowserInstance {
        protocol: PROTOCOL_VERSION + 5,
        ..open(11)
    };
    let error = choose_instance(&file(vec![mismatched]), None, &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::VersionMismatch {
            pid: 11,
            protocol: PROTOCOL_VERSION + 5
        }
    );
}

#[test]
fn two_open_panels_are_refused_and_never_picked() {
    // Driving a browser in a window the user is not looking at is worse than
    // asking. There is deliberately no "most recent" tie-break.
    let error = choose_instance(&file(vec![open(100), open(200)]), None, &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::Ambiguous {
            candidates: vec![100, 200]
        }
    );
}

#[test]
fn the_ambiguity_refusal_names_the_pids_and_the_way_out() {
    let error = choose_instance(&file(vec![open(100), open(200)]), None, &all_alive).unwrap_err();
    let sentence = error.sentence();
    assert!(
        sentence.contains("100") && sentence.contains("200"),
        "{sentence}"
    );
    assert!(
        sentence.contains("--instance"),
        "the refusal must name the way out: {sentence}"
    );
}

// ---------------------------------------------------------------------------
// Selection: what is chosen
// ---------------------------------------------------------------------------

#[test]
fn the_one_open_panel_is_chosen() {
    let registered = file(vec![open(55)]);
    let chosen = choose_instance(&registered, None, &all_alive).unwrap();
    assert_eq!(chosen.pid, 55);
}

#[test]
fn an_instance_with_no_panel_does_not_make_the_open_one_ambiguous() {
    // A second window with its panel closed is not a candidate at all, so this
    // must not become "ask the user which one".
    let registered = file(vec![panel_closed(1), open(2), plugin_off(3)]);
    let chosen = choose_instance(&registered, None, &all_alive).unwrap();
    assert_eq!(chosen.pid, 2);
}

#[test]
fn a_dead_instance_does_not_make_a_live_one_ambiguous() {
    let registered = file(vec![open(1), open(2)]);
    let only_two = |instance: &BrowserInstance| instance.pid == 2;
    assert_eq!(
        choose_instance(&registered, None, &only_two).unwrap().pid,
        2
    );
}

// ---------------------------------------------------------------------------
// The hint
// ---------------------------------------------------------------------------

#[test]
fn a_hint_selects_between_two_open_panels() {
    let registered = file(vec![open(100), open(200)]);
    let chosen = choose_instance(&registered, Some("200"), &all_alive).unwrap();
    assert_eq!(chosen.pid, 200);
}

#[test]
fn a_hint_naming_nothing_is_none_running_and_says_which_hint_it_was() {
    let registered = file(vec![open(100)]);
    let error = choose_instance(&registered, Some("999"), &all_alive).unwrap_err();
    assert_eq!(
        error,
        InstanceError::NoneRunning {
            hint: Some("999".to_string())
        }
    );
    assert!(
        error.sentence().contains("999"),
        "a hint that matched nothing must be quoted back: {}",
        error.sentence()
    );
}

#[test]
fn a_hint_naming_a_disabled_instance_reports_that_and_not_ambiguity() {
    // The hint narrows *before* the state checks, so the refusal describes the
    // instance the caller actually named.
    let registered = file(vec![plugin_off(1), open(2)]);
    let error = choose_instance(&registered, Some("1"), &all_alive).unwrap_err();
    assert_eq!(error, InstanceError::PluginDisabled { pid: 1 });
}

#[test]
fn an_empty_hint_is_no_hint() {
    let registered = file(vec![open(3)]);
    assert_eq!(
        choose_instance(&registered, Some(""), &all_alive)
            .unwrap()
            .pid,
        3
    );
}

// ---------------------------------------------------------------------------
// Every refusal is distinguishable and actionable
// ---------------------------------------------------------------------------

#[test]
fn the_five_refusals_have_five_codes_and_five_sentences() {
    let errors = [
        InstanceError::NoneRunning { hint: None },
        InstanceError::PluginDisabled { pid: 1 },
        InstanceError::PanelClosed { pid: 1 },
        InstanceError::Ambiguous {
            candidates: vec![1, 2],
        },
        InstanceError::VersionMismatch {
            pid: 1,
            protocol: 99,
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
fn the_plugin_disabled_refusal_names_the_control_the_user_would_click() {
    let sentence = InstanceError::PluginDisabled { pid: 1 }.sentence();
    assert!(
        sentence.contains("Plugins") || sentence.contains("Optional features"),
        "{sentence}"
    );
}

#[test]
fn the_panel_closed_refusal_names_both_ways_of_getting_there() {
    // Two causes reach this variant - a closed panel, and an application that
    // could not create the pipe - and the registry cannot tell them apart. A
    // user told to "open the panel" while looking at an open one has been sent
    // to do something they cannot do, so both are stated.
    let sentence = InstanceError::PanelClosed { pid: 9 }.sentence();
    assert!(sentence.contains("Plugins"), "{sentence}");
    assert!(sentence.contains("already open"), "{sentence}");
    assert!(sentence.contains("could not create the pipe"), "{sentence}");
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
        vec![
            "browserFeature",
            "exe",
            "listener",
            "pid",
            "protocol",
            "workspaces"
        ]
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
    let path = dir.path().join("browser-instances.json");
    std::fs::write(&path, "{ this is not json").unwrap();
    assert!(load(&path).instances.is_empty());
}

#[test]
fn a_saved_file_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("browser-instances.json");
    let written = file(vec![open(1), plugin_off(2)]);
    save(&path, &written).unwrap();
    assert_eq!(load(&path), written);
}

#[test]
fn saving_leaves_no_temp_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("browser-instances.json");
    save(&path, &file(vec![open(1)])).unwrap();
    let strays: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name != "browser-instances.json")
        .collect();
    assert!(strays.is_empty(), "{strays:?}");
}

#[test]
fn upsert_replaces_the_entry_for_the_same_pid_rather_than_appending() {
    // A pid is reused across restarts, and two entries for one pid would make
    // `Ambiguous` fire against a single window.
    let mut registered = file(vec![panel_closed(1)]);
    upsert(&mut registered, open(1));
    assert_eq!(registered.instances.len(), 1);
    assert!(registered.instances[0].listener.is_some());
}

#[test]
fn upsert_keeps_other_instances() {
    let mut registered = file(vec![open(1)]);
    upsert(&mut registered, open(2));
    assert_eq!(registered.instances.len(), 2);
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
    // Same escape hatch as CB_NOTES_PATH / CB_LAUNCHERS_PATH, and the only
    // reason the pipe and the shim are drivable by hand at all.
    let previous = std::env::var_os("CB_BROWSER_INSTANCES_PATH");
    std::env::set_var("CB_BROWSER_INSTANCES_PATH", r"C:\tmp\somewhere.json");
    assert_eq!(instances_path(), PathBuf::from(r"C:\tmp\somewhere.json"));
    match previous {
        Some(value) => std::env::set_var("CB_BROWSER_INSTANCES_PATH", value),
        None => std::env::remove_var("CB_BROWSER_INSTANCES_PATH"),
    }
}

// ---------------------------------------------------------------------------
// The pipe name is DERIVED from the pid, never read from the file.
//
// `Listener.pipe` used to be the name the client opened, and its doc comment
// justified that ("carried rather than recomputed so the name lives in exactly
// one place"). But the registry is an ordinary file under the user's config
// directory, so any local process that can write it can point the agent's shim
// at a pipe of its own and answer *as* the browser — serving fabricated page
// text, console lines and network rows into the agent's transcript, with no
// user click anywhere. The consent model cannot help: consent is enforced by
// the application, and on this path the application is never reached.
//
// Deriving from the pid closes it, because the pid is already verified against
// `exe` for recycled-pid safety. A file whose stated name disagrees with the
// derived one is evidence of tampering, so it is refused and named rather than
// silently corrected.
// ---------------------------------------------------------------------------

#[test]
fn the_pipe_name_is_a_function_of_the_pid() {
    assert_eq!(
        r"\\.\pipe\code-basics.browser.1234",
        pipe_name(1234),
        "the shape the application creates and the client opens must be one rule"
    );
    assert_ne!(pipe_name(1234), pipe_name(1235));
}

#[test]
fn a_registry_naming_a_pipe_the_pid_does_not_imply_is_refused_as_tampering() {
    let mut instance = open(1234);
    instance.listener = Some(Listener {
        // The attack: a pipe this process owns, published under the app's pid.
        pipe: r"\\.\pipe\attacker-owned".to_string(),
        token: "t".to_string(),
    });
    let file = file(vec![instance]);

    match choose_instance(&file, None, &all_alive) {
        Err(InstanceError::PipeNameMismatch { pid, stated }) => {
            assert_eq!(1234, pid);
            assert_eq!(r"\\.\pipe\attacker-owned", stated);
        }
        other => panic!("a forged pipe name must be refused, got {other:?}"),
    }
}

#[test]
fn a_registry_naming_the_derived_pipe_is_accepted() {
    // The other half: the honest case must keep working, or the refusal above
    // would simply disable the feature.
    let mut instance = open(1234);
    instance.listener = Some(Listener {
        pipe: pipe_name(1234),
        token: "t".to_string(),
    });
    let file = file(vec![instance]);

    let chosen = choose_instance(&file, None, &all_alive).expect("the honest case resolves");
    assert_eq!(1234, chosen.pid);
}

#[test]
fn the_refusal_names_the_file_so_a_tampered_registry_is_diagnosable() {
    let refusal = InstanceError::PipeNameMismatch {
        pid: 1234,
        stated: r"\\.\pipe\attacker-owned".to_string(),
    }
    .sentence();
    assert!(refusal.contains("1234"), "{refusal}");
    assert!(refusal.contains("attacker-owned"), "{refusal}");
}
