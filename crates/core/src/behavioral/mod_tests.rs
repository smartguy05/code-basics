//! Key-pinning for the behavioral wire types.
//!
//! `src/ipc/types.ts` mirrors these structs by hand, so a rename on the Rust
//! side has to fail somewhere visible rather than silently produce an
//! `undefined` in the UI. Same guard as the `tests` module in
//! [`crate::model`].

use super::*;
use crate::git::attribution::Confidence;
use serde_json::json;

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

fn sample_case_delta() -> CaseDelta {
    CaseDelta {
        full_name: "a::t".into(),
        base: Some(crate::model::TestOutcome::Passed),
        work: Some(crate::model::TestOutcome::Failed),
        transition: CaseTransition::Regressed,
        files_hint: vec![],
    }
}

fn sample_http_delta() -> HttpDelta {
    HttpDelta {
        name: "get-user".into(),
        status: Some((200, 404)),
        header_changes: vec![HeaderChange {
            name: "content-length".into(),
            before: Some("12".into()),
            after: Some("0".into()),
        }],
        body: Some(BodyDelta {
            added_lines: vec![],
            removed_lines: vec![],
            normalized: true,
        }),
        confidence: Confidence::Medium,
    }
}

#[test]
fn report_keys() {
    let report = BehavioralReport {
        tests: None,
        console: None,
        http: vec![],
        attributions: vec![],
        unattributed: vec![],
        scorecard: BehavioralScorecard::default(),
        warnings: vec![],
    };
    assert_eq!(
        keys(&serde_json::to_value(&report).unwrap()),
        [
            "attributions",
            "console",
            "http",
            "scorecard",
            "tests",
            "unattributed",
            "warnings",
        ]
    );
}

#[test]
fn scorecard_keys() {
    assert_eq!(
        keys(&serde_json::to_value(BehavioralScorecard::default()).unwrap()),
        [
            "abstained",
            "attributedDeltas",
            "deltas",
            "outcomesCompared",
            "unattributedDeltas",
        ]
    );
}

#[test]
fn case_delta_keys() {
    assert_eq!(
        keys(&serde_json::to_value(sample_case_delta()).unwrap()),
        ["base", "filesHint", "fullName", "transition", "work"]
    );
}

#[test]
fn test_delta_keys() {
    let d = TestDelta {
        cases: vec![],
        summary_before: crate::model::TestSummary::default(),
        summary_after: crate::model::TestSummary::default(),
    };
    assert_eq!(
        keys(&serde_json::to_value(&d).unwrap()),
        ["cases", "summaryAfter", "summaryBefore"]
    );
}

#[test]
fn console_delta_keys() {
    let d = ConsoleDelta {
        added_lines: vec![],
        removed_lines: vec![],
        normalized: true,
        confidence: Confidence::High,
    };
    assert_eq!(
        keys(&serde_json::to_value(&d).unwrap()),
        ["addedLines", "confidence", "normalized", "removedLines"]
    );
}

#[test]
fn http_delta_keys() {
    assert_eq!(
        keys(&serde_json::to_value(sample_http_delta()).unwrap()),
        ["body", "confidence", "headerChanges", "name", "status"]
    );
}

#[test]
fn header_change_keys() {
    let h = HeaderChange {
        name: "x".into(),
        before: None,
        after: None,
    };
    assert_eq!(
        keys(&serde_json::to_value(&h).unwrap()),
        ["after", "before", "name"]
    );
}

#[test]
fn card_behavior_keys() {
    let c = CardBehavior {
        group_id: "g1".into(),
        deltas: vec![],
        confidence: Confidence::Low,
    };
    assert_eq!(
        keys(&serde_json::to_value(&c).unwrap()),
        ["confidence", "deltas", "groupId"]
    );
}

#[test]
fn behavioral_delta_is_tagged() {
    // Internally tagged on `kind`; the inner struct's fields sit alongside.
    let v = serde_json::to_value(BehavioralDelta::Test(sample_case_delta())).unwrap();
    assert_eq!(v["kind"], json!("test"));
    assert_eq!(v["transition"], json!("regressed"));

    let v = serde_json::to_value(BehavioralDelta::Http(sample_http_delta())).unwrap();
    assert_eq!(v["kind"], json!("http"));
}

#[test]
fn transition_variants_are_camel_case() {
    assert_eq!(
        serde_json::to_value(CaseTransition::StillFailing).unwrap(),
        json!("stillFailing")
    );
    assert_eq!(
        serde_json::to_value(CaseTransition::Regressed).unwrap(),
        json!("regressed")
    );
}
