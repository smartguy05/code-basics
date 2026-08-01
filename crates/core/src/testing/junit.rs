//! Parser for JUnit-style XML test reports.
//!
//! This is the universal fallback. pytest, cargo-nextest, go-junit-report,
//! Gradle, PHPUnit, RSpec and most other runners can emit it, which is what
//! lets a new ecosystem be added through a declarative manifest with no Rust
//! code at all (see [`crate::adapters::manifest`]).
//!
//! The format is a convention rather than a standard, so this parser is
//! deliberately permissive: the root may be `<testsuites>` or a bare
//! `<testsuite>`, suites may nest, and outcome is expressed through the
//! *presence* of a child element rather than an attribute.

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::model::{TestCase, TestOutcome, TestRunResult, TestSummary};

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().as_ref() == name.as_bytes() {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

/// Which element's character data we are collecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextTarget {
    None,
    /// Body of `<failure>` / `<error>`, which conventionally holds the stack.
    Detail,
    SystemOut,
}

#[derive(Debug, Default)]
struct Partial {
    name: String,
    classname: Option<String>,
    duration_ms: Option<f64>,
    /// `None` until a `<failure>`, `<error>` or `<skipped>` child says otherwise.
    outcome: Option<TestOutcome>,
    message: Option<String>,
    detail: Option<String>,
    system_out: Option<String>,
}

/// Parse JUnit XML into a flat list of cases plus a summary.
pub fn parse(xml: &str) -> Result<TestRunResult> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut cases: Vec<TestCase> = Vec::new();
    let mut current: Option<Partial> = None;
    let mut text_target = TextTarget::None;
    // Suites nest, so the enclosing suite names form a stack.
    let mut suite_stack: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut total_time_ms: Option<f64> = None;

    loop {
        match reader.read_event().context("malformed JUnit XML")? {
            Event::Start(e) => {
                depth += 1;
                handle_open(
                    &e,
                    &mut current,
                    &mut suite_stack,
                    &mut text_target,
                    &mut total_time_ms,
                    true,
                );
            }

            Event::Empty(e) => {
                // Self-closing elements carry no body, so anything they
                // establish has to be finished immediately.
                handle_open(
                    &e,
                    &mut current,
                    &mut suite_stack,
                    &mut text_target,
                    &mut total_time_ms,
                    false,
                );
                if e.local_name().as_ref() == b"testcase" {
                    if let Some(p) = current.take() {
                        cases.push(finish(p, &suite_stack));
                    }
                }
                text_target = TextTarget::None;
            }

            Event::Text(t) => {
                let text = t.unescape().unwrap_or_default().into_owned();
                append(&mut current, text_target, &text);
            }
            Event::CData(t) => {
                let text = String::from_utf8_lossy(&t).into_owned();
                append(&mut current, text_target, &text);
            }

            Event::End(e) => {
                depth -= 1;
                match e.local_name().as_ref() {
                    b"testcase" => {
                        if let Some(p) = current.take() {
                            cases.push(finish(p, &suite_stack));
                        }
                    }
                    b"testsuite" => {
                        suite_stack.pop();
                    }
                    b"failure" | b"error" | b"skipped" | b"system-out" => {
                        text_target = TextTarget::None;
                    }
                    _ => {}
                }
            }

            Event::Eof => {
                anyhow::ensure!(
                    depth == 0,
                    "JUnit report is truncated: {depth} element(s) unclosed"
                );
                break;
            }
            _ => {}
        }
    }

    let summary = TestSummary::from_cases(&cases);
    Ok(TestRunResult {
        duration_ms: total_time_ms
            .or_else(|| Some(cases.iter().filter_map(|c| c.duration_ms).sum())),
        summary,
        cases,
    })
}

fn handle_open(
    e: &BytesStart,
    current: &mut Option<Partial>,
    suite_stack: &mut Vec<String>,
    text_target: &mut TextTarget,
    total_time_ms: &mut Option<f64>,
    is_container: bool,
) {
    match e.local_name().as_ref() {
        b"testsuites" => {
            // Only the outermost element's time describes the whole run.
            if total_time_ms.is_none() {
                *total_time_ms = attr(e, "time").and_then(|t| seconds_to_ms(&t));
            }
        }
        b"testsuite" => {
            if is_container {
                suite_stack.push(attr(e, "name").unwrap_or_default());
            }
            if total_time_ms.is_none() {
                // A bare <testsuite> root: its time is the run's time.
                *total_time_ms = attr(e, "time").and_then(|t| seconds_to_ms(&t));
            }
        }
        b"testcase" => {
            *current = Some(Partial {
                name: attr(e, "name").unwrap_or_default(),
                classname: attr(e, "classname").filter(|c| !c.is_empty()),
                duration_ms: attr(e, "time").and_then(|t| seconds_to_ms(&t)),
                outcome: None,
                ..Default::default()
            });
        }
        // Outcome is signalled by the presence of these children, not by an
        // attribute — an absent child means the test passed.
        b"failure" | b"error" => {
            if let Some(p) = current.as_mut() {
                p.outcome = Some(TestOutcome::Failed);
                if let Some(m) = attr(e, "message") {
                    p.message = Some(m);
                }
            }
            *text_target = TextTarget::Detail;
        }
        b"skipped" => {
            if let Some(p) = current.as_mut() {
                p.outcome = Some(TestOutcome::Skipped);
                if let Some(m) = attr(e, "message") {
                    p.message = Some(m);
                }
            }
            *text_target = TextTarget::Detail;
        }
        b"system-out" => *text_target = TextTarget::SystemOut,
        _ => {}
    }
}

fn append(current: &mut Option<Partial>, target: TextTarget, text: &str) {
    if target == TextTarget::None || text.is_empty() {
        return;
    }
    let Some(p) = current.as_mut() else { return };
    let slot = match target {
        TextTarget::Detail => &mut p.detail,
        TextTarget::SystemOut => &mut p.system_out,
        TextTarget::None => unreachable!("guarded above"),
    };
    match slot {
        Some(existing) => existing.push_str(text),
        None => *slot = Some(text.to_string()),
    }
}

fn finish(p: Partial, suite_stack: &[String]) -> TestCase {
    // A testcase with no failure/error/skipped child passed.
    let outcome = p.outcome.unwrap_or(TestOutcome::Passed);

    let suite = p
        .classname
        .clone()
        .or_else(|| suite_stack.last().cloned().filter(|s| !s.is_empty()));

    let full_name = match &suite {
        Some(s) => format!("{s}.{}", p.name),
        None => p.name.clone(),
    };

    // The outermost suite makes the most useful project label; nested suites
    // are already represented by `suite`.
    let project = suite_stack.first().cloned().filter(|s| !s.is_empty());

    let detail = p.detail.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());

    TestCase {
        id: full_name.clone(),
        name: p.name,
        full_name,
        suite,
        project,
        outcome,
        duration_ms: p.duration_ms,
        // `message` is the attribute; the body is conventionally the stack.
        // When only one is present it becomes the message, so nothing is lost.
        message: p.message.or_else(|| detail.clone()),
        stack_trace: detail,
        stdout: p.system_out.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    }
}

/// JUnit expresses durations in fractional seconds.
fn seconds_to_ms(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok().map(|s| s * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../fixtures/reports/junit.xml");

    #[test]
    fn parses_counts_across_nested_suites() {
        let run = parse(SAMPLE).expect("sample JUnit should parse");
        assert_eq!(run.summary.total, 4);
        assert_eq!(run.summary.passed, 2);
        assert_eq!(run.summary.failed, 1);
        assert_eq!(run.summary.skipped, 1);
    }

    #[test]
    fn treats_a_testcase_without_children_as_passed() {
        let run = parse(SAMPLE).unwrap();
        let case = run.cases.iter().find(|c| c.name == "test_addition").unwrap();
        assert_eq!(case.outcome, TestOutcome::Passed);
    }

    #[test]
    fn reads_failure_message_from_attribute_and_stack_from_body() {
        let run = parse(SAMPLE).unwrap();
        let failed = run
            .cases
            .iter()
            .find(|c| c.outcome == TestOutcome::Failed)
            .unwrap();

        assert_eq!(failed.message.as_deref(), Some("assert 3 == 2"));
        assert!(failed.stack_trace.as_ref().unwrap().contains("test_math.py:14"));
    }

    #[test]
    fn uses_classname_as_the_suite() {
        let run = parse(SAMPLE).unwrap();
        let case = run.cases.iter().find(|c| c.name == "test_addition").unwrap();
        assert_eq!(case.suite.as_deref(), Some("tests.test_math"));
        assert_eq!(case.full_name, "tests.test_math.test_addition");
    }

    #[test]
    fn accepts_a_bare_testsuite_root() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuite name="solo" tests="1" failures="0" time="0.25">
  <testcase classname="solo.Thing" name="works" time="0.25" />
</testsuite>"#;
        let run = parse(xml).unwrap();
        assert_eq!(run.summary.total, 1);
        assert_eq!(run.summary.passed, 1);
        assert_eq!(run.duration_ms, Some(250.0));
    }

    #[test]
    fn handles_self_closing_skipped_without_a_message() {
        let xml = r#"<testsuite name="s"><testcase name="t"><skipped /></testcase></testsuite>"#;
        let run = parse(xml).unwrap();
        assert_eq!(run.cases[0].outcome, TestOutcome::Skipped);
    }

    #[test]
    fn converts_seconds_to_milliseconds() {
        assert_eq!(seconds_to_ms("0.25"), Some(250.0));
        assert_eq!(seconds_to_ms("12"), Some(12_000.0));
        assert_eq!(seconds_to_ms(""), None);
    }

    #[test]
    fn truncated_report_is_an_error() {
        assert!(parse("<testsuites><testsuite>").is_err());
    }
}
