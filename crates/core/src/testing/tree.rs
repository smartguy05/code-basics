//! Turning a flat list of test cases into the hierarchy the UI renders.
//!
//! Parsers deliberately emit a flat list — it is the shape every report format
//! naturally produces — and the grouping into project → suite → test happens
//! once, here, so all four runners produce an identical tree.

use std::collections::BTreeMap;

use crate::model::{TestCase, TestNode, TestOutcome, TestSummary};

/// Precedence used when rolling a node's children up into one outcome.
///
/// A single failure must colour every ancestor, so `Failed` outranks
/// everything. `Passed` outranks `Skipped` so that a suite of mostly-passing
/// tests with one skip still reads as passing, while a suite that is entirely
/// skipped correctly reads as skipped.
fn rank(outcome: TestOutcome) -> u8 {
    match outcome {
        TestOutcome::Failed => 3,
        TestOutcome::Other => 2,
        TestOutcome::Passed => 1,
        TestOutcome::Skipped => 0,
    }
}

fn worst(outcomes: impl IntoIterator<Item = TestOutcome>) -> TestOutcome {
    outcomes
        .into_iter()
        .max_by_key(|o| rank(*o))
        .unwrap_or(TestOutcome::Skipped)
}

/// Group cases into a project → suite → test tree.
///
/// Cases with no project or suite are attached at the level above rather than
/// getting an empty placeholder node, so a runner that reports neither still
/// produces a flat, usable list.
pub fn build(cases: &[TestCase]) -> Vec<TestNode> {
    // BTreeMap keeps projects and suites in a stable alphabetical order, so
    // the tree does not reshuffle between runs.
    let mut by_project: BTreeMap<Option<&str>, BTreeMap<Option<&str>, Vec<&TestCase>>> =
        BTreeMap::new();

    for case in cases {
        by_project
            .entry(case.project.as_deref())
            .or_default()
            .entry(case.suite.as_deref())
            .or_default()
            .push(case);
    }

    by_project
        .into_iter()
        .map(|(project, suites)| {
            let mut children: Vec<TestNode> = Vec::new();

            for (suite, suite_cases) in suites {
                let leaves: Vec<TestNode> = suite_cases.iter().map(|c| leaf(c)).collect();

                match suite {
                    // Ungrouped cases hang directly off the project.
                    None => children.extend(leaves),
                    Some(name) => children.push(branch(
                        format!("{}::{name}", project.unwrap_or("")),
                        name.to_string(),
                        leaves,
                    )),
                }
            }

            match project {
                None => children,
                Some(name) => vec![branch(name.to_string(), name.to_string(), children)],
            }
        })
        // A workspace with several projects yields several roots.
        .reduce(|mut acc, mut next| {
            acc.append(&mut next);
            acc
        })
        .unwrap_or_default()
}

fn leaf(case: &TestCase) -> TestNode {
    TestNode {
        id: case.id.clone(),
        label: case.name.clone(),
        outcome: case.outcome,
        summary: TestSummary::from_cases(std::slice::from_ref(case)),
        duration_ms: case.duration_ms,
        case: Some(case.clone()),
        children: Vec::new(),
    }
}

fn branch(id: String, label: String, children: Vec<TestNode>) -> TestNode {
    let mut summary = TestSummary::default();
    for child in &children {
        summary.total += child.summary.total;
        summary.passed += child.summary.passed;
        summary.failed += child.summary.failed;
        summary.skipped += child.summary.skipped;
        summary.other += child.summary.other;
    }

    // A branch with no timings at all should report none, rather than 0ms.
    let durations: Vec<f64> = children.iter().filter_map(|c| c.duration_ms).collect();
    let duration_ms = (!durations.is_empty()).then(|| durations.iter().sum());

    TestNode {
        id,
        label,
        outcome: worst(children.iter().map(|c| c.outcome)),
        summary,
        duration_ms,
        case: None,
        children,
    }
}

/// Fully qualified names of every failing case, for a re-run-failed filter.
pub fn failed_names(cases: &[TestCase]) -> Vec<String> {
    cases
        .iter()
        .filter(|c| c.outcome == TestOutcome::Failed)
        .map(|c| c.full_name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(project: Option<&str>, suite: Option<&str>, name: &str, outcome: TestOutcome) -> TestCase {
        TestCase {
            id: format!("{}::{name}", suite.unwrap_or("")),
            name: name.to_string(),
            full_name: match suite {
                Some(s) => format!("{s}.{name}"),
                None => name.to_string(),
            },
            suite: suite.map(str::to_string),
            project: project.map(str::to_string),
            outcome,
            duration_ms: Some(10.0),
            message: None,
            stack_trace: None,
            stdout: None,
        }
    }

    #[test]
    fn groups_by_project_then_suite() {
        let cases = vec![
            case(Some("Calc.Tests"), Some("CalcTests"), "Adds", TestOutcome::Passed),
            case(Some("Calc.Tests"), Some("CalcTests"), "Subtracts", TestOutcome::Passed),
            case(Some("Calc.Tests"), Some("NumTests"), "IsEven", TestOutcome::Passed),
        ];
        let tree = build(&cases);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].label, "Calc.Tests");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].label, "CalcTests");
        assert_eq!(tree[0].children[0].children.len(), 2);
    }

    #[test]
    fn a_single_failure_colours_every_ancestor() {
        let cases = vec![
            case(Some("P"), Some("S"), "ok", TestOutcome::Passed),
            case(Some("P"), Some("S"), "bad", TestOutcome::Failed),
        ];
        let tree = build(&cases);

        assert_eq!(tree[0].outcome, TestOutcome::Failed, "project should show failed");
        assert_eq!(tree[0].children[0].outcome, TestOutcome::Failed, "suite should show failed");
    }

    #[test]
    fn a_suite_of_passes_and_skips_reads_as_passing() {
        let cases = vec![
            case(Some("P"), Some("S"), "ok", TestOutcome::Passed),
            case(Some("P"), Some("S"), "later", TestOutcome::Skipped),
        ];
        assert_eq!(build(&cases)[0].outcome, TestOutcome::Passed);
    }

    #[test]
    fn an_entirely_skipped_suite_reads_as_skipped() {
        let cases = vec![
            case(Some("P"), Some("S"), "a", TestOutcome::Skipped),
            case(Some("P"), Some("S"), "b", TestOutcome::Skipped),
        ];
        assert_eq!(build(&cases)[0].outcome, TestOutcome::Skipped);
    }

    #[test]
    fn rolls_up_counts_and_durations() {
        let cases = vec![
            case(Some("P"), Some("S"), "a", TestOutcome::Passed),
            case(Some("P"), Some("S"), "b", TestOutcome::Failed),
            case(Some("P"), Some("S"), "c", TestOutcome::Skipped),
        ];
        let tree = build(&cases);

        assert_eq!(tree[0].summary.total, 3);
        assert_eq!(tree[0].summary.passed, 1);
        assert_eq!(tree[0].summary.failed, 1);
        assert_eq!(tree[0].summary.skipped, 1);
        assert_eq!(tree[0].duration_ms, Some(30.0));
    }

    #[test]
    fn cases_without_a_suite_hang_directly_off_the_project() {
        let cases = vec![case(Some("P"), None, "loose", TestOutcome::Passed)];
        let tree = build(&cases);

        assert_eq!(tree[0].label, "P");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].label, "loose");
        assert!(tree[0].children[0].case.is_some(), "should be a leaf");
    }

    #[test]
    fn cases_without_a_project_produce_a_flat_list() {
        let cases = vec![
            case(None, None, "one", TestOutcome::Passed),
            case(None, None, "two", TestOutcome::Passed),
        ];
        let tree = build(&cases);

        assert_eq!(tree.len(), 2);
        assert!(tree.iter().all(|n| n.case.is_some()));
    }

    #[test]
    fn several_projects_produce_several_roots() {
        let cases = vec![
            case(Some("A"), Some("S"), "x", TestOutcome::Passed),
            case(Some("B"), Some("S"), "y", TestOutcome::Passed),
        ];
        let tree = build(&cases);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].label, "A");
        assert_eq!(tree[1].label, "B");
    }

    #[test]
    fn collects_failing_names_for_a_rerun() {
        let cases = vec![
            case(Some("P"), Some("S"), "ok", TestOutcome::Passed),
            case(Some("P"), Some("S"), "bad", TestOutcome::Failed),
            case(Some("P"), Some("S"), "worse", TestOutcome::Failed),
        ];
        assert_eq!(failed_names(&cases), vec!["S.bad".to_string(), "S.worse".to_string()]);
    }

    #[test]
    fn empty_input_produces_an_empty_tree() {
        assert!(build(&[]).is_empty());
    }
}
