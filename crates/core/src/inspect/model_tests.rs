use super::*;

/// The exact JSON keys each type produces.
///
/// `src/ipc/types.ts` is written by hand against these names, so a rename on
/// the Rust side has to fail somewhere visible rather than turning into an
/// `undefined` in the tree view.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn node() -> InspectNode {
    InspectNode {
        id: "root._total".into(),
        label: "_total".into(),
        type_name: Some("System.Decimal".into()),
        value: ObjectValue::Primitive {
            text: "12450.00".into(),
        },
        children: Vec::new(),
        has_more: false,
        child_count_total: None,
    }
}

#[test]
fn inspect_node_serialises_with_the_keys_the_ui_reads() {
    assert_eq!(
        keys(&serde_json::to_value(node()).unwrap()),
        ["children", "hasMore", "id", "label", "typeName", "value"]
    );
}

#[test]
fn inspect_graph_serialises_with_the_keys_the_ui_reads() {
    let graph = InspectGraph {
        session_id: "s1".into(),
        snapshot_id: "n1".into(),
        captured_at: "2026-08-06T14:32:07Z".into(),
        target: TargetSummary {
            target: InspectTarget::Live { pid: 4242 },
            bitness: Some(Bitness::X64),
            runtime_version: Some("9.0.3".into()),
            process_name: Some("MyApi".into()),
        },
        roots: vec![node()],
        caps: Caps::default(),
        warnings: Vec::new(),
    };

    assert_eq!(
        keys(&serde_json::to_value(&graph).unwrap()),
        [
            "caps",
            "capturedAt",
            "roots",
            "sessionId",
            "snapshotId",
            "target"
        ]
    );
}

#[test]
fn target_and_caps_serialise_with_camel_case_keys() {
    let summary = TargetSummary {
        target: InspectTarget::Dump {
            path: "a.dmp".into(),
        },
        bitness: None,
        runtime_version: None,
        process_name: None,
    };
    assert_eq!(keys(&serde_json::to_value(&summary).unwrap()), ["target"]);

    assert_eq!(
        keys(&serde_json::to_value(Caps::default()).unwrap()),
        ["maxChildren", "maxDepth", "maxNodes", "maxStringLength"]
    );

    assert_eq!(
        keys(
            &serde_json::to_value(DumpFile {
                path: "a.dmp".into(),
                executable: "MyApi".into(),
                pid: 1,
                captured_at: 2,
                bytes: 3,
            })
            .unwrap()
        ),
        ["bytes", "capturedAt", "executable", "path", "pid"]
    );
}

#[test]
fn value_variants_are_tagged_by_kind_in_camel_case() {
    let tag = |v: &ObjectValue| {
        serde_json::to_value(v).unwrap()["kind"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert_eq!(tag(&ObjectValue::Null), "null");
    assert_eq!(
        tag(&ObjectValue::Primitive { text: "1".into() }),
        "primitive"
    );
    assert_eq!(
        tag(&ObjectValue::Text {
            text: "a".into(),
            truncated: false
        }),
        "text"
    );
    assert_eq!(
        tag(&ObjectValue::Unavailable { reason: "r".into() }),
        "unavailable"
    );
    assert_eq!(
        tag(&ObjectValue::Elided {
            reason: ElidedReason::DepthLimit
        }),
        "elided"
    );

    // Field names inside a variant are camelCase too — without
    // `rename_all_fields` these cross as snake_case and read as `undefined`.
    let reference = serde_json::to_value(ObjectValue::Reference {
        address: "0x7f6ad09421f8".into(),
        type_name: "Order".into(),
        expandable: true,
    })
    .unwrap();
    assert_eq!(
        keys(&reference),
        ["address", "expandable", "kind", "typeName"]
    );

    assert_eq!(
        serde_json::to_value(ElidedReason::ChildLimit).unwrap(),
        serde_json::json!("childLimit")
    );
    assert_eq!(
        serde_json::to_value(Bitness::X64).unwrap(),
        serde_json::json!("x64")
    );
}

#[test]
fn request_variants_serialise_in_camel_case() {
    let tag = |v: &RootSpec| {
        serde_json::to_value(v).unwrap()["kind"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert_eq!(tag(&RootSpec::Exceptions), "exceptions");
    assert_eq!(tag(&RootSpec::CrashException), "crashException");
    assert_eq!(
        tag(&RootSpec::Type {
            name: "Order".into(),
            limit: 20
        }),
        "type"
    );
    assert_eq!(
        tag(&RootSpec::Statics {
            name: "Cache".into()
        }),
        "statics"
    );

    let target = serde_json::to_value(InspectTarget::Live { pid: 7 }).unwrap();
    assert_eq!(target["kind"], "live");
    assert_eq!(target["pid"], 7);
}

/// Heap addresses cross as hexadecimal **strings**, never as numbers.
///
/// An address is a full 64-bit value. Today's Windows and Linux user-mode
/// addresses happen to sit below 2^47, comfortably inside what a JavaScript
/// number holds exactly — but that is an accident of current address-space
/// layout, not a guarantee, and it is not a thing to bet correctness on.
///
/// The bet matters because the address is not decoration: it is the identity
/// used to expand a node and to recognise a cycle. An address rounded on its
/// way through JSON would send an expansion to the wrong object and render a
/// stranger's fields under the user's variable name.
///
/// A string also happens to be what every other .NET debugging tool shows, so
/// what the user sees can be pasted straight into SOS or WinDbg.
#[test]
fn addresses_cross_as_hex_strings() {
    // A value past 2^53, to prove the representation is safe across the whole
    // range a `u64` permits rather than only the part currently in use.
    let address = "0x7ff6ad09421f8ab";
    assert!(
        u64::from_str_radix(address.trim_start_matches("0x"), 16).unwrap() > (1u64 << 53),
        "the test address must exceed what a JS number holds exactly"
    );

    let json = serde_json::to_value(ObjectValue::Reference {
        address: address.into(),
        type_name: "Order".into(),
        expandable: true,
    })
    .unwrap();

    assert!(
        json["address"].is_string(),
        "addresses must be strings, got {}",
        json["address"]
    );
    // Byte-identical, so what comes back is what the sidecar sent.
    assert_eq!(json["address"], address);

    let cycle = serde_json::to_value(ObjectValue::Cycle {
        address: address.into(),
        path: "root.customer".into(),
    })
    .unwrap();
    assert!(cycle["address"].is_string());
    assert_eq!(cycle["path"], "root.customer");
}

#[test]
fn an_attachable_process_serialises_with_the_keys_the_ui_reads() {
    let json = serde_json::to_value(AttachableProcess {
        pid: 4242,
        name: "Api".into(),
        path: Some("C:/src/api/bin/Api.exe".into()),
        attribution: Attribution::Descendant,
        config_id: Some("cfg".into()),
        config_name: Some("Api".into()),
        is_application: true,
        launcher_caveat: Some("this is the launcher".into()),
    })
    .unwrap();

    assert_eq!(
        keys(&json),
        [
            "attribution",
            "configId",
            "configName",
            "isApplication",
            "launcherCaveat",
            "name",
            "path",
            "pid"
        ]
    );
    // The evidence crosses as a camelCase string, not a number: the UI branches
    // on it to decide whether it may show a configuration name at all.
    assert_eq!(json["attribution"], "descendant");
    // Always present, never omitted: the view that preselects a capture target
    // reads it on every row, and an absent field would read as `undefined` —
    // falsy, and so indistinguishable from "no evidence" for a row that has it.
    assert_eq!(json["isApplication"], true);
}

#[test]
fn a_list_that_could_not_be_completed_carries_its_warnings() {
    // The parent of each process being unreadable degrades every attribution to
    // `unrelated`, which looks exactly like a machine running nothing of the
    // user's. The reason has to cross with the list.
    let json = serde_json::to_value(AttachableList {
        processes: Vec::new(),
        warnings: vec!["the parent of each process could not be read".into()],
    })
    .unwrap();

    assert_eq!(keys(&json), ["processes", "warnings"]);

    // Nothing to say is said by omission, so the mirror is `?`.
    let clean = serde_json::to_value(AttachableList::default()).unwrap();
    assert_eq!(keys(&clean), ["processes"]);
}

#[test]
fn an_unattributed_process_carries_no_configuration_at_all() {
    // Absent rather than null, so the TypeScript mirror is `?` and a view that
    // forgets to check cannot render an empty string where a name would be.
    let json = serde_json::to_value(AttachableProcess {
        pid: 4242,
        name: "SomeoneElse".into(),
        path: None,
        attribution: Attribution::Unrelated,
        config_id: None,
        config_name: None,
        is_application: false,
        launcher_caveat: None,
    })
    .unwrap();

    assert_eq!(keys(&json), ["attribution", "isApplication", "name", "pid"]);
    assert_eq!(json["isApplication"], false);
    assert_eq!(json["attribution"], "unrelated");
}

#[test]
fn a_dotnet_process_serialises_with_the_keys_the_sidecar_writes() {
    // The same shape is read back off the wire, so these names are a contract
    // with the sidecar as well as with the UI.
    let full = serde_json::to_value(DotnetProcess {
        pid: 9960,
        name: "Crasher".into(),
        path: Some("C:/src/Crasher/bin/Crasher.exe".into()),
        parent_pid: Some(8352),
        started_at: Some("2026-08-06T13:35:02.1230000Z".into()),
    })
    .unwrap();
    assert_eq!(
        keys(&full),
        ["name", "parentPid", "path", "pid", "startedAt"]
    );

    // Everything but the pid and the name is best effort: a process whose
    // details could not be read omits them rather than reporting a zero parent
    // or an empty path, either of which would be believed.
    let bare = serde_json::to_value(DotnetProcess {
        pid: 9960,
        name: "Crasher".into(),
        path: None,
        parent_pid: None,
        started_at: None,
    })
    .unwrap();
    assert_eq!(keys(&bare), ["name", "pid"]);
}

#[test]
fn a_run_dump_serialises_with_the_keys_the_ui_reads() {
    // `certain` is the field the whole affordance turns on: false means the UI
    // may offer the dump but must not say it is this run's.
    let json = serde_json::to_value(RunDump {
        dump: DumpFile {
            path: "Api.exe_5100_100.dmp".into(),
            executable: "Api.exe".into(),
            pid: 5100,
            captured_at: 100,
            bytes: 9,
        },
        certain: false,
    })
    .unwrap();

    assert_eq!(keys(&json), ["certain", "dump"]);
    assert_eq!(
        keys(&json["dump"]),
        ["bytes", "capturedAt", "executable", "path", "pid"]
    );
}

#[test]
fn a_default_inspector_config_stays_out_of_the_checked_in_file() {
    // `.code-basics/config.json` is checked in and shared with the team, so an
    // untouched inspector section must not appear in it at all.
    let json = serde_json::to_value(InspectorConfig::default()).unwrap();
    assert_eq!(keys(&json), ["captureDumps"]);
    assert_eq!(json["captureDumps"], false);
}

#[test]
fn dump_capture_is_off_unless_asked_for() {
    // The one setting that writes hundreds of megabytes of process memory to
    // disk must never default to on.
    assert!(!InspectorConfig::default().capture_dumps);
}

#[test]
fn a_request_carries_the_schema_version_and_does_not_suspend_by_default() {
    let request = InspectRequest::new(
        InspectTarget::Live { pid: 1 },
        RootSpec::Exceptions,
        Caps::default(),
    );

    assert_eq!(request.schema_version, SCHEMA_VERSION);
    // Suspending stops the user's application; it has to be asked for.
    assert!(!request.suspend);

    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(
        keys(&json),
        ["caps", "root", "schemaVersion", "suspend", "target"]
    );
}

#[test]
fn caps_bound_every_axis_a_graph_can_run_away_along() {
    let caps = Caps::default();
    assert!(caps.max_depth > 0);
    assert!(caps.max_children > 0);
    assert!(caps.max_string_length > 0);
    // A wide, shallow graph is bounded even when the depth cap never bites.
    assert!(caps.max_nodes > caps.max_children);
}

#[test]
fn only_references_and_elisions_offer_more() {
    assert!(ObjectValue::Elided {
        reason: ElidedReason::NodeLimit
    }
    .is_expandable());
    assert!(ObjectValue::Reference {
        address: "0x1".into(),
        type_name: "T".into(),
        expandable: true,
    }
    .is_expandable());

    // A cycle is a leaf: the object is already on screen somewhere else, and
    // expanding it again is what would recurse forever.
    assert!(!ObjectValue::Cycle {
        address: "0x1".into(),
        path: "root".into()
    }
    .is_expandable());
    assert!(!ObjectValue::Null.is_expandable());
    assert!(!ObjectValue::Unavailable { reason: "r".into() }.is_expandable());
    assert!(!ObjectValue::Primitive { text: "1".into() }.is_expandable());
}

// ---------------------------------------------------------------------------
// Widening for a re-read
// ---------------------------------------------------------------------------

#[test]
fn expanding_past_a_child_limit_asks_for_more_children_than_stopped_it() {
    // The whole point of the expand affordance. Re-reading the same object
    // under the limit that truncated it returns the identical first N children,
    // which is a fresh read reported as progress that did not happen.
    let caps = Caps::default();
    let widened = caps.widened(ElidedReason::ChildLimit);

    assert!(
        widened.max_children > caps.max_children,
        "{widened:?} did not get past {caps:?}"
    );
    // The node budget must not become the next thing to stop it at the same
    // place, which would look identical to no change at all.
    assert!(widened.max_nodes >= widened.max_children);
    // Nothing else moves: an expand is not a licence to walk the whole heap.
    assert_eq!(widened.max_depth, caps.max_depth);
    assert_eq!(widened.max_string_length, caps.max_string_length);
}

#[test]
fn expanding_past_a_depth_limit_asks_for_more_depth_than_stopped_it() {
    let caps = Caps::default();
    let widened = caps.widened(ElidedReason::DepthLimit);

    assert!(widened.max_depth > caps.max_depth);
    assert_eq!(widened.max_children, caps.max_children);
}

#[test]
fn expanding_past_a_node_limit_asks_for_more_nodes_than_stopped_it() {
    let caps = Caps::default();
    let widened = caps.widened(ElidedReason::NodeLimit);

    assert!(widened.max_nodes > caps.max_nodes);
}

#[test]
fn a_widened_capture_is_still_bounded() {
    // Repeated expansion must converge on a ceiling rather than on a request
    // to walk an entire heap.
    let mut caps = Caps::default();
    for _ in 0..40 {
        caps = caps
            .widened(ElidedReason::ChildLimit)
            .widened(ElidedReason::DepthLimit)
            .widened(ElidedReason::NodeLimit);
    }

    assert!(caps.max_children <= 10_000, "{caps:?}");
    assert!(caps.max_depth <= 32, "{caps:?}");
    assert!(caps.max_nodes <= 200_000, "{caps:?}");
}

#[test]
fn a_zero_cap_still_widens_to_something_readable() {
    // A workspace can write `"maxChildren": 0`; multiplying it would leave the
    // expand button permanently inert.
    let caps = Caps {
        max_children: 0,
        max_nodes: 0,
        ..Caps::default()
    };

    assert!(caps.widened(ElidedReason::ChildLimit).max_children > 0);
    assert!(caps.widened(ElidedReason::NodeLimit).max_nodes > 0);
}

#[test]
fn a_partly_written_caps_section_falls_back_key_by_key() {
    // `.code-basics/config.json` has to load for the workspace to open at all,
    // so a hand-written subset of the limits cannot be a parse failure.
    let caps: Caps = serde_json::from_str(r#"{"maxDepth":2}"#).unwrap();

    assert_eq!(caps.max_depth, 2);
    assert_eq!(caps.max_children, Caps::default().max_children);
    assert_eq!(caps.max_string_length, Caps::default().max_string_length);
    assert_eq!(caps.max_nodes, Caps::default().max_nodes);
}
