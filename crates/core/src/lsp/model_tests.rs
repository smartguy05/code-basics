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
// Type hierarchy
// ---------------------------------------------------------------------------

#[test]
fn type_node_serialises_with_the_keys_the_ui_reads() {
    let node = TypeNode {
        name: "Order".into(),
        kind: SymbolKind::Class,
        detail: Some("Shop.Domain".into()),
        path: Some(PathBuf::from("src/order.cs")),
        label: "src/order.cs".into(),
        line: 12,
        character: 13,
    };
    let value = serde_json::to_value(&node).unwrap();
    assert_eq!(
        keys(&value),
        [
            "character",
            "detail",
            "kind",
            "label",
            "line",
            "name",
            "path"
        ]
    );
    assert_eq!(value["kind"], json!("class"));
}

#[test]
fn a_type_node_outside_the_workspace_carries_a_null_path_and_detail() {
    let node = TypeNode {
        name: "Entity".into(),
        kind: SymbolKind::Class,
        detail: None,
        path: None,
        label: "metadata:/Entity".into(),
        line: 1,
        character: 0,
    };
    let value = serde_json::to_value(&node).unwrap();
    present_and_null(&value, "path");
    present_and_null(&value, "detail");
}

#[test]
fn type_hierarchy_result_serialises_with_the_keys_the_ui_reads() {
    let result = TypeHierarchyResult {
        outcome: Availability::Ready,
        item: None,
        supertypes: vec![],
        subtypes: vec![],
        message: None,
        server: None,
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        keys(&value),
        [
            "item",
            "message",
            "outcome",
            "server",
            "subtypes",
            "supertypes"
        ]
    );
    // `item: None` when the caret was not on a type — a real answer, present and
    // null so the frontend can tell it from a forgotten field.
    present_and_null(&value, "item");
    present_and_null(&value, "message");
    present_and_null(&value, "server");
}

#[test]
fn a_type_hierarchy_that_could_not_be_asked_carries_no_item_and_a_reason() {
    let result = TypeHierarchyResult::unavailable(Availability::Unsupported, "no provider");
    let value = serde_json::to_value(&result).unwrap();
    present_and_null(&value, "item");
    assert_eq!(value["supertypes"], json!([]));
    assert_eq!(value["subtypes"], json!([]));
    assert!(value["message"].is_string());
}

#[test]
fn type_hierarchy_round_trips_through_json() {
    let result = TypeHierarchyResult {
        outcome: Availability::Ready,
        item: Some(TypeNode {
            name: "Order".into(),
            kind: SymbolKind::Class,
            detail: None,
            path: Some(PathBuf::from("a.cs")),
            label: "a.cs".into(),
            line: 1,
            character: 6,
        }),
        supertypes: vec![TypeNode {
            name: "Entity".into(),
            kind: SymbolKind::Class,
            detail: Some("Base".into()),
            path: None,
            label: "metadata:/Entity".into(),
            line: 1,
            character: 0,
        }],
        subtypes: vec![],
        message: Some("No subtypes: ...".into()),
        server: Some("csharp".into()),
    };
    let round =
        serde_json::from_value::<TypeHierarchyResult>(serde_json::to_value(&result).unwrap());
    assert_eq!(round.unwrap(), result);
}

// ---------------------------------------------------------------------------
// Overloads
// ---------------------------------------------------------------------------

#[test]
fn overload_result_serialises_with_the_keys_the_ui_reads() {
    let result = OverloadResult {
        outcome: Availability::Ready,
        signatures: vec![SignatureInfo {
            label: "Add(int a, int b)".into(),
            documentation: None,
            parameters: vec![ParameterInfo {
                label: "int a".into(),
                documentation: None,
            }],
        }],
        active_signature: Some(0),
        active_parameter: Some(1),
        message: None,
        server: Some("csharp".into()),
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        keys(&value),
        [
            "activeParameter",
            "activeSignature",
            "message",
            "outcome",
            "server",
            "signatures"
        ]
    );
    present_and_null(&value, "message");
    assert_eq!(
        keys(&value["signatures"][0]),
        ["documentation", "label", "parameters"]
    );
    assert_eq!(
        keys(&value["signatures"][0]["parameters"][0]),
        ["documentation", "label"]
    );
}

#[test]
fn an_overload_result_that_could_not_be_asked_carries_no_active_index() {
    // `activeSignature: None` is "the server did not say", never `0` — those are
    // different facts, the same rule as `total`.
    let result = OverloadResult::unavailable(Availability::Loading, "still loading");
    let value = serde_json::to_value(&result).unwrap();
    present_and_null(&value, "activeSignature");
    present_and_null(&value, "activeParameter");
    assert_eq!(value["signatures"], json!([]));
    present_and_null(&value, "server");
}

#[test]
fn overload_result_round_trips_through_json() {
    let result = OverloadResult {
        outcome: Availability::Ready,
        signatures: vec![SignatureInfo {
            label: "f(x)".into(),
            documentation: Some("does f".into()),
            parameters: vec![ParameterInfo {
                label: "x".into(),
                documentation: Some("the x".into()),
            }],
        }],
        active_signature: None,
        active_parameter: None,
        message: None,
        server: None,
    };
    let round = serde_json::from_value::<OverloadResult>(serde_json::to_value(&result).unwrap());
    assert_eq!(round.unwrap(), result);
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_severity_serialises_to_its_exact_string() {
    let pairs = [
        (DiagnosticSeverity::Error, "error"),
        (DiagnosticSeverity::Warning, "warning"),
        (DiagnosticSeverity::Information, "information"),
        (DiagnosticSeverity::Hint, "hint"),
    ];
    for (variant, text) in pairs {
        assert_eq!(serde_json::to_value(variant).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<DiagnosticSeverity>(json!(text)).unwrap(),
            variant
        );
    }
}

#[test]
fn diagnostic_severity_has_exactly_four_variants() {
    // An exhaustive match, so adding a variant fails to compile until `types.ts`
    // and this list are updated together.
    let spelling = |severity: DiagnosticSeverity| match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Information => "information",
        DiagnosticSeverity::Hint => "hint",
    };
    for severity in [
        DiagnosticSeverity::Error,
        DiagnosticSeverity::Warning,
        DiagnosticSeverity::Information,
        DiagnosticSeverity::Hint,
    ] {
        assert_eq!(
            serde_json::to_value(severity).unwrap(),
            json!(spelling(severity))
        );
    }
}

#[test]
fn diagnostic_row_serialises_with_the_keys_the_ui_reads() {
    let row = DiagnosticRow {
        severity: DiagnosticSeverity::Error,
        line: 12,
        character: 4,
        end_line: 12,
        end_character: 9,
        message: "cannot find `Foo`".into(),
        source: Some("rustc".into()),
        code: Some("E0425".into()),
    };
    let value = serde_json::to_value(&row).unwrap();
    assert_eq!(
        keys(&value),
        [
            "character",
            "code",
            "endCharacter",
            "endLine",
            "line",
            "message",
            "severity",
            "source"
        ]
    );
    assert_eq!(value["severity"], json!("error"));
}

#[test]
fn diagnostics_result_serialises_with_the_keys_the_ui_reads() {
    let result = DiagnosticsResult {
        outcome: Availability::Ready,
        diagnostics: vec![],
        message: None,
        server: Some("csharp".into()),
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        keys(&value),
        ["diagnostics", "message", "outcome", "server"]
    );
    present_and_null(&value, "message");
}

#[test]
fn a_diagnostics_result_that_could_not_be_asked_is_empty_with_a_reason() {
    let result = DiagnosticsResult::unavailable(Availability::Failed, "the server died");
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["diagnostics"], json!([]));
    present_and_null(&value, "server");
    assert!(value["message"].is_string());
}

#[test]
fn diagnostics_round_trip_through_json() {
    let result = DiagnosticsResult {
        outcome: Availability::Ready,
        diagnostics: vec![DiagnosticRow {
            severity: DiagnosticSeverity::Warning,
            line: 1,
            character: 0,
            end_line: 1,
            end_character: 3,
            message: "unused".into(),
            source: None,
            code: None,
        }],
        message: None,
        server: Some("rust".into()),
    };
    let round = serde_json::from_value::<DiagnosticsResult>(serde_json::to_value(&result).unwrap());
    assert_eq!(round.unwrap(), result);
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

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

#[test]
fn rename_result_serialises_with_the_keys_the_ui_reads() {
    let result = RenameResult {
        outcome: Availability::Ready,
        total: Some(3),
        written: vec![RenamedFile {
            path: PathBuf::from("src/order.rs"),
            edits: 2,
        }],
        buffers: vec![BufferEdits {
            path: PathBuf::from("src/main.rs"),
            edits: vec![RangeEdit {
                start_line: 12,
                start_character: 4,
                end_line: 12,
                end_character: 9,
                new_text: "Total".into(),
            }],
        }],
        failures: vec![],
        message: None,
        server: Some("rust".into()),
    };
    let value = serde_json::to_value(&result).unwrap();

    assert_eq!(
        keys(&value),
        ["buffers", "failures", "message", "outcome", "server", "total", "written"]
    );
    present_and_null(&value, "message");
    assert_eq!(value["written"][0]["path"], json!("src/order.rs"));
    assert_eq!(value["written"][0]["edits"], json!(2));
    assert_eq!(
        keys(&value["buffers"][0]["edits"][0]),
        [
            "endCharacter",
            "endLine",
            "newText",
            "startCharacter",
            "startLine"
        ]
    );
}

#[test]
fn a_rename_that_could_not_be_asked_carries_the_total_key_as_null() {
    // The `Some(0)` / `None` split, which for a rename is the difference between
    // "the symbol is used nowhere else" and "nobody could be asked".
    let result = RenameResult::unavailable(Availability::Unsupported, "no renameProvider");
    let value = serde_json::to_value(&result).unwrap();
    present_and_null(&value, "total");
    assert_eq!(value["written"], json!([]));
    assert_eq!(value["buffers"], json!([]));
    assert_eq!(value["failures"], json!([]));
    present_and_null(&value, "server");
}

#[test]
fn a_rename_failure_says_whether_the_file_is_beyond_recovery() {
    // The one genuinely unrecoverable state in the feature: a write failed, and
    // restoring the pre-image of an *earlier* write failed too. A boolean rather
    // than prose, because the UI has to escalate on it without parsing English.
    let failure = RenameFailure {
        path: PathBuf::from("src/a.rs"),
        detail: "the file is read-only".into(),
        unrecoverable: true,
    };
    let value = serde_json::to_value(&failure).unwrap();
    assert_eq!(keys(&value), ["detail", "path", "unrecoverable"]);
    assert_eq!(value["unrecoverable"], json!(true));
}

#[test]
fn prepare_rename_result_serialises_with_the_keys_the_ui_reads() {
    let result = PrepareRenameResult {
        outcome: Availability::Ready,
        renameable: true,
        start_line: Some(31),
        start_character: Some(22),
        end_line: Some(31),
        end_character: Some(28),
        placeholder: None,
        message: None,
        server: Some("csharp".into()),
    };
    let value = serde_json::to_value(&result).unwrap();

    assert_eq!(
        keys(&value),
        [
            "endCharacter",
            "endLine",
            "message",
            "outcome",
            "placeholder",
            "renameable",
            "server",
            "startCharacter",
            "startLine"
        ]
    );
    // Roslyn answers a bare range with no placeholder, so this is the live path
    // for C# and the prefill comes from the buffer.
    present_and_null(&value, "placeholder");
}

#[test]
fn a_symbol_that_cannot_be_renamed_is_ready_and_says_so_rather_than_failing() {
    // `prepareRename` answering `null` is a real answer about the caret, not a
    // failure of the server — so the outcome stays `ready` and `renameable` is
    // what carries the news. Collapsing it into `failed` would tell the user
    // their language server is broken when it is working perfectly.
    let result = PrepareRenameResult::not_renameable("csharp");
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["outcome"], json!("ready"));
    assert_eq!(value["renameable"], json!(false));
    for key in ["startLine", "startCharacter", "endLine", "endCharacter"] {
        present_and_null(&value, key);
    }
}

#[test]
fn a_prepare_rename_that_could_not_be_asked_is_not_renameable_and_carries_no_range() {
    let result = PrepareRenameResult::unavailable(Availability::Loading, "still loading");
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["renameable"], json!(false));
    present_and_null(&value, "startLine");
    assert!(value["message"].is_string());
}

#[test]
fn every_rename_type_round_trips_through_json() {
    let result = RenameResult {
        outcome: Availability::Failed,
        total: None,
        written: vec![RenamedFile {
            path: PathBuf::from("a.rs"),
            edits: 1,
        }],
        buffers: vec![BufferEdits {
            path: PathBuf::from("b.rs"),
            edits: vec![RangeEdit {
                start_line: 1,
                start_character: 0,
                end_line: 1,
                end_character: 1,
                new_text: String::new(),
            }],
        }],
        failures: vec![RenameFailure {
            path: PathBuf::from("c.rs"),
            detail: "boom".into(),
            unrecoverable: false,
        }],
        message: Some("something".into()),
        server: None,
    };
    let round = serde_json::from_value::<RenameResult>(serde_json::to_value(&result).unwrap());
    assert_eq!(round.unwrap(), result);

    let prepare = PrepareRenameResult {
        outcome: Availability::Ready,
        renameable: true,
        start_line: Some(1),
        start_character: Some(2),
        end_line: Some(1),
        end_character: Some(5),
        placeholder: Some("Foo".into()),
        message: None,
        server: Some("typescript".into()),
    };
    let round =
        serde_json::from_value::<PrepareRenameResult>(serde_json::to_value(&prepare).unwrap());
    assert_eq!(round.unwrap(), prepare);
}
