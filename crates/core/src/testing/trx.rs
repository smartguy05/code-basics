//! Parser for Visual Studio `.trx` test reports.
//!
//! Both `dotnet test` execution paths — classic VSTest and
//! Microsoft.Testing.Platform — can emit TRX, which is the whole reason the
//! .NET adapter targets this format: the two runners disagree about almost
//! every command line flag, but they agree on the report.
//!
//! The interesting structural detail is that a TRX splits what the UI needs
//! across two sections. `<Results>` carries outcomes and timings keyed by
//! `testId`; `<TestDefinitions>` carries the class name and owning assembly
//! keyed by the same id. Neither alone is enough, so this parses both and
//! joins them.

use std::collections::HashMap;

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::model::{TestCase, TestOutcome, TestRunResult, TestSummary};

/// Read the value of an attribute, ignoring any namespace prefix.
fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        let key = a.key;
        let local = key.local_name();
        if local.as_ref() == name.as_bytes() {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

/// Parse a TRX duration (`HH:MM:SS.fffffff`) into milliseconds.
///
/// Returns `None` for anything unparseable rather than failing the whole
/// report: a missing timing is not worth losing results over.
fn parse_duration_ms(raw: &str) -> Option<f64> {
    let mut parts = raw.trim().split(':').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    // Right-align so "SS.fff" and "MM:SS" also work.
    while parts.len() < 3 {
        parts.insert(0, "0");
    }
    let hours: f64 = parts[0].parse().ok()?;
    let minutes: f64 = parts[1].parse().ok()?;
    let seconds: f64 = parts[2].parse().ok()?;
    Some(((hours * 3600.0) + (minutes * 60.0) + seconds) * 1000.0)
}

/// Map a TRX outcome string onto our four-state model.
///
/// TRX has a dozen outcomes; anything that clearly ran and clearly failed maps
/// to `Failed`, anything that never ran maps to `Skipped`, and genuinely
/// ambiguous verdicts land in `Other` so they are visible without being
/// miscounted as failures.
fn parse_outcome(raw: &str) -> TestOutcome {
    match raw {
        "Passed" => TestOutcome::Passed,
        "Failed" | "Error" | "Timeout" | "Aborted" => TestOutcome::Failed,
        "NotExecuted" | "NotRunnable" | "Disconnected" => TestOutcome::Skipped,
        _ => TestOutcome::Other,
    }
}

/// Class name and owning assembly for one test, from `<TestDefinitions>`.
#[derive(Debug, Default, Clone)]
struct Definition {
    class_name: Option<String>,
    /// Assembly file name, used as the project label in the tree.
    assembly: Option<String>,
}

/// A result under construction. A stack of these handles `<InnerResults>`,
/// which data-driven tests use to nest one result per test case.
#[derive(Debug, Default)]
struct PartialResult {
    test_id: Option<String>,
    execution_id: Option<String>,
    test_name: String,
    outcome: TestOutcome,
    duration_ms: Option<f64>,
    stdout: Option<String>,
    message: Option<String>,
    stack_trace: Option<String>,
    /// Set when this result contained `<InnerResults>`. A data-driven test
    /// reports a parent that merely rolls up its rows, so emitting both would
    /// double-count every theory.
    has_children: bool,
}

/// Which element's text we are currently collecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextTarget {
    None,
    StdOut,
    Message,
    StackTrace,
}

/// Parse TRX content into a flat list of test cases plus a summary.
pub fn parse(xml: &str) -> Result<TestRunResult> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut definitions: HashMap<String, Definition> = HashMap::new();
    let mut cases: Vec<TestCase> = Vec::new();

    // In-progress state.
    let mut stack: Vec<PartialResult> = Vec::new();
    let mut text_target = TextTarget::None;
    let mut current_def_id: Option<String> = None;
    let mut current_def: Definition = Definition::default();
    let mut run_duration_ms: Option<f64> = None;
    // quick-xml validates that end tags match their start tags, but a document
    // simply truncated mid-element reaches EOF without complaint. Track depth
    // so a truncated report is reported as corrupt rather than silently
    // yielding a partial run that looks like a real one.
    let mut depth: i32 = 0;

    loop {
        match reader.read_event().context("malformed TRX XML")? {
            // Self-closing elements produce `Empty` with no matching `End`, so
            // a result that carries no output has to be completed here.
            Event::Empty(e) => {
                if e.local_name().as_ref() == b"UnitTestResult" {
                    // A self-closing result has no inner rows, so it is always
                    // a leaf. Its parent, if any, is now a rollup.
                    if let Some(parent) = stack.last_mut() {
                        parent.has_children = true;
                    }
                    cases.push(finish_case(read_result(&e)));
                } else {
                    handle_start(&e, &mut current_def_id, &mut current_def, &mut run_duration_ms);
                }
            }

            Event::Start(e) => {
                depth += 1;
                match e.local_name().as_ref() {
                    b"UnitTestResult" => stack.push(read_result(&e)),
                    b"StdOut" => text_target = TextTarget::StdOut,
                    b"Message" => text_target = TextTarget::Message,
                    b"StackTrace" => text_target = TextTarget::StackTrace,
                    _ => handle_start(
                        &e,
                        &mut current_def_id,
                        &mut current_def,
                        &mut run_duration_ms,
                    ),
                }
            }

            Event::Text(t) => {
                let text = t.unescape().unwrap_or_default().into_owned();
                append_text(&mut stack, text_target, &text);
            }

            Event::CData(t) => {
                let text = String::from_utf8_lossy(&t).into_owned();
                append_text(&mut stack, text_target, &text);
            }

            Event::End(e) => {
                depth -= 1;
                match e.local_name().as_ref() {
                    b"UnitTestResult" => {
                        if let Some(partial) = stack.pop() {
                            if let Some(parent) = stack.last_mut() {
                                parent.has_children = true;
                            }
                            // Drop rollup parents; their rows carry the detail.
                            if !partial.has_children {
                                cases.push(finish_case(partial));
                            }
                        }
                    }
                    b"UnitTest" => {
                        if let Some(id) = current_def_id.take() {
                            definitions.insert(id, std::mem::take(&mut current_def));
                        }
                    }
                    b"StdOut" | b"Message" | b"StackTrace" => text_target = TextTarget::None,
                    _ => {}
                }
            }

            Event::Eof => {
                anyhow::ensure!(depth == 0, "TRX report is truncated: {depth} element(s) unclosed");
                break;
            }
            _ => {}
        }
    }

    // Join in the class name and assembly now that both sections are read.
    for case in &mut cases {
        let Some(def) = case
            .suite
            .as_ref()
            .and_then(|id| definitions.get(id))
            .cloned()
        else {
            case.suite = None;
            continue;
        };
        case.project = def.assembly;
        case.suite = def.class_name;

        // Prefer a fully qualified name built from the class when the runner
        // reported only a bare method name.
        if let Some(suite) = &case.suite {
            if !case.full_name.contains('.') {
                case.full_name = format!("{suite}.{}", case.name);
            }
        }
    }

    let summary = TestSummary::from_cases(&cases);
    Ok(TestRunResult {
        summary,
        // Fall back to the sum of test durations when the run has no timings.
        duration_ms: run_duration_ms
            .or_else(|| Some(cases.iter().filter_map(|c| c.duration_ms).sum())),
        cases,
    })
}

/// Read a `<UnitTestResult>` element's attributes.
fn read_result(e: &BytesStart) -> PartialResult {
    PartialResult {
        test_id: attr(e, "testId"),
        execution_id: attr(e, "executionId"),
        test_name: attr(e, "testName").unwrap_or_default(),
        outcome: attr(e, "outcome")
            .map(|o| parse_outcome(&o))
            .unwrap_or(TestOutcome::Other),
        duration_ms: attr(e, "duration").and_then(|d| parse_duration_ms(&d)),
        ..Default::default()
    }
}

/// Handle the elements that contribute to test *definitions* and run timings.
///
/// Shared between `Start` and `Empty` because `<TestMethod>` and `<Times>` are
/// almost always self-closing while `<UnitTest>` is not.
fn handle_start(
    e: &BytesStart,
    current_def_id: &mut Option<String>,
    current_def: &mut Definition,
    run_duration_ms: &mut Option<f64>,
) {
    match e.local_name().as_ref() {
        b"UnitTest" => {
            *current_def_id = attr(e, "id");
            *current_def = Definition {
                class_name: None,
                assembly: attr(e, "storage").as_deref().map(assembly_name),
            };
        }
        b"TestMethod" => {
            current_def.class_name = attr(e, "className").map(|c| {
                // TRX often stores an assembly-qualified name; keep only the
                // type portion.
                c.split(',').next().unwrap_or(&c).trim().to_string()
            });
            // `codeBase` preserves the assembly's real casing, whereas the
            // `storage` attribute on `<UnitTest>` is written lowercased. Both
            // name the same file, so prefer the one that displays correctly.
            if let Some(code_base) = attr(e, "codeBase") {
                current_def.assembly = Some(assembly_name(&code_base));
            }
        }
        b"Times" => {
            if let (Some(start), Some(finish)) = (attr(e, "start"), attr(e, "finish")) {
                *run_duration_ms = duration_between(&start, &finish);
            }
        }
        _ => {}
    }
}

/// Append text to whichever field of the innermost result is being collected.
fn append_text(stack: &mut [PartialResult], target: TextTarget, text: &str) {
    if target == TextTarget::None || text.is_empty() {
        return;
    }
    let Some(current) = stack.last_mut() else {
        return;
    };
    let slot = match target {
        TextTarget::StdOut => &mut current.stdout,
        TextTarget::Message => &mut current.message,
        TextTarget::StackTrace => &mut current.stack_trace,
        TextTarget::None => unreachable!("guarded above"),
    };
    match slot {
        Some(existing) => existing.push_str(text),
        None => *slot = Some(text.to_string()),
    }
}

/// Convert a partial result into a case, stashing the `testId` in `suite` so
/// the definition join can find it.
fn finish_case(p: PartialResult) -> TestCase {
    let display = display_name(&p.test_name);

    TestCase {
        id: p
            .execution_id
            .or_else(|| p.test_id.clone())
            .unwrap_or_else(|| p.test_name.clone()),
        name: if display.is_empty() { p.test_name.clone() } else { display },
        full_name: p.test_name,
        // Temporarily the testId; replaced with the class name during the join.
        suite: p.test_id,
        project: None,
        outcome: p.outcome,
        duration_ms: p.duration_ms,
        message: p.message.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        stack_trace: p
            .stack_trace
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty()),
        stdout: p.stdout.map(|s| s.trim_end().to_string()).filter(|s| !s.is_empty()),
    }
}

/// Reduce a fully qualified test name to what the tree leaf should show.
///
/// Splitting on the last `.` is not enough on its own: a theory's arguments are
/// part of the name and routinely contain dots (`IsClose(value: 1.5)`), so the
/// argument list is set aside before taking the final name segment.
fn display_name(full: &str) -> String {
    let (head, args) = match full.find('(') {
        Some(i) => (&full[..i], &full[i..]),
        None => (full, ""),
    };
    let short = head.rsplit('.').next().unwrap_or(head);
    format!("{short}{args}")
}

/// Reduce an assembly path to a display name (`/x/y/My.Tests.dll` → `My.Tests`).
fn assembly_name(storage: &str) -> String {
    let file = storage
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(storage);
    file.strip_suffix(".dll")
        .or_else(|| file.strip_suffix(".exe"))
        .unwrap_or(file)
        .to_string()
}

/// Milliseconds between two ISO-8601 timestamps, without pulling in a date
/// library: TRX timestamps share a date almost always, and a wrong total here
/// costs nothing but a display value.
fn duration_between(start: &str, finish: &str) -> Option<f64> {
    let s = iso_time_of_day_ms(start)?;
    let f = iso_time_of_day_ms(finish)?;
    let delta = f - s;
    // Guard against a run that crossed midnight.
    Some(if delta < 0.0 { delta + 86_400_000.0 } else { delta })
}

/// Milliseconds since midnight for the time portion of an ISO-8601 timestamp.
fn iso_time_of_day_ms(ts: &str) -> Option<f64> {
    let time = ts.split('T').nth(1)?;
    // Trim any timezone suffix.
    let time = time
        .split(['+', 'Z'])
        .next()?
        // A negative offset is only a timezone if it follows the seconds.
        .rsplit_once('-')
        .map(|(head, _)| head)
        .unwrap_or(time);
    parse_duration_ms(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../fixtures/reports/sample.trx");
    /// A TRX produced by Microsoft.Testing.Platform rather than VSTest —
    /// captured from a real `--report-trx` run of MSTest 4 on the .NET 10 SDK.
    const MTP: &str = include_str!("../../fixtures/reports/mtp.trx");

    #[test]
    fn parses_a_report_written_by_microsoft_testing_platform() {
        // Both `dotnet test` paths emit TRX, and the whole design rests on one
        // parser serving both. MTP writes the same schema but is a separate
        // implementation, so it gets its own fixture rather than an assumption.
        let run = parse(MTP).expect("an MTP-written TRX should parse");

        assert_eq!(run.summary.total, 1);
        assert_eq!(run.summary.passed, 1);

        let case = &run.cases[0];
        assert_eq!(case.name, "TestMethod1");
        assert_eq!(case.full_name, "T.Test1.TestMethod1");
        assert_eq!(case.suite.as_deref(), Some("T.Test1"), "class name comes from TestDefinitions");
        assert_eq!(case.project.as_deref(), Some("T"), "assembly name, from the codeBase path");
        assert!(case.duration_ms.is_some_and(|d| d > 0.0));
    }

    #[test]
    fn parses_outcomes_and_counts() {
        let run = parse(SAMPLE).expect("sample TRX should parse");

        // Three plain tests plus the theory's two rows. The theory's own
        // rollup result is deliberately not counted.
        assert_eq!(run.summary.total, 5);
        assert_eq!(run.summary.passed, 3);
        assert_eq!(run.summary.failed, 1);
        assert_eq!(run.summary.skipped, 1);
    }

    #[test]
    fn drops_the_rollup_parent_of_a_data_driven_test() {
        let run = parse(SAMPLE).unwrap();
        // The bare theory name must not appear alongside its rows.
        assert!(!run
            .cases
            .iter()
            .any(|c| c.full_name == "Calc.Tests.NumberTests.IsEven"));
    }

    #[test]
    fn keeps_theory_arguments_in_the_display_name() {
        assert_eq!(display_name("N.C.IsEven(value: 2)"), "IsEven(value: 2)");
        // Arguments routinely contain dots, which a naive rsplit would cut on.
        assert_eq!(display_name("N.C.IsClose(value: 1.5)"), "IsClose(value: 1.5)");
        assert_eq!(display_name("N.C.Plain"), "Plain");
    }

    #[test]
    fn joins_class_name_and_assembly_from_test_definitions() {
        let run = parse(SAMPLE).unwrap();
        let case = run
            .cases
            .iter()
            .find(|c| c.name == "Adds")
            .expect("Adds case");

        assert_eq!(case.suite.as_deref(), Some("Calc.Tests.CalculatorTests"));
        assert_eq!(case.project.as_deref(), Some("Calc.Tests"));
        assert_eq!(case.full_name, "Calc.Tests.CalculatorTests.Adds");
    }

    #[test]
    fn captures_failure_message_and_stack_trace() {
        let run = parse(SAMPLE).unwrap();
        let failed = run
            .cases
            .iter()
            .find(|c| c.outcome == TestOutcome::Failed)
            .expect("a failing case");

        assert_eq!(failed.name, "Subtracts");
        assert!(failed.message.as_ref().unwrap().contains("Assert.Equal() Failure"));
        assert!(failed
            .stack_trace
            .as_ref()
            .unwrap()
            .contains("CalculatorTests.Subtracts()"));
    }

    #[test]
    fn captures_stdout_for_a_passing_test() {
        let run = parse(SAMPLE).unwrap();
        let case = run.cases.iter().find(|c| c.name == "Adds").unwrap();
        assert_eq!(case.stdout.as_deref(), Some("computing 1 + 1"));
    }

    #[test]
    fn flattens_data_driven_inner_results() {
        let run = parse(SAMPLE).unwrap();
        // The theory itself plus its two rows are all reported.
        let rows: Vec<_> = run
            .cases
            .iter()
            .filter(|c| c.full_name.contains("IsEven"))
            .collect();
        assert_eq!(rows.len(), 2, "both theory rows should appear as cases");
    }

    #[test]
    fn parses_durations_in_milliseconds() {
        assert_eq!(parse_duration_ms("00:00:01.5000000"), Some(1500.0));
        assert_eq!(parse_duration_ms("00:01:00.0000000"), Some(60_000.0));
        assert_eq!(parse_duration_ms("01:00:00.0000000"), Some(3_600_000.0));
        assert_eq!(parse_duration_ms("not a duration"), None);
    }

    #[test]
    fn maps_trx_outcome_vocabulary() {
        assert_eq!(parse_outcome("Passed"), TestOutcome::Passed);
        assert_eq!(parse_outcome("Failed"), TestOutcome::Failed);
        assert_eq!(parse_outcome("Timeout"), TestOutcome::Failed);
        assert_eq!(parse_outcome("NotExecuted"), TestOutcome::Skipped);
        assert_eq!(parse_outcome("Inconclusive"), TestOutcome::Other);
    }

    #[test]
    fn strips_assembly_qualification_from_class_names() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <Results>
    <UnitTestResult executionId="e1" testId="t1" testName="N.C.M" outcome="Passed" duration="00:00:00.0010000" />
  </Results>
  <TestDefinitions>
    <UnitTest name="M" storage="/build/N.Tests.dll" id="t1">
      <TestMethod className="N.C, N.Tests, Version=1.0.0.0" name="M" />
    </UnitTest>
  </TestDefinitions>
</TestRun>"#;
        let run = parse(xml).unwrap();
        assert_eq!(run.cases[0].suite.as_deref(), Some("N.C"));
    }

    #[test]
    fn empty_report_parses_to_an_empty_run() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <Results />
  <TestDefinitions />
</TestRun>"#;
        let run = parse(xml).unwrap();
        assert_eq!(run.summary.total, 0);
        assert!(run.cases.is_empty());
    }

    #[test]
    fn malformed_xml_is_an_error_not_a_panic() {
        assert!(parse("<TestRun><Results>").is_err());
    }
}
