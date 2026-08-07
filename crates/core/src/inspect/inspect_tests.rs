use super::*;

const CRASH: &str = include_str!("../../fixtures/inspect/crash-exception.json");
/// Genuine output from the sidecar, recorded from a real crash dump of
/// `sidecar/fixtures/Crasher`. The hand-written fixtures pin the *contract*;
/// this one pins what the implementation actually emits, which is the half a
/// hand-written fixture can always drift away from.
const RECORDED: &str = include_str!("../../fixtures/inspect/recorded-crash.json");
const ATTACH_FAILED: &str = include_str!("../../fixtures/inspect/attach-failed.json");
const UNREADABLE: &str = include_str!("../../fixtures/inspect/unreadable.json");

/// Depth-first search for a node by id, so tests name what they mean rather
/// than indexing through the shape.
fn find<'a>(nodes: &'a [InspectNode], id: &str) -> &'a InspectNode {
    fn walk<'a>(nodes: &'a [InspectNode], id: &str) -> Option<&'a InspectNode> {
        for node in nodes {
            if node.id == id {
                return Some(node);
            }
            if let Some(found) = walk(&node.children, id) {
                return Some(found);
            }
        }
        None
    }
    walk(nodes, id).unwrap_or_else(|| panic!("no node with id `{id}`"))
}

// ---------------------------------------------------------------------------
// A real capture, end to end
// ---------------------------------------------------------------------------

#[test]
fn a_crash_capture_becomes_a_tree_the_ui_can_render() {
    let graph = parse_result("session-1", CRASH).unwrap();

    assert_eq!(graph.session_id, "session-1");
    assert_eq!(graph.snapshot_id, "snap-8f2c1a");
    assert_eq!(graph.captured_at, "2026-08-06T14:32:07.4180000Z");
    assert_eq!(graph.caps.max_children, 100);

    assert_eq!(graph.roots.len(), 1);
    let root = &graph.roots[0];
    assert_eq!(
        root.type_name.as_deref(),
        Some("System.NullReferenceException")
    );

    // The whole point: the message and the object that caused it, together.
    assert_eq!(
        find(&graph.roots, "root._message").value,
        ObjectValue::Text {
            text: "Object reference not set to an instance of an object.".into(),
            truncated: false,
        }
    );
    assert_eq!(
        find(&graph.roots, "root.quote.total").value,
        ObjectValue::Primitive {
            text: "0.00".into()
        }
    );
}

#[test]
fn the_capture_identifies_what_it_read() {
    let graph = parse_result("s", CRASH).unwrap();

    assert_eq!(graph.target.bitness, Some(Bitness::X64));
    assert_eq!(graph.target.runtime_version.as_deref(), Some("9.0.3"));
    assert_eq!(graph.target.process_name.as_deref(), Some("Quotes.Api"));
    assert!(matches!(graph.target.target, InspectTarget::Dump { .. }));
}

#[test]
fn auto_properties_read_as_the_names_in_the_source() {
    let graph = parse_result("s", CRASH).unwrap();

    // The heap holds `<Total>k__BackingField`; a developer looking for `Total`
    // should find `Total`.
    assert_eq!(find(&graph.roots, "root.quote.total").label, "Total");
    assert_eq!(find(&graph.roots, "root.quote.currency").label, "Currency");
    assert_eq!(find(&graph.roots, "root.quote.legs[0].id").label, "Id");

    // Ordinary fields keep their real names — nothing is invented.
    assert_eq!(find(&graph.roots, "root._message").label, "_message");
    assert_eq!(
        find(&graph.roots, "root.quote._cachedRate").label,
        "_cachedRate"
    );
}

/// The structure that makes a naive object dumper hang forever.
#[test]
fn a_cycle_is_a_leaf_pointing_back_at_where_the_object_already_is() {
    let graph = parse_result("s", CRASH).unwrap();
    let back_reference = find(&graph.roots, "root.quote.legs[0].quote");

    let ObjectValue::Cycle { address, path } = &back_reference.value else {
        panic!("expected a cycle, got {:?}", back_reference.value);
    };

    // It names the same object as the one further up...
    let ObjectValue::Reference {
        address: original, ..
    } = &find(&graph.roots, "root.quote").value
    else {
        panic!("root.quote should be a reference");
    };
    assert_eq!(address, original, "the cycle must identify the same object");

    // ...and says where to find it, so the UI can offer to jump there.
    assert_eq!(path, "root.quote");
    assert!(
        back_reference.children.is_empty(),
        "a cycle must not recurse"
    );
}

#[test]
fn every_cap_is_visible_rather_than_a_shorter_list() {
    let graph = parse_result("s", CRASH).unwrap();

    // A 5,412-element list shows one element and says so.
    let legs = find(&graph.roots, "root.quote.legs");
    assert_eq!(legs.child_count_total, Some(5412));
    assert!(legs.has_more);

    assert_eq!(
        find(&graph.roots, "root.quote.legs[1]").value,
        ObjectValue::Elided {
            reason: ElidedReason::ChildLimit
        }
    );
    assert_eq!(
        find(&graph.roots, "root.quote.customer").value,
        ObjectValue::Elided {
            reason: ElidedReason::NodeLimit
        }
    );

    // A truncated string admits it, so nobody reads it as the whole value.
    let ObjectValue::Text { truncated, .. } = find(&graph.roots, "root._stackTraceString").value
    else {
        panic!("expected text");
    };
    assert!(truncated);
}

#[test]
fn a_field_that_could_not_be_read_says_so_instead_of_showing_a_number() {
    let graph = parse_result("s", CRASH).unwrap();

    // `_cachedRate` is a decimal. Rendering it as `0` — indistinguishable from
    // a real zero — is exactly the failure this design exists to prevent.
    let ObjectValue::Unavailable { reason } = &find(&graph.roots, "root.quote._cachedRate").value
    else {
        panic!("an unreadable field must not be given a value");
    };
    assert!(reason.contains("register"), "got {reason}");

    // A genuine null is still a null, and distinct from unreadable.
    assert_eq!(
        find(&graph.roots, "root._innerException").value,
        ObjectValue::Null
    );
    assert_eq!(
        find(&graph.roots, "root.quote.aircraft").value,
        ObjectValue::Null
    );
}

#[test]
fn warnings_from_the_sidecar_reach_the_caller() {
    let graph = parse_result("s", CRASH).unwrap();

    assert!(
        graph
            .warnings
            .iter()
            .any(|w| w.contains("could not be read")),
        "{:?}",
        graph.warnings
    );
}

// ---------------------------------------------------------------------------
// Captures that went wrong
// ---------------------------------------------------------------------------

#[test]
fn a_sidecar_that_could_not_attach_reports_its_own_reason() {
    // The sidecar ran and diagnosed the problem precisely. Replacing that with
    // a generic message would throw away the only useful part.
    let err = parse_result("s", ATTACH_FAILED).expect_err("a failed capture should error");
    let message = err.to_string();

    assert!(message.contains("32-bit"), "got {message}");
    assert!(message.contains("retry"), "got {message}");
}

#[test]
fn an_unreadable_capture_still_shows_what_it_could_read() {
    let graph = parse_result("s", UNREADABLE).unwrap();

    // One good value among the unreadable ones must survive.
    assert_eq!(
        find(&graph.roots, "root.attempts").value,
        ObjectValue::Primitive { text: "3".into() }
    );

    // And the warning explains the underlying cause, with the fix.
    assert!(
        graph
            .warnings
            .iter()
            .any(|w| w.contains("DOTNET_DbgMiniDumpType")),
        "{:?}",
        graph.warnings
    );
}

#[test]
fn each_kind_of_uncertainty_keeps_its_own_explanation() {
    let graph = parse_result("s", UNREADABLE).unwrap();

    let reason = |id: &str| match &find(&graph.roots, id).value {
        ObjectValue::Unavailable { reason } => reason.clone(),
        other => panic!("`{id}` should be unavailable, got {other:?}"),
    };

    assert!(reason("root.name").contains("Mini dump"));
    assert!(reason("root.pending").contains("not been forced"));
    // A kind from a newer sidecar than this build.
    assert!(reason("root.mystery").contains("somethingNewerThanThisBuild"));
    // A reference with no address cannot be expanded, so it is not a reference.
    assert!(reason("root.broken").contains("no address"));
}

// ---------------------------------------------------------------------------
// Reading from disk
// ---------------------------------------------------------------------------

#[test]
fn a_missing_result_file_explains_the_likely_cause() {
    let err = parse_result_file("s", std::path::Path::new("/nonexistent/result.json"))
        .expect_err("a missing result should error");
    let message = err.to_string();

    assert!(
        message.contains("did not produce a result"),
        "got {message}"
    );
    // The two things that actually cause this.
    assert!(message.contains("attach"), "got {message}");
    assert!(message.contains("exited"), "got {message}");
}

#[test]
fn a_result_on_disk_parses_the_same_as_one_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.json");
    std::fs::write(&path, CRASH).unwrap();

    let from_disk = parse_result_file("session-1", &path).unwrap();
    assert_eq!(from_disk, parse_result("session-1", CRASH).unwrap());
}

// ---------------------------------------------------------------------------
// Against what the sidecar really emits
// ---------------------------------------------------------------------------

/// The hand-written fixtures say what the format *should* be. This one is a
/// verbatim capture from the real sidecar reading a real crash dump, so the
/// two halves cannot quietly drift apart.
#[test]
fn a_recorded_capture_from_the_real_sidecar_parses() {
    let graph = parse_result("recorded", RECORDED).unwrap();

    assert_eq!(graph.roots.len(), 1, "one crashing thread");
    let root = &graph.roots[0];
    assert_eq!(
        root.type_name.as_deref(),
        Some("System.NullReferenceException")
    );
    assert!(graph.target.runtime_version.is_some());
    assert_eq!(graph.target.bitness, Some(Bitness::X64));
}

#[test]
fn the_recorded_capture_carries_the_message_and_the_frames() {
    let graph = parse_result("recorded", RECORDED).unwrap();
    let root = &graph.roots[0];

    let message = root
        .children
        .iter()
        .find(|c| c.label == "_message")
        .expect("an exception has a message");
    assert_eq!(
        message.value,
        ObjectValue::Text {
            text: "Object reference not set to an instance of an object.".into(),
            truncated: false,
        }
    );

    // `_stackTraceString` is null in a dump — the runtime fills it in lazily
    // when something reads `.StackTrace`, and the inspector never runs code.
    // The frames come from the runtime instead, and without them a capture
    // would look like a crash with no stack trace at all.
    let string_field = root
        .children
        .iter()
        .find(|c| c.label == "_stackTraceString");
    assert_eq!(string_field.map(|f| &f.value), Some(&ObjectValue::Null));

    let frames = root
        .children
        .iter()
        .find(|c| c.label == "stack trace")
        .expect("the frames should be attached even though the field is null");
    assert_eq!(frames.children.len(), 2);
    assert_eq!(
        frames.children[0].value,
        ObjectValue::Text {
            text: "Crasher.Program.Boom".into(),
            truncated: false
        }
    );
}

/// The CLR hangs several long byte arrays off every exception it creates.
/// Rendered one row per byte they consume the whole capture; `_watsonBuckets`
/// alone is 5,616 of them.
#[test]
fn byte_arrays_are_summarised_rather_than_listed() {
    let graph = parse_result("recorded", RECORDED).unwrap();
    let buckets = graph.roots[0]
        .children
        .iter()
        .find(|c| c.label == "_watsonBuckets")
        .expect("an exception carries watson buckets");

    let ObjectValue::Text { text, truncated } = &buckets.value else {
        panic!("a byte array should read as text, got {:?}", buckets.value);
    };
    assert!(text.starts_with("byte[5616]"), "got {text}");
    assert!(truncated, "5,616 bytes cannot fit in the string cap");
    assert!(
        buckets.children.is_empty(),
        "a summarised array must not also emit rows"
    );

    // The whole capture stays small enough to render.
    fn count(nodes: &[InspectNode]) -> usize {
        nodes.iter().map(|n| 1 + count(&n.children)).sum()
    }
    assert!(
        count(&graph.roots) < 200,
        "got {} nodes",
        count(&graph.roots)
    );
}

#[test]
fn the_recorded_capture_distinguishes_null_from_unreadable() {
    let graph = parse_result("recorded", RECORDED).unwrap();

    // Several of an exception's fields are genuinely null on a fresh throw.
    // They must read as null, not as something the inspector failed at.
    let nulls = graph.roots[0]
        .children
        .iter()
        .filter(|c| c.value == ObjectValue::Null)
        .count();
    assert!(nulls >= 3, "expected several genuine nulls, got {nulls}");

    assert!(
        graph.roots[0]
            .children
            .iter()
            .all(|c| !matches!(c.value, ObjectValue::Unavailable { .. })),
        "nothing on a plain exception should be unreadable in a heap dump"
    );
}

#[test]
fn the_whole_graph_survives_a_round_trip_through_json() {
    // The UI receives this as JSON, so anything that fails to serialise or
    // comes back different is a bug the frontend would see and not explain.
    let graph = parse_result("s", CRASH).unwrap();
    let json = serde_json::to_string(&graph).unwrap();
    let back: InspectGraph = serde_json::from_str(&json).unwrap();

    assert_eq!(graph, back);
}

// ---------------------------------------------------------------------------
// A real machine's processes, attributed
// ---------------------------------------------------------------------------

/// Genuine `--list-processes` output, recorded while `sidecar/fixtures/Crasher`
/// was running under `dotnet run`. The pids are the real ones and so is the
/// shape: the CLI at 11324 with the application at 17448 beneath it, beside
/// four processes nobody launched from here. Only the user name in the paths
/// was changed.
const PROCESSES: &str = include_str!("../../fixtures/inspect/process-list.json");

#[test]
fn a_real_listing_attributes_the_dotnet_run_child_and_nothing_else() {
    let list = sidecar::parse_process_list(PROCESSES).unwrap();
    assert!(list.warnings.is_empty(), "got {:?}", list.warnings);

    // What the supervisor would hold for `dotnet run`: the CLI, not the
    // application. This is the whole bug — 11324 is what was launched and
    // 17448 is where every one of the user's objects lives.
    let config = crate::model::RunConfig::new(
        "crasher",
        "Crasher",
        crate::model::RunKind::App,
        "dotnet",
        crate::model::ConfigSource::Detected,
    );
    let found = session::attribute(
        &list.processes,
        &[("crasher".to_string(), Some(11324))],
        std::slice::from_ref(&config),
    );

    let by_pid = |pid: u32| {
        found
            .iter()
            .find(|p| p.pid == pid)
            .unwrap_or_else(|| panic!("no entry for {pid} in {found:?}"))
    };

    assert_eq!(by_pid(17448).attribution, Attribution::Descendant);
    assert_eq!(by_pid(17448).config_name.as_deref(), Some("Crasher"));
    assert_eq!(by_pid(17448).launcher_caveat, None);

    // The launcher is still offered — it is a real process and refusing to
    // list it would be its own kind of lie — but it says what it is and where
    // to go instead.
    assert_eq!(by_pid(11324).attribution, Attribution::Launched);
    let caveat = by_pid(11324).launcher_caveat.as_deref().unwrap();
    assert!(caveat.contains("17448"), "got {caveat}");

    // A compiler server and a launcher tray application are on every developer
    // machine and belong to nobody's configuration.
    for pid in [28300, 18908] {
        assert_eq!(by_pid(pid).attribution, Attribution::Unrelated);
        assert_eq!(by_pid(pid).config_id, None);
    }

    // The user's own processes come first, whatever the machine's order.
    assert_eq!(
        found
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .first(),
        Some(&"Crasher")
    );

    // Our own enumerator found itself and is not offered as something to
    // inspect, even though this supervisor never started it.
    assert!(
        !found.iter().any(|p| p.name.starts_with("cb-inspector")),
        "got {found:?}"
    );
}
