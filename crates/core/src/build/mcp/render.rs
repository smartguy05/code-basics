//! Turning a [`crate::build::BuildReport`] into the words an agent reads.
//!
//! The application runs and caches the build and hands the report here; this
//! module — pure, in the core crate — turns it into a [`super::wire::ToolAnswer`].
//! The runner and the app-side dispatch decide nothing about the prose, exactly as
//! [`crate::roslyn::render`] keeps the wording out of the shim.
//!
//! # The rule that shapes every function here
//!
//! **An answer must never let an absence pass for a fact about the build.** The six
//! [`crate::build::BuildStatus`] variants stay six answers:
//!
//! * [`get_build_status`] renders *every* status as data, including never-built —
//!   asking for the status is exactly the question never-built answers.
//! * [`errors`] and [`warnings`] *refuse* [`BuildStatus::NeverBuilt`] and
//!   [`BuildStatus::CouldNotStart`] ([`super::answer::BuildRefusal`]), because a
//!   read of results that do not exist is not an empty success.
//! * A genuine `SucceededClean` with no errors *is* data — "no errors" — and is
//!   rendered as such.
//! * The parser's own abstentions ([`crate::build::BuildReport::warnings`]) are
//!   noted whenever present, because a diagnostic list that silently dropped lines
//!   it could not read would look complete and not be.

use crate::build::{BuildDiagnostic, BuildReport, BuildSeverity, BuildStatus};

use super::answer::BuildRefusal;
use super::wire::ToolAnswer;

/// The `build_solution` answer: what the build just did.
pub fn build_outcome(report: &BuildReport) -> ToolAnswer {
    if report.status == BuildStatus::CouldNotStart {
        let refusal = BuildRefusal::BuildFailedToStart;
        return ToolAnswer::refused(refusal.code(), refusal.sentence());
    }
    ToolAnswer::ok(with_notes(status_summary(report), report))
}

/// The `get_build_status` answer: every status is data here.
pub fn status(report: &BuildReport) -> ToolAnswer {
    ToolAnswer::ok(with_notes(status_summary(report), report))
}

/// The `get_errors` answer.
pub fn errors(report: &BuildReport) -> ToolAnswer {
    if let Some(refusal) = BuildRefusal::from_status(report.status) {
        return ToolAnswer::refused(refusal.code(), refusal.sentence());
    }
    if report.status == BuildStatus::Building {
        return ToolAnswer::ok(with_notes(
            "A build is currently in progress, so its results are not final yet.".to_string(),
            report,
        ));
    }
    let rows: Vec<&BuildDiagnostic> = report.errors().collect();
    if rows.is_empty() {
        return ToolAnswer::ok(with_notes(
            "No errors: the most recent build reported no errors.".to_string(),
            report,
        ));
    }
    ToolAnswer::ok(with_notes(diagnostic_list("error", &rows), report))
}

/// The `get_warnings` answer.
pub fn warnings(report: &BuildReport) -> ToolAnswer {
    if let Some(refusal) = BuildRefusal::from_status(report.status) {
        return ToolAnswer::refused(refusal.code(), refusal.sentence());
    }
    if report.status == BuildStatus::Building {
        return ToolAnswer::ok(with_notes(
            "A build is currently in progress, so its results are not final yet.".to_string(),
            report,
        ));
    }
    let rows: Vec<&BuildDiagnostic> = report.warning_diagnostics().collect();
    if rows.is_empty() {
        return ToolAnswer::ok(with_notes(
            "No warnings: the most recent build reported no warnings.".to_string(),
            report,
        ));
    }
    ToolAnswer::ok(with_notes(diagnostic_list("warning", &rows), report))
}

/// A one-line summary of the status, with counts where they apply.
fn status_summary(report: &BuildReport) -> String {
    let errors = report.errors().count();
    let warnings = report.warning_diagnostics().count();
    match report.status {
        BuildStatus::NeverBuilt => "No build has run for this workspace yet.".to_string(),
        BuildStatus::Building => "A build is currently in progress.".to_string(),
        BuildStatus::SucceededClean => {
            "The most recent build succeeded with no errors and no warnings.".to_string()
        }
        BuildStatus::SucceededWithWarnings => {
            format!("The most recent build succeeded with {warnings} warning(s) and no errors.")
        }
        BuildStatus::Failed => {
            format!("The most recent build failed: {errors} error(s) and {warnings} warning(s).")
        }
        BuildStatus::CouldNotStart => {
            "The most recent build could not be started at all.".to_string()
        }
    }
}

/// A heading plus one line per diagnostic.
fn diagnostic_list(word: &str, rows: &[&BuildDiagnostic]) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format!("{} {word}(s):", rows.len()));
    for row in rows {
        lines.push(diagnostic_line(row));
    }
    lines.join("\n")
}

fn diagnostic_line(row: &BuildDiagnostic) -> String {
    let severity = match row.severity {
        BuildSeverity::Error => "error",
        BuildSeverity::Warning => "warning",
    };
    let location = match row.column {
        Some(column) => format!("{}:{}:{}", row.file, row.line, column),
        None => format!("{}:{}", row.file, row.line),
    };
    let mut line = format!(
        "  {severity} {} {location}  {}",
        row.code,
        row.message.trim()
    );
    if let Some(project) = &row.project {
        if !project.trim().is_empty() {
            line.push_str(&format!("  [{}]", project.trim()));
        }
    }
    line
}

/// Append a note about any lines the parser could not interpret — never dropped
/// silently.
fn with_notes(body: String, report: &BuildReport) -> String {
    if report.warnings.is_empty() {
        return body;
    }
    format!(
        "{body}\nNote: {} line(s) of build output could not be parsed and were not interpreted \
         as diagnostics.",
        report.warnings.len()
    )
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
