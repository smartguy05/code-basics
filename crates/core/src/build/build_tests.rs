//! Tests for the build-diagnostics parser and model.
//!
//! Fixtures are canonical MSBuild file-logger strings — the exact shape
//! `dotnet build -flp1:errorsOnly;logfile=…` and `-flp2:warningsOnly;logfile=…`
//! emit under `/p:GenerateFullPaths=true`. The two artifacts are parsed
//! together with the process exit success into one [`BuildReport`].

use super::model::{BuildDiagnostic, BuildReport, BuildSeverity, BuildStatus};
use super::parse::{parse_diagnostic_line, parse_report};

// -- fixtures ---------------------------------------------------------------

/// One error, as the errors-only artifact writes it.
const ERRORS_ONLY: &str = "C:\\src\\Foo\\Bar.cs(12,9): error CS0103: The name 'x' does not exist in the current context [C:\\src\\Foo\\Foo.csproj]\r\n";

/// One warning, as the warnings-only artifact writes it.
const WARNINGS_ONLY: &str = "C:\\src\\Foo\\Bar.cs(3,1): warning CS0219: The variable 'y' is assigned but never used [C:\\src\\Foo\\Foo.csproj]\r\n";

// -- status: the three a pure parse can produce -----------------------------

#[test]
fn a_clean_build_with_empty_artifacts_and_success_is_succeeded_clean() {
    let report = parse_report("", "", true);
    assert_eq!(report.status, BuildStatus::SucceededClean);
    assert!(report.diagnostics.is_empty());
    assert!(report.warnings.is_empty());
}

#[test]
fn warnings_only_is_succeeded_with_warnings() {
    let report = parse_report("", WARNINGS_ONLY, true);
    assert_eq!(report.status, BuildStatus::SucceededWithWarnings);
    assert_eq!(report.warning_diagnostics().count(), 1);
    assert_eq!(report.errors().count(), 0);
}

#[test]
fn errors_present_is_failed_with_parsed_diagnostics() {
    let report = parse_report(ERRORS_ONLY, "", false);
    assert_eq!(report.status, BuildStatus::Failed);

    let errors: Vec<&BuildDiagnostic> = report.errors().collect();
    assert_eq!(errors.len(), 1, "one error should be parsed");
    let e = errors[0];
    assert_eq!(e.file, "C:\\src\\Foo\\Bar.cs");
    assert_eq!(e.line, 12);
    assert_eq!(e.column, Some(9));
    assert_eq!(e.code, "CS0103");
    assert_eq!(e.severity, BuildSeverity::Error);
    assert_eq!(
        e.message,
        "The name 'x' does not exist in the current context"
    );
    assert_eq!(e.project.as_deref(), Some("C:\\src\\Foo\\Foo.csproj"));
}

/// A non-zero exit with no parseable error line is still a failure: the exit
/// code is authoritative, and collapsing it into "clean" would be the guess
/// the abstain philosophy refuses.
#[test]
fn a_failing_exit_with_no_diagnostics_is_still_failed() {
    let report = parse_report("", "", false);
    assert_eq!(report.status, BuildStatus::Failed);
}

// -- the abstain rule: unparseable lines are preserved, never guessed -------

#[test]
fn an_unparseable_line_lands_in_warnings_not_diagnostics() {
    let junk = "MSBuild version 17.0 for .NET\r\nBuild started.\r\n";
    let report = parse_report(junk, "", true);

    assert!(
        report.diagnostics.is_empty(),
        "no diagnostic should be invented from noise"
    );
    assert_eq!(report.warnings.len(), 2, "both noise lines are preserved");
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("MSBuild version")));
    assert!(report.warnings.iter().any(|w| w.contains("Build started")));
}

// -- single-line parser ------------------------------------------------------

#[test]
fn parses_a_canonical_error_line() {
    let d = parse_diagnostic_line(
        "C:\\src\\Foo\\Bar.cs(12,9): error CS0103: The name 'x' does not exist [C:\\src\\Foo\\Foo.csproj]",
    )
    .expect("canonical error line should parse");
    assert_eq!(d.file, "C:\\src\\Foo\\Bar.cs");
    assert_eq!(d.line, 12);
    assert_eq!(d.column, Some(9));
    assert_eq!(d.code, "CS0103");
    assert_eq!(d.severity, BuildSeverity::Error);
    assert_eq!(d.project.as_deref(), Some("C:\\src\\Foo\\Foo.csproj"));
}

#[test]
fn parses_a_line_with_no_column() {
    let d = parse_diagnostic_line("/src/Foo.cs(7): warning CS0168: unused [Foo.csproj]")
        .expect("line-only location should parse");
    assert_eq!(d.line, 7);
    assert_eq!(d.column, None);
    assert_eq!(d.severity, BuildSeverity::Warning);
}

#[test]
fn parses_a_line_with_no_project() {
    let d = parse_diagnostic_line("/src/Foo.cs(7,3): error CS0103: nope")
        .expect("a projectless diagnostic should still parse");
    assert_eq!(d.project, None);
    assert_eq!(d.message, "nope");
    assert_eq!(d.column, Some(3));
}

#[test]
fn a_span_location_keeps_the_starting_line_and_column() {
    let d = parse_diagnostic_line("/src/Foo.cs(7,3,7,10): error CS0103: nope")
        .expect("a span location should parse");
    assert_eq!(d.line, 7);
    assert_eq!(d.column, Some(3));
}

#[test]
fn a_non_diagnostic_line_does_not_parse() {
    assert!(parse_diagnostic_line("Build succeeded.").is_none());
    assert!(parse_diagnostic_line("").is_none());
    assert!(parse_diagnostic_line("   Determining projects to restore...").is_none());
}

/// Multi-line messages: MSBuild writes the continuation on following indented
/// lines that do not match the canonical shape. They are appended to the
/// current diagnostic's message rather than dumped into `warnings`.
#[test]
fn an_indented_continuation_extends_the_previous_message() {
    let text =
        "/src/Foo.cs(1,1): error CS9999: first line\r\n    second line\r\n    third line\r\n";
    let report = parse_report(text, "", false);

    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.warnings.is_empty(), "continuations are not warnings");
    assert_eq!(
        report.diagnostics[0].message,
        "first line\nsecond line\nthird line"
    );
}

// -- IPC contract: exact JSON keys ------------------------------------------

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

#[test]
fn build_diagnostic_serialises_with_the_keys_the_ui_reads() {
    let d = BuildDiagnostic {
        file: "Bar.cs".into(),
        line: 1,
        column: Some(2),
        code: "CS0103".into(),
        severity: BuildSeverity::Error,
        message: "msg".into(),
        project: Some("Foo.csproj".into()),
    };
    assert_eq!(
        keys(&serde_json::to_value(&d).unwrap()),
        ["code", "column", "file", "line", "message", "project", "severity"]
    );
}

#[test]
fn absent_column_and_project_serialise_as_null_not_omitted() {
    // The UI (types.ts: `column: number | null`, `project: string | null`) relies on
    // an honest "no column" crossing as JSON `null`, distinct from the field being
    // missing because the backend forgot it. A `skip_serializing_if` on either field
    // would silently break that distinction with the whole build:: suite still green,
    // so pin it here.
    let d = BuildDiagnostic {
        file: "Bar.cs".into(),
        line: 1,
        column: None,
        code: "CS0103".into(),
        severity: BuildSeverity::Error,
        message: "msg".into(),
        project: None,
    };
    let value = serde_json::to_value(&d).unwrap();
    assert_eq!(
        keys(&value),
        ["code", "column", "file", "line", "message", "project", "severity"]
    );
    assert!(value["column"].is_null());
    assert!(value["project"].is_null());
}

#[test]
fn build_report_serialises_with_the_keys_the_ui_reads() {
    let report = BuildReport::of_status(BuildStatus::NeverBuilt);
    assert_eq!(
        keys(&serde_json::to_value(&report).unwrap()),
        ["diagnostics", "status", "warnings"]
    );
}

#[test]
fn build_enums_serialise_in_camel_case() {
    assert_eq!(
        serde_json::to_string(&BuildSeverity::Error).unwrap(),
        "\"error\""
    );
    assert_eq!(
        serde_json::to_string(&BuildSeverity::Warning).unwrap(),
        "\"warning\""
    );
    // Every one of the six status answers has its own distinct wire spelling.
    assert_eq!(
        serde_json::to_string(&BuildStatus::NeverBuilt).unwrap(),
        "\"neverBuilt\""
    );
    assert_eq!(
        serde_json::to_string(&BuildStatus::Building).unwrap(),
        "\"building\""
    );
    assert_eq!(
        serde_json::to_string(&BuildStatus::SucceededClean).unwrap(),
        "\"succeededClean\""
    );
    assert_eq!(
        serde_json::to_string(&BuildStatus::SucceededWithWarnings).unwrap(),
        "\"succeededWithWarnings\""
    );
    assert_eq!(
        serde_json::to_string(&BuildStatus::Failed).unwrap(),
        "\"failed\""
    );
    assert_eq!(
        serde_json::to_string(&BuildStatus::CouldNotStart).unwrap(),
        "\"couldNotStart\""
    );
}

#[test]
fn a_report_round_trips_through_json() {
    let report = parse_report(ERRORS_ONLY, WARNINGS_ONLY, false);
    let json = serde_json::to_string(&report).unwrap();
    assert_eq!(serde_json::from_str::<BuildReport>(&json).unwrap(), report);
}
