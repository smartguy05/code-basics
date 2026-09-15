//! Types for the build-diagnostics feature.
//!
//! These mirror the shape of [`crate::model`]'s test types: a parser reads an
//! artifact the tool wrote (here the MSBuild file-logger output) into typed
//! rows the UI and an agent can act on directly. `src/ipc/types.ts` mirrors
//! these by hand, so [`super::build_tests`] pins the exact JSON keys.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Whether a diagnostic is an error or a warning.
///
/// The severity is read from the diagnostic line itself (`error CS0103` /
/// `warning CS0219`), which is authoritative regardless of which file-logger
/// artifact the line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BuildSeverity {
    Error,
    Warning,
}

/// One structured build diagnostic, parsed from a canonical MSBuild
/// file-logger line: `PATH(line,col): severity CODE: message [project]`.
///
/// `column` and `project` are optional because MSBuild omits them — a
/// diagnostic without a precise column reports `(line)` alone, and one raised
/// outside any project (or before the project is known) carries no `[project]`
/// suffix. Both cross as `null` rather than absent, so the UI can tell an
/// honest "no column" from a parser that forgot the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BuildDiagnostic {
    /// File the diagnostic points at. Absolute when the build was run with
    /// `/p:GenerateFullPaths=true`, as this feature always does.
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column, when MSBuild reported one.
    pub column: Option<u32>,
    /// Diagnostic code, e.g. `CS0103`, `MSB3021`, `NU1605`.
    pub code: String,
    pub severity: BuildSeverity,
    /// Human-readable message. May span several source lines when MSBuild wrote
    /// a multi-line message; the continuation lines are joined with newlines.
    pub message: String,
    /// Owning project, from the trailing `[...]`, when present.
    pub project: Option<String>,
}

/// The state of the most recent build for a workspace.
///
/// The abstain philosophy applies to status exactly as it does to the LSP
/// six-state `Availability` and the DAP `DebugState`: *never built*,
/// *building*, *succeeded clean*, *succeeded with warnings*, *failed* and
/// *build could not start* are six distinct answers and must never collapse
/// into one. A pure parse of the artifacts can only ever produce the middle
/// three ([`Self::SucceededClean`], [`Self::SucceededWithWarnings`],
/// [`Self::Failed`]); the app layer supplies the other three from state the
/// artifacts cannot describe — that no build has run, that one is in progress,
/// or that the process never started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BuildStatus {
    /// No build has been run for this workspace yet.
    NeverBuilt,
    /// A build is currently in progress.
    Building,
    /// The build succeeded with no warnings and no errors.
    SucceededClean,
    /// The build succeeded but produced one or more warnings.
    SucceededWithWarnings,
    /// The build failed: it reported errors, or the process exited non-zero.
    Failed,
    /// The build process could not be started at all (e.g. `dotnet` missing).
    CouldNotStart,
}

/// The parsed result of a build: a status, the structured diagnostics, and the
/// lines the parser could not interpret.
///
/// `warnings` is not build warnings — those are [`BuildDiagnostic`]s with
/// [`BuildSeverity::Warning`]. It is the parser's own abstentions: any
/// artifact line that did not match the canonical shape and was not a
/// recognised message continuation lands here, never dropped and never guessed
/// into a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub status: BuildStatus,
    pub diagnostics: Vec<BuildDiagnostic>,
    pub warnings: Vec<String>,
}

impl BuildReport {
    /// A report carrying only a status and no rows — used for the app-supplied
    /// states ([`BuildStatus::NeverBuilt`], [`BuildStatus::Building`],
    /// [`BuildStatus::CouldNotStart`]) that a parse can never produce.
    pub fn of_status(status: BuildStatus) -> Self {
        Self {
            status,
            diagnostics: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Errors among the diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &BuildDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == BuildSeverity::Error)
    }

    /// Warnings among the diagnostics.
    pub fn warning_diagnostics(&self) -> impl Iterator<Item = &BuildDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == BuildSeverity::Warning)
    }
}
