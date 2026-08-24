use super::*;
use crate::model::{TestCase, TestOutcome, TestRunResult, TestSummary};

fn case(full_name: &str, outcome: TestOutcome) -> TestCase {
    TestCase {
        id: full_name.to_string(),
        name: full_name.to_string(),
        full_name: full_name.to_string(),
        suite: None,
        project: None,
        outcome,
        duration_ms: None,
        message: None,
        stack_trace: None,
        stdout: None,
    }
}

fn run(cases: Vec<TestCase>) -> TestRunResult {
    TestRunResult {
        summary: TestSummary::from_cases(&cases),
        cases,
        duration_ms: None,
    }
}

/// Find the delta for a case by name; panics if it was omitted (Unchanged).
fn delta<'a>(d: &'a TestDelta, full_name: &str) -> &'a CaseDelta {
    d.cases
        .iter()
        .find(|c| c.full_name == full_name)
        .unwrap_or_else(|| panic!("no delta for {full_name}; got {:?}", d.cases))
}

#[test]
fn regression_is_pass_to_fail() {
    let base = run(vec![case("a::t", TestOutcome::Passed)]);
    let work = run(vec![case("a::t", TestOutcome::Failed)]);
    let d = diff_tests(&base, &work);
    assert_eq!(delta(&d, "a::t").transition, CaseTransition::Regressed);
}

#[test]
fn fix_is_fail_to_pass() {
    let base = run(vec![case("a::t", TestOutcome::Failed)]);
    let work = run(vec![case("a::t", TestOutcome::Passed)]);
    let d = diff_tests(&base, &work);
    assert_eq!(delta(&d, "a::t").transition, CaseTransition::Fixed);
}

#[test]
fn other_is_never_a_pass() {
    // Passed -> Other is a regression (Other is on the failing side)...
    let base = run(vec![case("a::t", TestOutcome::Passed)]);
    let work = run(vec![case("a::t", TestOutcome::Other)]);
    assert_eq!(
        delta(&diff_tests(&base, &work), "a::t").transition,
        CaseTransition::Regressed
    );
    // ...and Other -> Passed is a fix, not "unchanged".
    let base = run(vec![case("a::t", TestOutcome::Other)]);
    let work = run(vec![case("a::t", TestOutcome::Passed)]);
    assert_eq!(
        delta(&diff_tests(&base, &work), "a::t").transition,
        CaseTransition::Fixed
    );
}

#[test]
fn still_failing_when_both_fail() {
    let base = run(vec![case("a::t", TestOutcome::Failed)]);
    let work = run(vec![case("a::t", TestOutcome::Other)]);
    assert_eq!(
        delta(&diff_tests(&base, &work), "a::t").transition,
        CaseTransition::StillFailing
    );
}

#[test]
fn unchanged_cases_are_omitted() {
    let base = run(vec![case("a::t", TestOutcome::Passed)]);
    let work = run(vec![case("a::t", TestOutcome::Passed)]);
    let d = diff_tests(&base, &work);
    assert!(
        d.cases.is_empty(),
        "pass->pass should not appear: {:?}",
        d.cases
    );
    // Skipped transitions are not confident changes either.
    let base = run(vec![case("a::t", TestOutcome::Passed)]);
    let work = run(vec![case("a::t", TestOutcome::Skipped)]);
    assert!(diff_tests(&base, &work).cases.is_empty());
}

#[test]
fn added_and_removed_by_full_name() {
    let base = run(vec![case("only_base::t", TestOutcome::Passed)]);
    let work = run(vec![case("only_work::t", TestOutcome::Passed)]);
    let d = diff_tests(&base, &work);
    assert_eq!(delta(&d, "only_work::t").transition, CaseTransition::Added);
    assert_eq!(
        delta(&d, "only_base::t").transition,
        CaseTransition::Removed
    );
    // Deterministic ordering by full_name.
    let names: Vec<&str> = d.cases.iter().map(|c| c.full_name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
}

#[test]
fn summaries_carry_totals() {
    let base = run(vec![
        case("a::t", TestOutcome::Passed),
        case("b::t", TestOutcome::Failed),
    ]);
    let work = run(vec![
        case("a::t", TestOutcome::Passed),
        case("b::t", TestOutcome::Passed),
    ]);
    let d = diff_tests(&base, &work);
    assert_eq!(d.summary_before.failed, 1);
    assert_eq!(d.summary_after.failed, 0);
    assert_eq!(d.summary_after.passed, 2);
}
