//! Diffing two [`TestRunResult`]s by test-case outcome.
//!
//! The two results come from running the same test config against HEAD and the
//! working tree. Cases are joined by [`TestCase::full_name`] — the same
//! fully-qualified name the "re-run failed" filter is built from — so a case
//! that moved file or line still lines up.
//!
//! The pass/fail sides follow the model's own rule: [`TestOutcome::Other`] is
//! *never* a pass (see [`crate::model::TestOutcome`]). `Passed` is the only
//! pass; `Failed` and `Other` are the failing side; `Skipped` is neither, so a
//! transition into or out of `Skipped` is reported as `Unchanged` rather than
//! guessed as a fix or a regression.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::{TestOutcome, TestRunResult, TestSummary};

/// How one test case's outcome moved between the HEAD and working-tree runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CaseTransition {
    /// Pass→pass, or any move the pass/fail rule does not treat as meaningful
    /// (e.g. involving `Skipped`).
    Unchanged,
    /// A failing case now passes.
    Fixed,
    /// A passing case now fails.
    Regressed,
    /// Failing on both sides (the message may still differ).
    StillFailing,
    /// Present only in the working-tree run.
    Added,
    /// Present only in the HEAD run.
    Removed,
}

/// One test case's before/after outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CaseDelta {
    pub full_name: String,
    /// The HEAD-run outcome, `None` when the case only appeared in the working tree.
    pub base: Option<TestOutcome>,
    /// The working-tree-run outcome, `None` when the case was removed.
    pub work: Option<TestOutcome>,
    pub transition: CaseTransition,
    /// Source files this case plausibly exercises, filled in during attribution
    /// (empty here). Used to map the delta to an intent card.
    pub files_hint: Vec<String>,
}

/// Every case whose outcome changed, plus the before/after summaries.
///
/// `Unchanged` cases are omitted — the summaries carry the totals, and a diff
/// should show what moved, not thousands of stable rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestDelta {
    pub cases: Vec<CaseDelta>,
    pub summary_before: TestSummary,
    pub summary_after: TestSummary,
}

fn is_pass(o: TestOutcome) -> bool {
    matches!(o, TestOutcome::Passed)
}

/// `Failed` and `Other` are the failing side; `Other` is never a pass.
fn is_fail(o: TestOutcome) -> bool {
    matches!(o, TestOutcome::Failed | TestOutcome::Other)
}

/// Classify a case present on both sides.
fn classify(base: TestOutcome, work: TestOutcome) -> CaseTransition {
    if is_fail(base) && is_pass(work) {
        CaseTransition::Fixed
    } else if is_pass(base) && is_fail(work) {
        CaseTransition::Regressed
    } else if is_fail(base) && is_fail(work) {
        CaseTransition::StillFailing
    } else {
        // pass→pass, and anything involving Skipped — not a confident change.
        CaseTransition::Unchanged
    }
}

/// Diff two test runs by `full_name`. Deterministic: cases are returned sorted
/// by name.
pub fn diff_tests(base: &TestRunResult, work: &TestRunResult) -> TestDelta {
    let base_by_name: BTreeMap<&str, TestOutcome> = base
        .cases
        .iter()
        .map(|c| (c.full_name.as_str(), c.outcome))
        .collect();
    let work_by_name: BTreeMap<&str, TestOutcome> = work
        .cases
        .iter()
        .map(|c| (c.full_name.as_str(), c.outcome))
        .collect();

    // Union of names, ascending (BTreeMap keys are already sorted; merge them).
    let mut names: Vec<&str> = base_by_name.keys().copied().collect();
    for name in work_by_name.keys() {
        if !base_by_name.contains_key(name) {
            names.push(name);
        }
    }
    names.sort_unstable();

    let mut cases = Vec::new();
    for name in names {
        let base_outcome = base_by_name.get(name).copied();
        let work_outcome = work_by_name.get(name).copied();
        let transition = match (base_outcome, work_outcome) {
            (Some(b), Some(w)) => classify(b, w),
            (None, Some(_)) => CaseTransition::Added,
            (Some(_), None) => CaseTransition::Removed,
            (None, None) => unreachable!("name came from one of the two maps"),
        };
        if transition == CaseTransition::Unchanged {
            continue;
        }
        cases.push(CaseDelta {
            full_name: name.to_string(),
            base: base_outcome,
            work: work_outcome,
            transition,
            files_hint: Vec::new(),
        });
    }

    TestDelta {
        cases,
        summary_before: base.summary,
        summary_after: work.summary,
    }
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
