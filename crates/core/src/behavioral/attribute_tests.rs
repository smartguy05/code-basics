use super::*;
use crate::behavioral::{BodyDelta, CaseDelta, CaseTransition, ConsoleDelta, HttpDelta};
use crate::git::grouping::{GroupFile, GroupKind, IntentGroup};

fn group(id: &str, paths: &[&str]) -> IntentGroup {
    IntentGroup {
        id: id.into(),
        kind: GroupKind::Intent,
        label: id.into(),
        symbol: None,
        files: paths
            .iter()
            .map(|p| GroupFile {
                path: p.to_string(),
                line_indices: vec![],
                hunks: vec![],
            })
            .collect(),
        line_count: 0,
        confidence: Confidence::High,
    }
}

fn test_delta(hints: &[&str]) -> BehavioralDelta {
    BehavioralDelta::Test(CaseDelta {
        full_name: "some::test".into(),
        base: None,
        work: None,
        transition: CaseTransition::Regressed,
        files_hint: hints.iter().map(|s| s.to_string()).collect(),
    })
}

fn console_delta(added: &[&str], confidence: Confidence) -> BehavioralDelta {
    BehavioralDelta::Console(ConsoleDelta {
        added_lines: added.iter().map(|s| s.to_string()).collect(),
        removed_lines: vec![],
        normalized: true,
        confidence,
    })
}

fn http_delta() -> BehavioralDelta {
    BehavioralDelta::Http(HttpDelta {
        name: "get".into(),
        status: Some((200, 500)),
        header_changes: vec![],
        body: Some(BodyDelta {
            added_lines: vec![],
            removed_lines: vec![],
            normalized: true,
        }),
        confidence: Confidence::Medium,
    })
}

#[test]
fn single_group_pin() {
    let groups = [group("g1", &["src/a.ts"]), group("g2", &["src/b.ts"])];
    let (cards, un) = attribute_behavioral(vec![test_delta(&["src/a.ts"])], &groups);
    assert!(un.is_empty());
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].group_id, "g1");
}

#[test]
fn ambiguous_delta_goes_to_unattributed() {
    // A hint that lands in two groups is never pinned to either.
    let groups = [group("g1", &["src/a.ts"]), group("g2", &["src/b.ts"])];
    let (cards, un) = attribute_behavioral(vec![test_delta(&["src/a.ts", "src/b.ts"])], &groups);
    assert!(cards.is_empty(), "ambiguous delta must not land on a card");
    assert_eq!(un.len(), 1);
}

#[test]
fn unowned_delta_goes_to_unattributed() {
    let groups = [group("g1", &["src/a.ts"])];
    let (cards, un) = attribute_behavioral(vec![test_delta(&["src/nowhere.ts"])], &groups);
    assert!(cards.is_empty());
    assert_eq!(un.len(), 1);
}

#[test]
fn a_delta_with_no_evidence_is_unattributed() {
    let groups = [group("g1", &["src/a.ts"])];
    // No files_hint at all — nothing to attribute on.
    let (cards, un) = attribute_behavioral(vec![test_delta(&[])], &groups);
    assert!(cards.is_empty());
    assert_eq!(un.len(), 1);
}

#[test]
fn console_line_naming_a_file_attributes() {
    let groups = [group("g1", &["src/api.ts"]), group("g2", &["src/other.ts"])];
    let d = console_delta(&["updated handler in src/api.ts:42"], Confidence::Medium);
    let (cards, un) = attribute_behavioral(vec![d], &groups);
    assert!(un.is_empty(), "a console line naming one group's file should attribute");
    assert_eq!(cards[0].group_id, "g1");
}

#[test]
fn http_delta_is_unattributed() {
    // No path evidence on an HTTP delta — route→file is not derivable here.
    let groups = [group("g1", &["src/a.ts"])];
    let (cards, un) = attribute_behavioral(vec![http_delta()], &groups);
    assert!(cards.is_empty());
    assert_eq!(un.len(), 1);
}

#[test]
fn card_confidence_is_the_weakest_member() {
    let groups = [group("g1", &["src/a.ts"])];
    let deltas = vec![
        test_delta(&["src/a.ts"]),                            // Medium
        console_delta(&["touched src/a.ts"], Confidence::Low), // Low
    ];
    let (cards, un) = attribute_behavioral(deltas, &groups);
    assert!(un.is_empty());
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].deltas.len(), 2);
    assert_eq!(cards[0].confidence, Confidence::Low, "card takes its shakiest delta");
}

#[test]
fn cards_are_sorted_by_group_id() {
    let groups = [group("z1", &["src/z.ts"]), group("a1", &["src/a.ts"])];
    let deltas = vec![test_delta(&["src/z.ts"]), test_delta(&["src/a.ts"])];
    let (cards, _) = attribute_behavioral(deltas, &groups);
    let ids: Vec<&str> = cards.iter().map(|c| c.group_id.as_str()).collect();
    assert_eq!(ids, ["a1", "z1"]);
}
