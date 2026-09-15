use super::*;

use crate::build::{BuildDiagnostic, BuildReport, BuildSeverity, BuildStatus};

fn error_row(code: &str, line: u32, column: Option<u32>) -> BuildDiagnostic {
    BuildDiagnostic {
        file: r"C:\code\repo\src\App.cs".to_string(),
        line,
        column,
        code: code.to_string(),
        severity: BuildSeverity::Error,
        message: "the name does not exist".to_string(),
        project: Some("App.csproj".to_string()),
    }
}

fn warning_row(code: &str) -> BuildDiagnostic {
    BuildDiagnostic {
        severity: BuildSeverity::Warning,
        code: code.to_string(),
        message: "unused variable".to_string(),
        project: None,
        ..error_row(code, 3, Some(9))
    }
}

fn report(status: BuildStatus, diagnostics: Vec<BuildDiagnostic>) -> BuildReport {
    BuildReport {
        status,
        diagnostics,
        warnings: Vec::new(),
    }
}

// --- get_errors -----------------------------------------------------------

#[test]
fn errors_before_any_build_is_a_never_built_refusal_not_an_empty_success() {
    let answer = errors(&BuildReport::of_status(BuildStatus::NeverBuilt));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("neverBuilt"));
}

#[test]
fn errors_after_a_build_that_could_not_start_is_its_own_refusal() {
    let answer = errors(&BuildReport::of_status(BuildStatus::CouldNotStart));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("buildFailedToStart"));
}

#[test]
fn a_clean_build_reports_no_errors_as_data() {
    let answer = errors(&report(BuildStatus::SucceededClean, vec![]));
    assert!(answer.ok);
    assert!(answer.text.contains("No errors"), "{}", answer.text);
}

#[test]
fn errors_lists_only_the_errors_with_file_line_and_column() {
    let answer = errors(&report(
        BuildStatus::Failed,
        vec![error_row("CS0103", 12, Some(5)), warning_row("CS0219")],
    ));
    assert!(answer.ok);
    assert!(answer.text.contains("1 error(s):"), "{}", answer.text);
    assert!(answer.text.contains("CS0103"), "{}", answer.text);
    assert!(answer.text.contains("App.cs:12:5"), "{}", answer.text);
    assert!(
        !answer.text.contains("CS0219"),
        "warnings must not appear in errors: {}",
        answer.text
    );
}

#[test]
fn a_diagnostic_without_a_column_omits_it_rather_than_inventing_one() {
    let answer = errors(&report(
        BuildStatus::Failed,
        vec![error_row("CS0103", 12, None)],
    ));
    assert!(answer.text.contains("App.cs:12"), "{}", answer.text);
    assert!(!answer.text.contains("App.cs:12:"), "{}", answer.text);
}

// --- get_warnings ---------------------------------------------------------

#[test]
fn warnings_lists_only_the_warnings() {
    let answer = warnings(&report(
        BuildStatus::SucceededWithWarnings,
        vec![error_row("CS0103", 1, Some(1)), warning_row("CS0219")],
    ));
    assert!(answer.ok);
    assert!(answer.text.contains("1 warning(s):"), "{}", answer.text);
    assert!(answer.text.contains("CS0219"), "{}", answer.text);
    assert!(!answer.text.contains("CS0103"), "{}", answer.text);
}

#[test]
fn warnings_before_any_build_is_a_never_built_refusal() {
    let answer = warnings(&BuildReport::of_status(BuildStatus::NeverBuilt));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("neverBuilt"));
}

// --- get_build_status -----------------------------------------------------

#[test]
fn status_renders_never_built_as_data_not_a_refusal() {
    // Asking for the status is exactly the question never-built answers.
    let answer = status(&BuildReport::of_status(BuildStatus::NeverBuilt));
    assert!(answer.ok, "status of never-built is data, not a refusal");
    assert!(answer.text.contains("No build has run"), "{}", answer.text);
}

#[test]
fn status_distinguishes_the_six_states() {
    let texts: std::collections::BTreeSet<String> = [
        BuildStatus::NeverBuilt,
        BuildStatus::Building,
        BuildStatus::SucceededClean,
        BuildStatus::SucceededWithWarnings,
        BuildStatus::Failed,
        BuildStatus::CouldNotStart,
    ]
    .into_iter()
    .map(|s| status(&BuildReport::of_status(s)).text)
    .collect();
    assert_eq!(texts.len(), 6, "each status must read differently");
}

#[test]
fn a_failed_status_reports_the_error_and_warning_counts() {
    let answer = status(&report(
        BuildStatus::Failed,
        vec![error_row("CS0103", 1, Some(1)), warning_row("CS0219")],
    ));
    assert!(answer.text.contains("1 error(s)"), "{}", answer.text);
    assert!(answer.text.contains("1 warning(s)"), "{}", answer.text);
}

// --- build_solution -------------------------------------------------------

#[test]
fn build_outcome_refuses_when_the_build_could_not_start() {
    let answer = build_outcome(&BuildReport::of_status(BuildStatus::CouldNotStart));
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("buildFailedToStart"));
}

#[test]
fn build_outcome_summarises_a_successful_build() {
    let answer = build_outcome(&report(BuildStatus::SucceededClean, vec![]));
    assert!(answer.ok);
    assert!(answer.text.contains("succeeded"), "{}", answer.text);
}

// --- parser abstentions ---------------------------------------------------

#[test]
fn unparsed_lines_are_noted_rather_than_dropped_silently() {
    let mut report = report(BuildStatus::SucceededClean, vec![]);
    report
        .warnings
        .push("some line MSBuild wrote we could not read".to_string());
    let answer = errors(&report);
    assert!(
        answer.text.contains("could not be parsed"),
        "{}",
        answer.text
    );
}
