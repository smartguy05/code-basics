//! Tests for resolving a `{symbol}` target against a file's declaration anchors.

use super::*;
use crate::lsp::model::{AnchorResult, Availability, DeclarationAnchor};
use crate::symbols::declarations::SymbolKind;

fn anchor(name: &str, selection_line: u32, character: u32) -> DeclarationAnchor {
    DeclarationAnchor {
        id: format!("{name}@{selection_line}"),
        name: name.to_string(),
        kind: SymbolKind::Function,
        line: selection_line,
        character,
        selection_line,
    }
}

fn ready(anchors: Vec<DeclarationAnchor>) -> AnchorResult {
    AnchorResult {
        outcome: Availability::Ready,
        anchors,
        message: None,
    }
}

#[test]
fn one_match_resolves_to_its_identifier_position() {
    let anchors = ready(vec![anchor("Alpha", 3, 8), anchor("Beta", 10, 4)]);
    assert_eq!(
        locate_symbol("Beta", "src/App.cs", &anchors),
        Located::At {
            line: 10,
            character: 4
        }
    );
}

#[test]
fn a_surrounding_whitespace_symbol_still_matches() {
    let anchors = ready(vec![anchor("Alpha", 3, 8)]);
    assert_eq!(
        locate_symbol("  Alpha  ", "src/App.cs", &anchors),
        Located::At {
            line: 3,
            character: 8
        }
    );
}

#[test]
fn no_match_refuses_and_lists_the_names_found() {
    let anchors = ready(vec![anchor("Alpha", 3, 8), anchor("Beta", 10, 4)]);
    let Located::Refused(answer) = locate_symbol("Gamma", "src/App.cs", &anchors) else {
        panic!("a name with no declaration must refuse");
    };
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some(SYMBOL_NOT_FOUND));
    // The available names are listed rather than the caller being left to guess.
    assert!(answer.text.contains("Alpha"));
    assert!(answer.text.contains("Beta"));
}

#[test]
fn several_matches_refuse_rather_than_pick() {
    let anchors = ready(vec![
        anchor("Overloaded", 3, 8),
        anchor("Overloaded", 20, 8),
    ]);
    let Located::Refused(answer) = locate_symbol("Overloaded", "src/App.cs", &anchors) else {
        panic!("an ambiguous name must refuse, never pick");
    };
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some(AMBIGUOUS_SYMBOL));
    // Both candidate lines are named so the agent can pass a position.
    assert!(answer.text.contains('3'));
    assert!(answer.text.contains("20"));
}

#[test]
fn a_non_ready_anchor_result_surfaces_that_state_not_not_found() {
    let anchors = AnchorResult::unavailable(Availability::Loading, "still loading projects");
    let Located::Refused(answer) = locate_symbol("Alpha", "src/App.cs", &anchors) else {
        panic!("a loading server must not report the symbol as missing");
    };
    // The loading state must not collapse into "no such symbol".
    assert_eq!(
        answer.code.as_deref(),
        Some(super::super::answer::RoslynRefusal::Loading.code())
    );
    assert_ne!(answer.code.as_deref(), Some(SYMBOL_NOT_FOUND));
}

#[test]
fn the_two_symbol_codes_are_distinct() {
    assert_ne!(SYMBOL_NOT_FOUND, AMBIGUOUS_SYMBOL);
}
