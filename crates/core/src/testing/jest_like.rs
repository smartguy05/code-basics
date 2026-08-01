//! Parser for the JSON report shared by Jest and Vitest.
//!
//! Vitest deliberately mirrors Jest's `--json` output, so one parser serves
//! both runners and the adapter only has to differ in how it spells the
//! command line. Where the two do diverge — Jest writes a suite-level
//! `failureMessage`, Vitest writes `message` — both spellings are accepted.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::model::{TestCase, TestOutcome, TestRunResult, TestSummary};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    #[serde(default)]
    test_results: Vec<FileResult>,
    #[serde(default)]
    start_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileResult {
    /// Absolute path to the test file.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    assertion_results: Vec<Assertion>,
    #[serde(default)]
    start_time: Option<f64>,
    #[serde(default)]
    end_time: Option<f64>,
    /// Vitest's spelling for a suite-level failure (e.g. an import that threw).
    #[serde(default)]
    message: Option<String>,
    /// Jest's spelling for the same thing.
    #[serde(default)]
    failure_message: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Assertion {
    #[serde(default)]
    ancestor_titles: Vec<String>,
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    failure_messages: Vec<String>,
}

/// Map the runner's status vocabulary onto our four-state model.
fn parse_status(raw: &str) -> TestOutcome {
    match raw {
        "passed" => TestOutcome::Passed,
        "failed" => TestOutcome::Failed,
        // `todo` and `disabled` never ran, same as an explicit skip.
        "pending" | "skipped" | "todo" | "disabled" => TestOutcome::Skipped,
        _ => TestOutcome::Other,
    }
}

/// Split a combined failure message into its human-readable part and its stack.
///
/// Jest and Vitest concatenate the assertion message and the stack into one
/// string. The stack begins at the first line of the form `    at ...`, so the
/// failure pane can show a short message without the frames drowning it.
fn split_message_and_stack(combined: &str) -> (Option<String>, Option<String>) {
    let mut split_at = None;
    let mut offset = 0usize;

    for line in combined.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("at ") && line.starts_with(char::is_whitespace) {
            split_at = Some(offset);
            break;
        }
        offset += line.len();
    }

    match split_at {
        Some(idx) => {
            let message = combined[..idx].trim_end();
            let stack = combined[idx..].trim_end();
            (
                (!message.is_empty()).then(|| message.to_string()),
                (!stack.is_empty()).then(|| stack.to_string()),
            )
        }
        None => {
            let message = combined.trim_end();
            ((!message.is_empty()).then(|| message.to_string()), None)
        }
    }
}

/// Parse a Jest or Vitest JSON report.
pub fn parse(json: &str) -> Result<TestRunResult> {
    let report: Report =
        serde_json::from_str(json).context("report is not valid Jest/Vitest JSON")?;

    let mut cases = Vec::new();
    let mut latest_end: Option<f64> = None;
    let mut earliest_start: Option<f64> = None;

    for file in &report.test_results {
        if let Some(end) = file.end_time {
            latest_end = Some(latest_end.map_or(end, |cur: f64| cur.max(end)));
        }
        if let Some(start) = file.start_time {
            earliest_start = Some(earliest_start.map_or(start, |cur: f64| cur.min(start)));
        }

        let file_label = file.name.as_deref().unwrap_or("(unknown file)");

        for a in &file.assertion_results {
            let title = a.title.clone().unwrap_or_default();
            let suite = (!a.ancestor_titles.is_empty()).then(|| a.ancestor_titles.join(" › "));

            let full_name = a.full_name.clone().unwrap_or_else(|| {
                let mut parts = a.ancestor_titles.clone();
                parts.push(title.clone());
                parts.join(" ")
            });

            let combined = a.failure_messages.join("\n\n");
            let (message, stack_trace) = if combined.is_empty() {
                (None, None)
            } else {
                split_message_and_stack(&combined)
            };

            cases.push(TestCase {
                // File plus full name is unique even when two files use the
                // same describe/it titles.
                id: format!("{file_label}::{full_name}"),
                name: if title.is_empty() { full_name.clone() } else { title },
                full_name,
                suite,
                project: file.name.clone(),
                outcome: a.status.as_deref().map(parse_status).unwrap_or_default(),
                duration_ms: a.duration,
                message,
                stack_trace,
                stdout: None,
            });
        }

        // A file that fails to load reports a failure with no assertions at
        // all. Without this it would vanish from the tree entirely, and the
        // run would look like it merely ran fewer tests.
        if file.assertion_results.is_empty() {
            let failure = file
                .message
                .as_deref()
                .or(file.failure_message.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let outcome = file.status.as_deref().map(parse_status).unwrap_or_default();
            if failure.is_some() || outcome == TestOutcome::Failed {
                let (message, stack_trace) =
                    failure.map_or((None, None), split_message_and_stack);
                cases.push(TestCase {
                    id: format!("{file_label}::<file>"),
                    name: short_file_name(file_label),
                    full_name: file_label.to_string(),
                    suite: None,
                    project: file.name.clone(),
                    outcome: TestOutcome::Failed,
                    duration_ms: None,
                    message: message.or_else(|| Some("test file failed to run".to_string())),
                    stack_trace,
                    stdout: None,
                });
            }
        }
    }

    // Vitest reports a run-level `startTime`; some Jest versions only report
    // per-file ones, so fall back to the earliest of those.
    let run_start = report.start_time.or(earliest_start);
    let duration_ms = match (run_start, latest_end) {
        (Some(start), Some(end)) if end >= start => Some(end - start),
        _ => {
            let total: f64 = cases.iter().filter_map(|c| c.duration_ms).sum();
            (total > 0.0).then_some(total)
        }
    };

    Ok(TestRunResult {
        summary: TestSummary::from_cases(&cases),
        cases,
        duration_ms,
    })
}

fn short_file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VITEST: &str = include_str!("../../fixtures/reports/vitest.json");
    const JEST: &str = include_str!("../../fixtures/reports/jest.json");

    #[test]
    fn parses_vitest_counts() {
        let run = parse(VITEST).expect("vitest report should parse");
        assert_eq!(run.summary.total, 4);
        assert_eq!(run.summary.passed, 2);
        assert_eq!(run.summary.failed, 1);
        assert_eq!(run.summary.skipped, 1);
    }

    #[test]
    fn parses_jest_counts_with_the_same_parser() {
        let run = parse(JEST).expect("jest report should parse");
        assert_eq!(run.summary.total, 2);
        assert_eq!(run.summary.passed, 1);
        assert_eq!(run.summary.failed, 1);
    }

    #[test]
    fn builds_suite_from_the_describe_chain() {
        let run = parse(VITEST).unwrap();
        let case = run.cases.iter().find(|c| c.name == "adds numbers").unwrap();

        assert_eq!(case.suite.as_deref(), Some("math › add"));
        assert_eq!(case.full_name, "math add adds numbers");
        assert_eq!(case.project.as_deref(), Some("/repo/src/math.test.ts"));
    }

    #[test]
    fn separates_the_assertion_message_from_the_stack() {
        let run = parse(VITEST).unwrap();
        let failed = run
            .cases
            .iter()
            .find(|c| c.outcome == TestOutcome::Failed)
            .expect("a failing case");

        let message = failed.message.as_ref().unwrap();
        let stack = failed.stack_trace.as_ref().unwrap();

        assert!(message.contains("expected 3 to be 2"));
        assert!(!message.contains("    at "), "stack frames leaked into the message");
        assert!(stack.starts_with("    at "));
        assert!(stack.contains("math.test.ts:9:23"));
    }

    #[test]
    fn message_without_a_stack_is_kept_whole() {
        let (msg, stack) = split_message_and_stack("boom, no frames here");
        assert_eq!(msg.as_deref(), Some("boom, no frames here"));
        assert_eq!(stack, None);
    }

    #[test]
    fn does_not_mistake_a_message_mentioning_at_for_a_stack_frame() {
        // A leading-whitespace `at ` line is a frame; prose is not.
        let (msg, stack) = split_message_and_stack("failed at the boundary\nsecond line");
        assert_eq!(msg.as_deref(), Some("failed at the boundary\nsecond line"));
        assert_eq!(stack, None);
    }

    #[test]
    fn maps_todo_and_skipped_to_skipped() {
        assert_eq!(parse_status("pending"), TestOutcome::Skipped);
        assert_eq!(parse_status("skipped"), TestOutcome::Skipped);
        assert_eq!(parse_status("todo"), TestOutcome::Skipped);
        assert_eq!(parse_status("passed"), TestOutcome::Passed);
        assert_eq!(parse_status("failed"), TestOutcome::Failed);
    }

    #[test]
    fn surfaces_a_file_that_failed_to_load_as_a_failing_case() {
        // No assertions ran, so without special handling this file would
        // disappear and the run would look smaller rather than broken.
        let json = r#"{
          "testResults": [
            { "name": "/repo/src/broken.test.ts",
              "status": "failed",
              "assertionResults": [],
              "message": "Cannot find module './missing'\n    at /repo/src/broken.test.ts:1:1" }
          ]
        }"#;
        let run = parse(json).unwrap();

        assert_eq!(run.summary.total, 1);
        assert_eq!(run.summary.failed, 1);
        assert_eq!(run.cases[0].name, "broken.test.ts");
        assert!(run.cases[0].message.as_ref().unwrap().contains("Cannot find module"));
    }

    #[test]
    fn empty_report_parses_to_an_empty_run() {
        let run = parse(r#"{"testResults":[]}"#).unwrap();
        assert_eq!(run.summary.total, 0);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse("not json at all").is_err());
    }
}
