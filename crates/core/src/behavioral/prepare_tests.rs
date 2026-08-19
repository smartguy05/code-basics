//! Tests for the pure report-assembly seam.

use super::*;
use crate::behavioral::{CaseDelta, CaseTransition, ConsoleDelta, TestDelta};
use crate::git::attribution::Confidence;
use crate::git::grouping::{GroupFile, GroupKind, IntentGroup};
use crate::model::{TestOutcome, TestSummary};

fn group(id: &str, path: &str) -> IntentGroup {
    IntentGroup {
        id: id.into(),
        kind: GroupKind::Intent,
        label: id.into(),
        symbol: None,
        files: vec![GroupFile {
            path: path.into(),
            line_indices: vec![],
            hunks: vec![],
        }],
        line_count: 1,
        confidence: Confidence::High,
    }
}

fn regressed_case(files_hint: Vec<String>) -> CaseDelta {
    CaseDelta {
        full_name: "Suite.regressed".into(),
        base: Some(TestOutcome::Passed),
        work: Some(TestOutcome::Failed),
        transition: CaseTransition::Regressed,
        files_hint,
    }
}

fn console_change(added: Vec<String>) -> ConsoleDelta {
    ConsoleDelta {
        added_lines: added,
        removed_lines: vec![],
        normalized: true,
        confidence: Confidence::Medium,
    }
}

#[test]
fn one_attributed_test_and_one_unattributed_console() {
    let groups = vec![group("g1", "src/foo.rs")];

    // The test's files_hint lands inside g1 → attributed to that one card.
    let tests = TestDelta {
        cases: vec![regressed_case(vec!["src/foo.rs".into()])],
        summary_before: TestSummary::default(),
        summary_after: TestSummary::default(),
    };
    // The console line names no known path → the honest unattributed bucket.
    let console = console_change(vec!["unrelated log line".into()]);

    let report = assemble_report(
        Some(tests),
        Some(console),
        vec![],
        &groups,
        vec!["a server never came up".into()],
    );

    assert_eq!(report.scorecard.outcomes_compared, 2); // tests + console
    assert_eq!(report.scorecard.deltas, 2); // one test change + one console change
    assert_eq!(report.scorecard.attributed_deltas, 1);
    assert_eq!(report.scorecard.unattributed_deltas, 1);
    assert_eq!(report.scorecard.abstained, 1); // one warning

    // The test delta is on g1's card; the console delta is unattributed.
    assert_eq!(report.attributions.len(), 1);
    assert_eq!(report.attributions[0].group_id, "g1");
    assert_eq!(report.attributions[0].deltas.len(), 1);
    assert!(matches!(
        report.attributions[0].deltas[0],
        BehavioralDelta::Test(_)
    ));
    assert_eq!(report.unattributed.len(), 1);
    assert!(matches!(
        report.unattributed[0],
        BehavioralDelta::Console(_)
    ));
}

#[test]
fn empty_inputs_yield_all_zero_scorecard_and_empty_buckets() {
    let report = assemble_report(None, None, vec![], &[], vec![]);

    assert_eq!(report.scorecard, BehavioralScorecard::default());
    assert_eq!(report.scorecard.outcomes_compared, 0);
    assert_eq!(report.scorecard.deltas, 0);
    assert!(report.attributions.is_empty());
    assert!(report.unattributed.is_empty());
    assert!(report.tests.is_none());
    assert!(report.console.is_none());
    assert!(report.http.is_empty());
    assert!(report.warnings.is_empty());
}

#[test]
fn console_equal_after_normalising_is_counted_but_not_a_delta() {
    // A console comparison that found no change still counts as an outcome
    // compared, but contributes no delta and is not pushed into any bucket.
    let no_change = ConsoleDelta {
        added_lines: vec![],
        removed_lines: vec![],
        normalized: true,
        confidence: Confidence::High,
    };
    let report = assemble_report(None, Some(no_change), vec![], &[], vec![]);

    assert_eq!(report.scorecard.outcomes_compared, 1);
    assert_eq!(report.scorecard.deltas, 0);
    assert!(report.unattributed.is_empty());
    assert!(report.attributions.is_empty());
    assert!(report.console.is_some());
}
