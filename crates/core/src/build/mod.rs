//! Build diagnostics.
//!
//! Modelled on [`crate::testing`]: a runner streams raw output live to the
//! console, and the structured view is built afterwards from an artifact the
//! tool wrote. Here the artifact is the MSBuild file-logger output requested
//! with `-flp1:errorsOnly;logfile=…` + `-flp2:warningsOnly;logfile=…` and
//! `/p:GenerateFullPaths=true`, which is emitted in a defined canonical shape
//! (`PATH(line,col): severity CODE: message [project]`) — so this stays in
//! Rust with no nuget or sidecar dependency, exactly as the test parsers do.

pub mod mcp;
pub mod model;
pub mod parse;

pub use model::{BuildDiagnostic, BuildReport, BuildSeverity, BuildStatus};
pub use parse::{parse_diagnostic_line, parse_report};

#[cfg(test)]
#[path = "build_tests.rs"]
mod build_tests;
