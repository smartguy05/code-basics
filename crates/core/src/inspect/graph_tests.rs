use super::*;

fn raw(kind: &str) -> RawNode {
    RawNode {
        id: "root".into(),
        parent: None,
        label: "field".into(),
        type_name: None,
        kind: kind.into(),
        text: None,
        address: None,
        path: None,
        reason: None,
        expandable: false,
        truncated: false,
        child_count_total: None,
    }
}

/// Every abstention carries a sentence a person can act on.
fn reason_of(value: &ObjectValue) -> &str {
    match value {
        ObjectValue::Unavailable { reason } => reason,
        other => panic!("expected an abstention, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The values that read cleanly
// ---------------------------------------------------------------------------

#[test]
fn well_formed_values_classify_as_themselves() {
    assert_eq!(classify(&raw("null")), ObjectValue::Null);

    let mut primitive = raw("primitive");
    primitive.text = Some("42".into());
    assert_eq!(
        classify(&primitive),
        ObjectValue::Primitive { text: "42".into() }
    );

    let mut text = raw("text");
    text.text = Some("hello".into());
    text.truncated = true;
    assert_eq!(
        classify(&text),
        ObjectValue::Text {
            text: "hello".into(),
            truncated: true
        }
    );

    let mut reference = raw("reference");
    reference.address = Some("0x7ff6ad09421f8".into());
    reference.type_name = Some("Order".into());
    reference.expandable = true;
    assert_eq!(
        classify(&reference),
        ObjectValue::Reference {
            address: "0x7ff6ad09421f8".into(),
            type_name: "Order".into(),
            expandable: true,
        }
    );

    let mut cycle = raw("cycle");
    cycle.address = Some("0x1".into());
    cycle.path = Some("root.customer".into());
    assert_eq!(
        classify(&cycle),
        ObjectValue::Cycle {
            address: "0x1".into(),
            path: "root.customer".into()
        }
    );
}

/// A dictionary entry the sidecar emits as a `pair` container is a value in its
/// own right — it groups a Key and a Value and has no address — so it must
/// classify as [`ObjectValue::Pair`] rather than falling through to the
/// unrecognised-kind abstention.
#[test]
fn a_dictionary_entry_classifies_as_a_pair() {
    assert_eq!(classify(&raw("pair")), ObjectValue::Pair);
}

#[test]
fn every_elision_reason_survives_the_round_trip() {
    // These three strings are the wire contract with the sidecar; if one is
    // renamed on either side the walk silently becomes "unreadable".
    for (wire, expected) in [
        ("depthLimit", ElidedReason::DepthLimit),
        ("childLimit", ElidedReason::ChildLimit),
        ("nodeLimit", ElidedReason::NodeLimit),
    ] {
        let mut node = raw("elided");
        node.reason = Some(wire.into());
        assert_eq!(
            classify(&node),
            ObjectValue::Elided { reason: expected },
            "`{wire}` should classify as {expected:?}"
        );
    }
}

#[test]
fn an_explicit_abstention_keeps_the_sidecars_own_wording() {
    let mut node = raw("unavailable");
    node.reason = Some("the field was optimised into a register".into());

    assert_eq!(
        reason_of(&classify(&node)),
        "the field was optimised into a register"
    );
}

// ---------------------------------------------------------------------------
// Abstaining rather than guessing
// ---------------------------------------------------------------------------

/// The governing rule: a value the user believes and acts on is far worse than
/// a visible gap. Each of these could plausibly have been rendered as
/// something — an empty string, a zero, "(unknown)" — and each would send
/// someone to debug the wrong thing.
#[test]
fn a_value_missing_the_pieces_it_needs_is_never_invented() {
    // Says it holds a number, did not say which.
    assert!(matches!(
        classify(&raw("primitive")),
        ObjectValue::Unavailable { .. }
    ));
    // Says it holds a string, did not say which. Rendering "" here would look
    // exactly like a genuinely empty string.
    assert!(matches!(
        classify(&raw("text")),
        ObjectValue::Unavailable { .. }
    ));

    // A reference with no address cannot be expanded and has no identity to
    // match a cycle against.
    let mut no_address = raw("reference");
    no_address.type_name = Some("Order".into());
    assert!(reason_of(&classify(&no_address)).contains("no address"));

    let mut no_type = raw("reference");
    no_type.address = Some("0x1".into());
    assert!(reason_of(&classify(&no_type)).contains("no type"));

    // "Already shown above" with nowhere to jump to is not useful.
    let mut cycle = raw("cycle");
    cycle.address = Some("0x1".into());
    assert!(matches!(classify(&cycle), ObjectValue::Unavailable { .. }));

    // Stopped, but would not say which cap did it.
    assert!(matches!(
        classify(&raw("elided")),
        ObjectValue::Unavailable { .. }
    ));

    let mut bad_reason = raw("elided");
    bad_reason.reason = Some("becauseIFeltLikeIt".into());
    assert!(matches!(
        classify(&bad_reason),
        ObjectValue::Unavailable { .. }
    ));
}

#[test]
fn an_unrecognised_kind_abstains_and_names_what_it_saw() {
    // A newer sidecar sending a kind this build predates must not be rendered
    // as a guess — but the message has to be specific enough to diagnose.
    let value = classify(&raw("quantumSuperposition"));
    let reason = reason_of(&value);

    assert!(reason.contains("quantumSuperposition"), "got {reason}");
    assert!(reason.contains("unrecognised"), "got {reason}");
}

#[test]
fn a_blank_abstention_reason_still_explains_itself() {
    // An empty string would render as a value pane with nothing in it.
    let mut node = raw("unavailable");
    node.reason = Some("   ".into());
    assert!(!reason_of(&classify(&node)).trim().is_empty());

    assert!(!reason_of(&classify(&raw("unavailable"))).trim().is_empty());
}

// ---------------------------------------------------------------------------
// Backing fields
// ---------------------------------------------------------------------------

#[test]
fn auto_property_backing_fields_are_shown_as_the_property() {
    assert_eq!(display_label("<Total>k__BackingField"), "Total");
    assert_eq!(display_label("<IsActive>k__BackingField"), "IsActive");
}

/// The other half of the abstain rule, applied to names.
///
/// The compiler generates many mangled names, and only one of them is an auto
/// property. Relabelling `<GetAsync>d__7` as `GetAsync` would claim the object
/// has a field it does not have.
#[test]
fn other_compiler_generated_names_are_left_exactly_as_they_are() {
    for name in [
        "<>c__DisplayClass0_0",
        "<GetAsync>d__7",
        "<>9__0",
        "<>k__BackingField", // no property name to recover
        "<Total>k__BackingFieldExtra",
        "<Totalk__BackingField",
        "Total>k__BackingField",
        "_total",
        "Total",
        "",
    ] {
        assert_eq!(display_label(name), name, "`{name}` should be left alone");
    }
}

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

fn document(schema_version: u32) -> String {
    format!(
        r#"{{
            "schemaVersion": {schema_version},
            "snapshotId": "n1",
            "capturedAt": "2026-08-06T14:32:07Z",
            "target": {{ "target": {{ "kind": "live", "pid": 4242 }} }},
            "caps": {{ "maxDepth": 5, "maxChildren": 100, "maxStringLength": 512, "maxNodes": 5000 }},
            "nodes": []
        }}"#
    )
}

#[test]
fn a_minimal_document_parses_with_everything_optional_defaulted() {
    let result = parse(&document(super::super::model::SCHEMA_VERSION)).unwrap();

    assert_eq!(result.snapshot_id, "n1");
    assert!(result.nodes.is_empty());
    assert!(result.warnings.is_empty());
    assert!(result.failure.is_none());
}

#[test]
fn a_mismatched_schema_version_says_the_inspector_is_out_of_step() {
    let err = parse(&document(super::super::model::SCHEMA_VERSION + 1))
        .expect_err("a future schema version should be refused, not guessed at");
    let message = err.to_string();

    // Misreading a newer format is exactly how wrong values reach the user.
    assert!(message.contains("out of step"), "got {message}");
    assert!(message.contains("rebuild"), "got {message}");
}

#[test]
fn a_malformed_document_names_what_it_was_reading() {
    let err = parse("{ not json").expect_err("malformed input should error");
    assert!(err.to_string().contains("inspector"), "got {err}");
}

#[test]
fn a_node_missing_optional_fields_still_deserialises() {
    // The wire format is loose on purpose: one surprising node must cost that
    // node, not the other ninety-nine in the capture.
    let json = r#"{ "id": "root", "kind": "null" }"#;
    let node: RawNode = serde_json::from_str(json).unwrap();

    assert_eq!(node.id, "root");
    assert!(node.parent.is_none());
    assert!(!node.expandable);
    assert_eq!(classify(&node), ObjectValue::Null);
}

#[test]
fn a_capture_that_failed_still_reports_its_reason() {
    // The sidecar ran and explained itself. That is a successful exchange and
    // must not be thrown away as an error.
    let json = document(super::super::model::SCHEMA_VERSION).replace(
        r#""nodes": []"#,
        r#""nodes": [], "failure": "the process exited before it could be read""#,
    );

    let result = parse(&json).unwrap();
    assert_eq!(
        result.failure.as_deref(),
        Some("the process exited before it could be read")
    );
}
