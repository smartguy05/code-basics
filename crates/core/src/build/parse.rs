//! Pure parser for MSBuild file-logger output.
//!
//! The build is run with `-flp1:errorsOnly;logfile=…` and
//! `-flp2:warningsOnly;logfile=…` under `/p:GenerateFullPaths=true`, which emit
//! diagnostics in one canonical shape:
//!
//! ```text
//! C:\src\Foo\Bar.cs(12,9): error CS0103: The name 'x' does not exist [C:\src\Foo\Foo.csproj]
//! ```
//!
//! The location paren may be `(line)`, `(line,col)` or a span
//! `(line,col,endLine,endCol)`; the trailing `[project]` is present only when
//! MSBuild knew the owning project. Anything that does not match — banners,
//! restore chatter — is preserved in [`BuildReport::warnings`], never dropped
//! and never guessed into a diagnostic. A multi-line message is written by
//! MSBuild as an indented continuation on the following lines; those are
//! appended to the current diagnostic's message rather than treated as noise.

use std::sync::OnceLock;

use regex::Regex;

use super::model::{BuildDiagnostic, BuildReport, BuildSeverity, BuildStatus};

/// The canonical diagnostic line.
///
/// Compiled once. The message is captured greedily and the trailing
/// `[project]` split off afterwards, because a message can itself contain
/// brackets and a non-greedy split would bind the wrong one.
fn line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            ^
            (?P<file>.+?)                       # file, up to the location paren
            \(
                (?P<line>\d+)                   # 1-based line
                (?:,(?P<col>\d+))?              # optional 1-based column
                (?:,\d+,\d+)?                   # optional span end (ignored)
            \)
            \s*:\s*
            (?P<sev>error|warning)              # severity keyword
            \s+
            (?P<code>[^\s:]+)                   # diagnostic code
            \s*:\s*
            (?P<rest>.*)                        # message [+ optional [project]]
            $
            ",
        )
        .expect("build diagnostic regex is valid")
    })
}

/// Parse a single canonical MSBuild diagnostic line, or `None` if it does not
/// match the shape.
pub fn parse_diagnostic_line(line: &str) -> Option<BuildDiagnostic> {
    let caps = line_re().captures(line.trim_end())?;

    let severity = match &caps["sev"] {
        "error" => BuildSeverity::Error,
        "warning" => BuildSeverity::Warning,
        // Unreachable: the regex alternation only matches these two.
        _ => return None,
    };

    let line_no: u32 = caps["line"].parse().ok()?;
    let column: Option<u32> = caps.name("col").and_then(|m| m.as_str().parse().ok());

    let (message, project) = split_project(caps["rest"].trim());

    Some(BuildDiagnostic {
        file: caps["file"].trim().to_string(),
        line: line_no,
        column,
        code: caps["code"].to_string(),
        severity,
        message,
        project,
    })
}

/// Split a trailing ` [project]` off a message. MSBuild appends the owning
/// project in brackets at the very end; a message with no such suffix keeps its
/// own trailing bracket.
fn split_project(rest: &str) -> (String, Option<String>) {
    if let Some(inner) = rest.strip_suffix(']') {
        if let Some(open) = inner.rfind('[') {
            // Require the bracket to be its own trailing token (preceded by a
            // space), so a message ending in `foo[0]` is not mistaken for a
            // project.
            if open > 0 && inner.as_bytes()[open - 1] == b' ' {
                let project = inner[open + 1..].to_string();
                let message = inner[..open].trim_end().to_string();
                return (message, Some(project));
            }
        }
    }
    (rest.to_string(), None)
}

/// True for a line that continues the previous diagnostic's message: a
/// non-empty line that is indented (MSBuild indents wrapped message lines) and
/// is not itself a canonical diagnostic.
fn is_continuation(raw: &str) -> bool {
    let trimmed = raw.trim_end();
    !trimmed.is_empty()
        && raw.starts_with(|c: char| c == ' ' || c == '\t')
        && parse_diagnostic_line(trimmed).is_none()
}

/// Parse one file-logger artifact, appending diagnostics and preserving any
/// unrecognised line in `warnings` (unless it continues the prior message).
fn parse_artifact(text: &str, diagnostics: &mut Vec<BuildDiagnostic>, warnings: &mut Vec<String>) {
    // Index into `diagnostics` of the row a continuation line extends. Only a
    // diagnostic added by *this* artifact can be continued, so it is reset per
    // call rather than carried across the errors/warnings boundary.
    let mut current: Option<usize> = None;

    for raw in text.lines() {
        let trimmed = raw.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(diag) = parse_diagnostic_line(trimmed) {
            diagnostics.push(diag);
            current = Some(diagnostics.len() - 1);
            continue;
        }

        if let Some(idx) = current {
            if is_continuation(raw) {
                let message = &mut diagnostics[idx].message;
                message.push('\n');
                message.push_str(raw.trim());
                continue;
            }
        }

        // Neither a diagnostic nor a continuation: preserve it verbatim.
        current = None;
        warnings.push(trimmed.to_string());
    }
}

/// Build a [`BuildReport`] from the two file-logger artifacts and the process
/// exit success.
///
/// This can only ever produce the three statuses a completed build's artifacts
/// justify — [`BuildStatus::Failed`], [`BuildStatus::SucceededWithWarnings`]
/// and [`BuildStatus::SucceededClean`]. The other three
/// ([`BuildStatus::NeverBuilt`], [`BuildStatus::Building`],
/// [`BuildStatus::CouldNotStart`]) describe state no artifact can express and
/// are supplied by the app layer via [`BuildReport::of_status`].
pub fn parse_report(errors: &str, warnings_text: &str, exit_success: bool) -> BuildReport {
    let mut diagnostics = Vec::new();
    let mut warnings = Vec::new();

    parse_artifact(errors, &mut diagnostics, &mut warnings);
    parse_artifact(warnings_text, &mut diagnostics, &mut warnings);

    let has_error = diagnostics
        .iter()
        .any(|d| d.severity == BuildSeverity::Error);
    let has_warning = diagnostics
        .iter()
        .any(|d| d.severity == BuildSeverity::Warning);

    // A non-zero exit is authoritative even when no error line parsed: a build
    // can fail for a reason the file logger did not phrase as a diagnostic
    // (a nuget restore failure, an MSBuild task crash). Collapsing that into
    // "clean" would be the guess the abstain philosophy refuses.
    let status = if has_error || !exit_success {
        BuildStatus::Failed
    } else if has_warning {
        BuildStatus::SucceededWithWarnings
    } else {
        BuildStatus::SucceededClean
    };

    BuildReport {
        status,
        diagnostics,
        warnings,
    }
}
