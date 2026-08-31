//! Key-pinning for the debug wire types, and the state distinctions.
//! Included by `model.rs`.
//!
//! `src/ipc/types.ts` mirrors these by hand — there is no codegen — so these
//! tests are what stops the two drifting. See `docs/architecture/ipc-contract.md`.

use super::*;

fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------------
// The six states
// ---------------------------------------------------------------------------

#[test]
fn each_state_serialises_under_its_own_kind() {
    let cases = [
        (DebugState::NotRunning, "notRunning"),
        (DebugState::Starting, "starting"),
        (DebugState::Running, "running"),
    ];
    for (state, kind) in cases {
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["kind"], serde_json::json!(kind));
    }
}

#[test]
fn not_installed_carries_what_was_looked_for_and_how_to_fix_it() {
    // The one state the user can act on. A bare "unavailable" sends them to
    // the wrong place.
    let state = DebugState::NotInstalled {
        looked_for: vec!["netcoredbg on PATH".into()],
        hint: "install the C# extension".into(),
    };
    let json = serde_json::to_value(&state).unwrap();

    assert_eq!(keys(&json), ["hint", "kind", "lookedFor"]);
    assert_eq!(
        json["lookedFor"][0],
        serde_json::json!("netcoredbg on PATH")
    );
}

#[test]
fn paused_carries_the_thread_the_stack_will_be_asked_for() {
    let state = DebugState::Paused {
        reason: "breakpoint".into(),
        thread_id: Some(4),
        description: None,
    };
    let json = serde_json::to_value(&state).unwrap();

    assert_eq!(keys(&json), ["description", "kind", "reason", "threadId"]);
    assert_eq!(json["threadId"], serde_json::json!(4));
}

#[test]
fn an_absent_optional_is_null_and_not_a_missing_key() {
    // There is no `skip_serializing_if` in this module on purpose: an absent
    // key and a key holding null are different things to a TypeScript reader,
    // and every one of these is read to decide what to draw.
    let state = DebugState::Paused {
        reason: "step".into(),
        thread_id: None,
        description: None,
    };
    let json = serde_json::to_value(&state).unwrap();

    assert!(json.as_object().unwrap().contains_key("threadId"));
    assert_eq!(json["threadId"], serde_json::Value::Null);
    assert_eq!(json["description"], serde_json::Value::Null);
}

#[test]
fn exiting_without_a_code_is_not_exiting_with_zero() {
    let json = serde_json::to_value(DebugState::Exited { code: None }).unwrap();
    assert_eq!(json["code"], serde_json::Value::Null);

    let json = serde_json::to_value(DebugState::Exited { code: Some(0) }).unwrap();
    assert_eq!(json["code"], serde_json::json!(0));
}

#[test]
fn only_paused_licenses_a_call_stack() {
    assert!(DebugState::Paused {
        reason: "breakpoint".into(),
        thread_id: Some(1),
        description: None,
    }
    .is_paused());

    for state in [
        DebugState::NotRunning,
        DebugState::Starting,
        DebugState::Running,
        DebugState::Exited { code: Some(0) },
        DebugState::Failed {
            detail: "gone".into(),
        },
    ] {
        assert!(!state.is_paused(), "{state:?} must not license a stack");
    }
}

#[test]
fn the_three_live_states_are_the_stoppable_ones() {
    assert!(DebugState::Starting.is_live());
    assert!(DebugState::Running.is_live());
    assert!(DebugState::Paused {
        reason: "step".into(),
        thread_id: None,
        description: None,
    }
    .is_live());

    assert!(!DebugState::NotRunning.is_live());
    assert!(!DebugState::Exited { code: Some(0) }.is_live());
    assert!(!DebugState::Failed { detail: "x".into() }.is_live());
    assert!(!DebugState::NotInstalled {
        looked_for: Vec::new(),
        hint: String::new(),
    }
    .is_live());
}

#[test]
fn a_state_round_trips_through_the_wire() {
    let state = DebugState::Paused {
        reason: "exception".into(),
        thread_id: Some(9),
        description: Some("boom".into()),
    };
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<DebugState>(&json).unwrap(), state);
}

// ---------------------------------------------------------------------------
// The rest of the wire surface
// ---------------------------------------------------------------------------

#[test]
fn a_stack_frame_serialises_with_the_keys_the_ui_reads() {
    let frame = StackFrame {
        id: 1,
        name: "Main".into(),
        path: Some("src/Program.cs".into()),
        line: Some(12),
        column: None,
        subtle: false,
    };
    assert_eq!(
        keys(&serde_json::to_value(&frame).unwrap()),
        ["column", "id", "line", "name", "path", "subtle"]
    );
}

#[test]
fn a_frame_with_no_source_keeps_a_null_path_rather_than_an_empty_string() {
    // Framework, generated and native frames are real and common, and an empty
    // string is a path the UI would try to open.
    let frame = StackFrame {
        id: 2,
        name: "[External Code]".into(),
        path: None,
        line: None,
        column: None,
        subtle: true,
    };
    let json = serde_json::to_value(&frame).unwrap();
    assert_eq!(json["path"], serde_json::Value::Null);
    assert_eq!(json["subtle"], serde_json::json!(true));
}

#[test]
fn a_variable_serialises_with_the_keys_the_ui_reads() {
    let variable = Variable {
        name: "count".into(),
        value: "3".into(),
        type_name: Some("int".into()),
        variables_reference: 0,
    };
    assert_eq!(
        keys(&serde_json::to_value(&variable).unwrap()),
        ["name", "typeName", "value", "variablesReference"]
    );
}

#[test]
fn a_zero_reference_is_what_makes_a_variable_a_leaf() {
    let leaf = Variable {
        name: "n".into(),
        value: "1".into(),
        type_name: None,
        variables_reference: 0,
    };
    assert!(!leaf.is_expandable());

    let expandable = Variable {
        variables_reference: 41,
        ..leaf
    };
    assert!(expandable.is_expandable());
}

#[test]
fn a_scope_and_a_thread_serialise_with_their_keys() {
    let scope = Scope {
        name: "Locals".into(),
        variables_reference: 3,
        expensive: false,
    };
    assert_eq!(
        keys(&serde_json::to_value(&scope).unwrap()),
        ["expensive", "name", "variablesReference"]
    );

    let thread = Thread {
        id: 1,
        name: "Main Thread".into(),
    };
    assert_eq!(
        keys(&serde_json::to_value(&thread).unwrap()),
        ["id", "name"]
    );
}

#[test]
fn the_status_serialises_with_the_keys_the_ui_polls_for() {
    assert_eq!(
        keys(&serde_json::to_value(DebugStatus::idle()).unwrap()),
        ["configId", "stack", "state", "threads"]
    );
}

#[test]
fn the_idle_status_shows_nothing_because_there_is_nothing_to_show() {
    let status = DebugStatus::idle();
    assert_eq!(status.state, DebugState::NotRunning);
    assert!(status.stack.is_empty());
    assert!(status.threads.is_empty());
    assert_eq!(status.config_id, None);
}
