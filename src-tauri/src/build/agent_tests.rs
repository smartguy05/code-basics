//! The one decision the Build host makes without a running build: turning a cached
//! report (or its absence) into a read tool's answer, and gating on the feature.
//!
//! `build_solution` runs a real `dotnet build` and so is not reachable from a test;
//! everything a test *can* pin lives in [`read_answer`].

use super::*;
use cb_core::build::{BuildDiagnostic, BuildReport, BuildSeverity, BuildStatus};

fn errored(file: &str) -> BuildReport {
    BuildReport {
        status: BuildStatus::Failed,
        diagnostics: vec![BuildDiagnostic {
            file: file.into(),
            line: 7,
            column: Some(2),
            code: "CS0103".into(),
            severity: BuildSeverity::Error,
            message: "The name 'Foo' does not exist".into(),
            project: None,
        }],
        warnings: Vec::new(),
    }
}

#[test]
fn the_feature_being_off_refuses_every_read_with_the_disabled_code() {
    for tool in [ReadTool::Errors, ReadTool::Warnings, ReadTool::Status] {
        let answer = read_answer(false, Some(errored("/a/Foo.cs")), tool);
        assert!(!answer.ok, "a disabled tool is a refusal");
        assert_eq!(
            answer.code.as_deref(),
            Some(cb_core::tool_gate::DISABLED_CODE)
        );
    }
}

#[test]
fn get_errors_before_any_build_refuses_never_built_not_an_empty_success() {
    // An absent cache is NeverBuilt — a distinct answer — so get_errors refuses
    // rather than reporting "no errors".
    let answer = read_answer(true, None, ReadTool::Errors);
    assert!(!answer.ok);
    assert_eq!(answer.code.as_deref(), Some("neverBuilt"));
}

#[test]
fn get_build_status_before_any_build_reports_never_built_as_data() {
    // Asking for the status is exactly the question never-built answers, so this is
    // a successful answer carrying the state, not a refusal.
    let answer = read_answer(true, None, ReadTool::Status);
    assert!(answer.ok);
    assert!(answer.text.to_lowercase().contains("no build has run"));
}

#[test]
fn get_errors_lists_the_cached_errors() {
    let answer = read_answer(true, Some(errored("/a/Foo.cs")), ReadTool::Errors);
    assert!(answer.ok);
    assert!(answer.text.contains("CS0103"));
    assert!(answer.text.contains("/a/Foo.cs"));
}

#[test]
fn get_warnings_on_a_clean_build_says_no_warnings_rather_than_refusing() {
    let clean = BuildReport::of_status(BuildStatus::SucceededClean);
    let answer = read_answer(true, Some(clean), ReadTool::Warnings);
    assert!(answer.ok, "a clean build is data — a genuine 'no warnings'");
    assert!(answer.text.to_lowercase().contains("no warnings"));
}
