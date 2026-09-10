use super::*;

use crate::lsp::model::{ParameterInfo, SignatureInfo, Usage};
use crate::symbols::declarations::SymbolKind;
use std::path::PathBuf;

fn usage(label: &str, line: u32, snippet: &str) -> Usage {
    Usage {
        path: Some(PathBuf::from(label)),
        label: label.to_string(),
        line,
        snippet: snippet.to_string(),
        highlight: None,
    }
}

fn type_node(name: &str, kind: SymbolKind, label: &str, line: u32) -> TypeNode {
    TypeNode {
        name: name.to_string(),
        kind,
        detail: None,
        path: Some(PathBuf::from(label)),
        label: label.to_string(),
        line,
        character: 0,
    }
}

// ---------------------------------------------------------------------------
// Non-Ready outcomes become refusals with the right code
// ---------------------------------------------------------------------------

#[test]
fn every_non_ready_find_references_outcome_is_a_refusal_with_its_own_code() {
    let cases = [
        (Availability::NotConfigured, "notConfigured"),
        (Availability::Starting, "serverStarting"),
        (Availability::Loading, "serverLoading"),
        (Availability::Failed, "serverFailed"),
        (Availability::Unsupported, "unsupported"),
    ];
    for (availability, code) in cases {
        let answer = find_references(&UsageResult::unavailable(availability, "raw reason"));
        assert!(!answer.ok, "{availability:?}");
        assert_eq!(answer.code.as_deref(), Some(code), "{availability:?}");
        // The result's curated message is preferred over the generic sentence —
        // except for Failed, whose message is the raw language-server error text
        // and must not be forwarded (see
        // a_failed_outcome_does_not_forward_raw_server_error_text).
        if availability == Availability::Failed {
            assert!(!answer.text.contains("raw reason"), "{}", answer.text);
        } else {
            assert!(answer.text.contains("raw reason"), "{}", answer.text);
        }
    }
}

#[test]
fn a_failed_outcome_does_not_forward_raw_server_error_text() {
    // A `Failed` result carries the language server's own words — its stderr
    // tail, file paths, OS error strings — in `message`. Forwarding those to the
    // agent breaks this module's "no internal error text" guarantee (and the
    // Failed refusal sentence, which says the server's words stay in the
    // application). It must render the generic serverFailed sentence instead.
    let raw = r"thread panicked at C:\Users\me\repo\src\server.rs:42: os error 5";
    let answer = diagnostics(&DiagnosticsResult::unavailable(Availability::Failed, raw));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("serverFailed"));
    assert!(!answer.text.contains(raw), "{}", answer.text);
    assert!(!answer.text.contains(".rs:"), "{}", answer.text);
    assert!(!answer.text.contains("os error"), "{}", answer.text);
    assert!(!answer.text.contains(r"C:\Users"), "{}", answer.text);
    // It falls back to the generic curated sentence.
    assert!(answer.text.contains("did not answer"), "{}", answer.text);
}

#[test]
fn an_unsupported_diagnostics_answer_is_not_a_clean_file() {
    // The collapse this refuses: "no diagnostics" must not stand in for "the
    // server does not do diagnostics for this language".
    let unsupported = diagnostics(&DiagnosticsResult::unavailable(
        Availability::Unsupported,
        "",
    ));
    assert!(!unsupported.ok);
    assert_eq!(unsupported.code.as_deref(), Some("unsupported"));

    let clean = diagnostics(&DiagnosticsResult {
        outcome: Availability::Ready,
        diagnostics: Vec::new(),
        message: None,
        server: None,
    });
    assert!(clean.ok);
    assert!(clean.text.contains("clean"), "{}", clean.text);
}

// ---------------------------------------------------------------------------
// Ready + genuinely empty is an answer, not a refusal
// ---------------------------------------------------------------------------

#[test]
fn no_usages_is_a_successful_answer_not_a_refusal() {
    let answer = find_references(&UsageResult {
        outcome: Availability::Ready,
        total: Some(0),
        usages: Vec::new(),
        truncated: false,
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("No usages"), "{}", answer.text);
}

#[test]
fn usages_are_listed_with_label_and_line() {
    let answer = find_references(&UsageResult {
        outcome: Availability::Ready,
        total: Some(2),
        usages: vec![
            usage("src/App.cs", 10, "var w = new Widget();"),
            usage("src/Main.cs", 3, "Widget w;"),
        ],
        truncated: false,
        message: None,
        server: Some("roslyn".to_string()),
    });
    assert!(answer.ok);
    assert!(answer.text.contains("2 usage(s):"), "{}", answer.text);
    assert!(answer.text.contains("src/App.cs:10"), "{}", answer.text);
    assert!(answer.text.contains("src/Main.cs:3"), "{}", answer.text);
    assert!(answer.text.contains("(server: roslyn)"), "{}", answer.text);
}

#[test]
fn a_truncated_usage_list_reports_the_full_count() {
    // A cap that also caps the number is a lie; the total is the claim the agent
    // acts on.
    let answer = find_references(&UsageResult {
        outcome: Availability::Ready,
        total: Some(500),
        usages: vec![usage("a.cs", 1, "x")],
        truncated: true,
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("500 usage(s)"), "{}", answer.text);
    assert!(answer.text.contains("the rest are not"), "{}", answer.text);
}

#[test]
fn a_ready_message_is_appended_as_a_note() {
    // A readiness caveat on a real count must reach the reader.
    let answer = find_references(&UsageResult {
        outcome: Availability::Ready,
        total: Some(3),
        usages: vec![usage("a.cs", 1, "x")],
        truncated: false,
        message: Some("the server was still indexing; this may be low".to_string()),
        server: None,
    });
    assert!(
        answer.text.contains("Note: the server was still indexing"),
        "{}",
        answer.text
    );
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_are_listed_with_severity_and_position() {
    let answer = diagnostics(&DiagnosticsResult {
        outcome: Availability::Ready,
        diagnostics: vec![DiagnosticRow {
            severity: DiagnosticSeverity::Error,
            line: 12,
            character: 4,
            end_line: 12,
            end_character: 9,
            message: "The name 'Widgit' does not exist".to_string(),
            source: Some("roslyn".to_string()),
            code: Some("CS0103".to_string()),
        }],
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("1 diagnostic(s):"), "{}", answer.text);
    assert!(answer.text.contains("error 12:4"), "{}", answer.text);
    assert!(answer.text.contains("CS0103"), "{}", answer.text);
}

// ---------------------------------------------------------------------------
// Type hierarchy
// ---------------------------------------------------------------------------

#[test]
fn not_on_a_type_is_a_successful_answer() {
    let answer = type_hierarchy(&TypeHierarchyResult {
        outcome: Availability::Ready,
        item: None,
        supertypes: Vec::new(),
        subtypes: Vec::new(),
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("not on a type"), "{}", answer.text);
}

#[test]
fn a_type_lists_its_supertypes_and_subtypes() {
    let answer = type_hierarchy(&TypeHierarchyResult {
        outcome: Availability::Ready,
        item: Some(type_node("Widget", SymbolKind::Class, "src/Widget.cs", 5)),
        supertypes: vec![type_node("Control", SymbolKind::Class, "src/Control.cs", 2)],
        subtypes: vec![type_node("Button", SymbolKind::Class, "src/Button.cs", 3)],
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("Type Widget"), "{}", answer.text);
    assert!(answer.text.contains("Supertypes:"), "{}", answer.text);
    assert!(answer.text.contains("Control"), "{}", answer.text);
    assert!(answer.text.contains("Subtypes:"), "{}", answer.text);
    assert!(answer.text.contains("Button"), "{}", answer.text);
}

#[test]
fn an_empty_direction_says_none() {
    let answer = type_hierarchy(&TypeHierarchyResult {
        outcome: Availability::Ready,
        item: Some(type_node("Widget", SymbolKind::Class, "src/Widget.cs", 5)),
        supertypes: Vec::new(),
        subtypes: vec![type_node("Button", SymbolKind::Class, "src/Button.cs", 3)],
        message: None,
        server: None,
    });
    assert!(answer.text.contains("Supertypes: none."), "{}", answer.text);
}

#[test]
fn a_partial_refusal_message_is_appended_so_an_empty_direction_is_not_read_as_none() {
    let answer = type_hierarchy(&TypeHierarchyResult {
        outcome: Availability::Ready,
        item: Some(type_node("Widget", SymbolKind::Class, "src/Widget.cs", 5)),
        supertypes: Vec::new(),
        subtypes: Vec::new(),
        message: Some("the server refused subtypes; supertypes are complete".to_string()),
        server: None,
    });
    assert!(
        answer.text.contains("Note: the server refused subtypes"),
        "{}",
        answer.text
    );
}

// ---------------------------------------------------------------------------
// Overloads
// ---------------------------------------------------------------------------

#[test]
fn not_in_a_call_is_a_successful_answer() {
    let answer = overloads(&OverloadResult {
        outcome: Availability::Ready,
        signatures: Vec::new(),
        active_signature: None,
        active_parameter: None,
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("not inside a call"), "{}", answer.text);
}

#[test]
fn overloads_mark_the_active_signature_and_parameter() {
    let answer = overloads(&OverloadResult {
        outcome: Availability::Ready,
        signatures: vec![
            SignatureInfo {
                label: "Add(int a, int b)".to_string(),
                documentation: None,
                parameters: vec![
                    ParameterInfo {
                        label: "int a".to_string(),
                        documentation: None,
                    },
                    ParameterInfo {
                        label: "int b".to_string(),
                        documentation: None,
                    },
                ],
            },
            SignatureInfo {
                label: "Add(string a, string b)".to_string(),
                documentation: None,
                parameters: Vec::new(),
            },
        ],
        active_signature: Some(0),
        active_parameter: Some(1),
        message: None,
        server: None,
    });
    assert!(answer.ok);
    assert!(answer.text.contains("2 signature(s):"), "{}", answer.text);
    assert!(
        answer.text.contains("→ Add(int a, int b)"),
        "{}",
        answer.text
    );
    assert!(
        answer.text.contains("active parameter: int b"),
        "{}",
        answer.text
    );
}

#[test]
fn no_active_signature_is_stated_rather_than_guessed_at_zero() {
    let answer = overloads(&OverloadResult {
        outcome: Availability::Ready,
        signatures: vec![SignatureInfo {
            label: "Add(int a)".to_string(),
            documentation: None,
            parameters: Vec::new(),
        }],
        active_signature: None,
        active_parameter: None,
        message: None,
        server: None,
    });
    assert!(
        answer.text.contains("did not mark an active signature"),
        "{}",
        answer.text
    );
    assert!(!answer.text.contains("→ "), "{}", answer.text);
}
