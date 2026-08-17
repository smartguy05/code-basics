//! The wire shape of everything this subsystem sends the frontend.
//!
//! These tests are deliberately about **keys and strings**, not values. The
//! TypeScript in `src/ipc/types.ts` is written by hand against these names, so a
//! rename on the Rust side has to fail somewhere visible rather than surfacing
//! as `undefined` in a dropdown.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;
use crate::symbols::declarations::SymbolKind;

/// Every key of a JSON object, sorted.
fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// A key that is present and explicitly `null` — the thing the
/// no-`skip_serializing_if` rule exists to guarantee.
fn present_and_null(value: &Value, key: &str) {
    let object = value.as_object().expect("expected a JSON object");
    assert!(
        object.contains_key(key),
        "`{key}` must be present even when it has no value"
    );
    assert_eq!(
        object.get(key),
        Some(&Value::Null),
        "`{key}` must cross as an explicit null"
    );
}

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

#[test]
fn every_availability_variant_serialises_to_its_exact_string() {
    let pairs = [
        (Availability::NotConfigured, "notConfigured"),
        (Availability::Starting, "starting"),
        (Availability::Loading, "loading"),
        (Availability::Ready, "ready"),
        (Availability::Failed, "failed"),
        (Availability::Unsupported, "unsupported"),
    ];
    for (variant, text) in pairs {
        assert_eq!(serde_json::to_value(variant).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<Availability>(json!(text)).unwrap(),
            variant
        );
    }
}

#[test]
fn availability_has_exactly_six_variants() {
    // Five reasons there is no answer and one reason there is. Collapsing any
    // two of them is the failure this whole subsystem is arranged to prevent,
    // so adding or removing one must be a deliberate edit to this list.
    let all = [
        Availability::NotConfigured,
        Availability::Starting,
        Availability::Loading,
        Availability::Ready,
        Availability::Failed,
        Availability::Unsupported,
    ];
    let mut spellings: Vec<String> = all
        .iter()
        .map(|a| {
            serde_json::to_value(a)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    spellings.sort();
    spellings.dedup();
    assert_eq!(spellings.len(), 6);
}

// ---------------------------------------------------------------------------
// Usages
// ---------------------------------------------------------------------------

#[test]
fn usage_result_serialises_with_the_keys_the_ui_reads() {
    let result = UsageResult {
        outcome: Availability::Ready,
        total: Some(2),
        usages: vec![],
        truncated: false,
        message: None,
        server: None,
    };
    let value = serde_json::to_value(&result).unwrap();

    assert_eq!(
        keys(&value),
        [
            "message",
            "outcome",
            "server",
            "total",
            "truncated",
            "usages"
        ]
    );
    present_and_null(&value, "message");
    present_and_null(&value, "server");
}

#[test]
fn a_usage_result_with_no_count_still_carries_the_total_key_as_null() {
    // `total: None` is "there is no count", which must not be indistinguishable
    // from "the backend forgot to send one".
    let result = UsageResult {
        outcome: Availability::Loading,
        total: None,
        usages: vec![],
        truncated: false,
        message: Some("still loading".into()),
        server: Some("roslyn".into()),
    };
    let value = serde_json::to_value(&result).unwrap();
    present_and_null(&value, "total");
    assert_eq!(value["usages"], json!([]));
}

#[test]
fn usage_serialises_with_the_keys_the_ui_reads() {
    let usage = Usage {
        path: Some(PathBuf::from("src/a.cs")),
        label: "src/a.cs".into(),
        line: 12,
        snippet: "var x = Total;".into(),
        highlight: None,
    };
    let value = serde_json::to_value(&usage).unwrap();

    assert_eq!(
        keys(&value),
        ["highlight", "label", "line", "path", "snippet"]
    );
    present_and_null(&value, "highlight");
}

#[test]
fn a_usage_outside_the_workspace_carries_a_null_path_not_a_missing_one() {
    let usage = Usage {
        path: None,
        label: "source-generated:/Foo.g.cs".into(),
        line: 1,
        snippet: String::new(),
        highlight: None,
    };
    let value = serde_json::to_value(&usage).unwrap();
    present_and_null(&value, "path");
    assert_eq!(value["label"], json!("source-generated:/Foo.g.cs"));
}

#[test]
fn highlight_is_an_object_with_start_and_end_not_an_array() {
    let value = serde_json::to_value(Highlight { start: 4, end: 9 }).unwrap();
    assert!(
        value.is_object(),
        "a tuple struct would serialise as an array"
    );
    assert_eq!(value, json!({"start": 4, "end": 9}));
}

// ---------------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------------

#[test]
fn definition_result_serialises_with_the_keys_the_ui_reads() {
    let result = DefinitionResult {
        outcome: Availability::Ready,
        declarations: vec![],
        implementations: vec![],
        type_definitions: vec![],
        message: None,
    };
    let value = serde_json::to_value(&result).unwrap();

    assert_eq!(
        keys(&value),
        [
            "declarations",
            "implementations",
            "message",
            "outcome",
            "typeDefinitions"
        ]
    );
    present_and_null(&value, "message");
}

#[test]
fn target_serialises_with_the_keys_the_ui_reads() {
    let target = Target {
        path: Some(PathBuf::from("src/a.cs")),
        label: "src/a.cs".into(),
        line: 3,
        character: 17,
        snippet: "public int Total { get; }".into(),
        container: None,
    };
    let value = serde_json::to_value(&target).unwrap();

    assert_eq!(
        keys(&value),
        ["character", "container", "label", "line", "path", "snippet"]
    );
    present_and_null(&value, "container");
}

// ---------------------------------------------------------------------------
// Anchors
// ---------------------------------------------------------------------------

#[test]
fn anchor_result_serialises_with_the_keys_the_ui_reads() {
    let result = AnchorResult {
        outcome: Availability::Unsupported,
        anchors: vec![],
        message: None,
    };
    let value = serde_json::to_value(&result).unwrap();

    assert_eq!(keys(&value), ["anchors", "message", "outcome"]);
    present_and_null(&value, "message");
    assert_eq!(value["outcome"], json!("unsupported"));
}

#[test]
fn declaration_anchor_serialises_with_the_keys_the_ui_reads() {
    let anchor = DeclarationAnchor {
        id: "Order.Total#4:23".into(),
        name: "Total".into(),
        kind: SymbolKind::Function,
        line: 5,
        character: 23,
        selection_line: 5,
    };
    let value = serde_json::to_value(&anchor).unwrap();

    assert_eq!(
        keys(&value),
        ["character", "id", "kind", "line", "name", "selectionLine"]
    );
}

#[test]
fn symbol_kind_still_serialises_as_the_lower_case_strings_types_ts_mirrors() {
    // `types.ts` mirrors these by hand and the palette already ships them; a
    // rename here would silently drop every badge.
    //
    // The **`match` is the point.** This used to be a hand-written array, which
    // pins the spelling of every variant somebody remembered to list and says
    // nothing about a new one: adding `Property` to the enum would have left the
    // array compiling, this test passing, and `types.ts` unmirrored — a kind on
    // the wire that the frontend's union does not contain. An exhaustive match
    // fails to compile instead, which is the only mechanism in this repository
    // that can make a wire-type change *stop* somebody.
    let spelling = |kind: SymbolKind| match kind {
        SymbolKind::Function => "function",
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Interface => "interface",
        SymbolKind::Trait => "trait",
        SymbolKind::Type => "type",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Constant => "constant",
        SymbolKind::Property => "property",
        SymbolKind::Variable => "variable",
        SymbolKind::Other => "other",
    };
    for kind in [
        SymbolKind::Function,
        SymbolKind::Class,
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Interface,
        SymbolKind::Trait,
        SymbolKind::Type,
        SymbolKind::Namespace,
        SymbolKind::Constant,
        SymbolKind::Property,
        SymbolKind::Variable,
        SymbolKind::Other,
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(spelling(kind)));
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[test]
fn lsp_status_serialises_with_the_keys_the_ui_reads() {
    let status = LspStatus { servers: vec![] };
    let value = serde_json::to_value(&status).unwrap();
    assert_eq!(keys(&value), ["servers"]);
}

#[test]
fn server_status_serialises_with_the_keys_the_ui_reads() {
    let status = ServerStatus {
        id: "csharp".into(),
        language: "C#".into(),
        state: Availability::NotConfigured,
        detail: None,
        caveat: None,
        looked_for: vec!["Microsoft.CodeAnalysis.LanguageServer.exe".into()],
        hint: None,
    };
    let value = serde_json::to_value(&status).unwrap();

    assert_eq!(
        keys(&value),
        [
            "caveat",
            "detail",
            "hint",
            "id",
            "language",
            "lookedFor",
            "state"
        ]
    );
    present_and_null(&value, "detail");
    // Present and null rather than absent: a ready row and a ready-with-a-caveat
    // row differ only here, so the field the UI branches on must always exist.
    present_and_null(&value, "caveat");
    present_and_null(&value, "hint");
    assert_eq!(value["state"], json!("notConfigured"));
    assert_eq!(
        value["lookedFor"],
        json!(["Microsoft.CodeAnalysis.LanguageServer.exe"])
    );
}

// ---------------------------------------------------------------------------
// Round trips
// ---------------------------------------------------------------------------

#[test]
fn every_result_type_round_trips_through_json() {
    // `types.ts` is hand-written, so a caller may well send one of these back.
    let usage = UsageResult {
        outcome: Availability::Ready,
        total: Some(0),
        usages: vec![Usage {
            path: None,
            label: "metadata:/Foo".into(),
            line: 1,
            snippet: "x".into(),
            highlight: Some(Highlight { start: 0, end: 1 }),
        }],
        truncated: true,
        message: Some("capped".into()),
        server: Some("csharp".into()),
    };
    let round = serde_json::from_value::<UsageResult>(serde_json::to_value(&usage).unwrap());
    assert_eq!(round.unwrap(), usage);

    let definition = DefinitionResult {
        outcome: Availability::Failed,
        declarations: vec![Target {
            path: Some(PathBuf::from("a/b.rs")),
            label: "a/b.rs".into(),
            line: 2,
            character: 0,
            snippet: String::new(),
            container: Some("Order".into()),
        }],
        implementations: vec![],
        type_definitions: vec![],
        message: None,
    };
    let round =
        serde_json::from_value::<DefinitionResult>(serde_json::to_value(&definition).unwrap());
    assert_eq!(round.unwrap(), definition);

    let anchors = AnchorResult {
        outcome: Availability::Starting,
        anchors: vec![DeclarationAnchor {
            id: "id".into(),
            name: "n".into(),
            kind: SymbolKind::Class,
            line: 1,
            character: 6,
            selection_line: 1,
        }],
        message: None,
    };
    let round = serde_json::from_value::<AnchorResult>(serde_json::to_value(&anchors).unwrap());
    assert_eq!(round.unwrap(), anchors);

    let status = LspStatus {
        servers: vec![ServerStatus {
            id: "rust".into(),
            language: "Rust".into(),
            state: Availability::Ready,
            detail: Some("rust-analyzer 1.0".into()),
            caveat: Some("the index never finished priming".into()),
            looked_for: vec![],
            hint: None,
        }],
    };
    let round = serde_json::from_value::<LspStatus>(serde_json::to_value(&status).unwrap());
    assert_eq!(round.unwrap(), status);
}
