//! Types shared between the Rust core and the TypeScript frontend.
//!
//! Everything here derives [`specta::Type`] so `cb-app` can export a single
//! `bindings.ts` and the UI never hand-maintains a duplicate of these shapes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

// ---------------------------------------------------------------------------
// Workspace and projects
// ---------------------------------------------------------------------------

/// A project discovered inside a workspace by one of the ecosystem adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Project {
    /// Stable identifier, derived from the path relative to the workspace root.
    pub id: String,
    pub name: String,
    /// Absolute path to the project manifest (`.csproj`, `package.json`, ...).
    pub manifest_path: PathBuf,
    /// Absolute path to the directory containing the manifest.
    pub dir: PathBuf,
    /// Which adapter found this project (`"dotnet"`, `"node"`, or a manifest id).
    pub ecosystem: String,
    pub kind: ProjectKind,
    /// Target frameworks for .NET, engines/runtimes elsewhere. May be empty.
    pub frameworks: Vec<String>,
    /// True when the project appears to contain tests.
    pub is_test_project: bool,
    /// Which test runner this project uses, when it is a test project.
    pub test_runner: Option<TestRunner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProjectKind {
    /// Produces a runnable executable.
    Executable,
    /// Produces a library and cannot be launched directly.
    Library,
    /// Contains tests.
    Test,
    Unknown,
}

/// The concrete test runner a project uses.
///
/// The .NET split matters a great deal: VSTest and Microsoft.Testing.Platform
/// take *different, mutually ignored* command line arguments. See
/// [`crate::adapters::dotnet`] for how the two are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TestRunner {
    /// Classic `dotnet test` driving VSTest. Takes `--logger "trx;..."`.
    VsTest,
    /// Microsoft.Testing.Platform. Takes `-- --report-trx ...` and *silently
    /// ignores* VSTest's `--logger`.
    MicrosoftTestingPlatform,
    Vitest,
    Jest,
    /// Supplied by a declarative adapter manifest.
    Custom,
}

// ---------------------------------------------------------------------------
// Run configurations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    /// Launch the application.
    App,
    /// Execute tests and collect a report.
    Test,
}

/// Where a configuration came from. Surfaced in the UI so imported and
/// auto-detected configurations can be told apart from ones the user wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConfigSource {
    /// Produced by scanning the workspace.
    Detected,
    /// Read from `.code-basics/config.json`.
    UserFile,
    /// Best-effort conversion of a JetBrains Rider run configuration.
    RiderImport,
}

/// A named way to run something: an application launch or a test run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunConfig {
    pub id: String,
    pub name: String,
    pub kind: RunKind,
    pub ecosystem: String,
    pub source: ConfigSource,

    /// Project this configuration targets, if any. Relative to the workspace
    /// root so the file stays portable when checked in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,

    /// .NET build configuration (`Debug` / `Release`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_configuration: Option<String>,

    /// .NET target framework (`net8.0`), when a project is multi-targeted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,

    /// `launchSettings.json` profile name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_profile: Option<String>,

    /// npm/pnpm script name for Node projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,

    /// Arguments passed to the program itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Environment variables layered on top of the inherited environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    /// Working directory, relative to the workspace root. Defaults to the
    /// project directory, or the workspace root when there is no project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,

    /// Free-form notes. Used by the Rider importer to record anything that
    /// could not be translated, so the user can see it during review.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl RunConfig {
    /// Minimal configuration with everything optional left unset.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: RunKind,
        ecosystem: impl Into<String>,
        source: ConfigSource,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            ecosystem: ecosystem.into(),
            source,
            project: None,
            build_configuration: None,
            framework: None,
            launch_profile: None,
            script: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            warnings: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Invocations
// ---------------------------------------------------------------------------

/// The report file a test run is expected to leave behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportSpec {
    pub path: PathBuf,
    pub format: ReportFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ReportFormat {
    /// Visual Studio `.trx`, emitted by both VSTest and Microsoft.Testing.Platform.
    Trx,
    /// The `--json` shape shared by Jest and Vitest.
    JestLike,
    /// The universal fallback: nearly every runner in every language can emit it.
    JunitXml,
}

/// A fully resolved command, ready to hand to the process supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Applied on top of the inherited environment.
    pub env: BTreeMap<String, String>,
    /// Present for test runs; absent for application launches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<ReportSpec>,
    /// Problems detected while building the invocation that do not prevent it
    /// from running, but that the user should see (e.g. a missing TRX package).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Test results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TestOutcome {
    Passed,
    Failed,
    Skipped,
    /// Ran but produced no clear verdict (inconclusive, aborted, timed out).
    Other,
}

/// A single executed test. Parsers emit a flat list; [`crate::tests::tree`]
/// turns it into the hierarchy the UI renders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    /// Stable id used for re-runs and UI selection.
    pub id: String,
    /// Display name — the method or `it(...)` title.
    pub name: String,
    /// Fully qualified name, used to build re-run filters.
    pub full_name: String,
    /// Owning class, describe-block chain, or file.
    pub suite: Option<String>,
    /// Owning project or assembly.
    pub project: Option<String>,
    pub outcome: TestOutcome,
    pub duration_ms: Option<f64>,
    pub message: Option<String>,
    pub stack_trace: Option<String>,
    pub stdout: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestSummary {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub other: u32,
}

impl TestSummary {
    pub fn from_cases(cases: &[TestCase]) -> Self {
        let mut s = Self::default();
        for c in cases {
            s.total += 1;
            match c.outcome {
                TestOutcome::Passed => s.passed += 1,
                TestOutcome::Failed => s.failed += 1,
                TestOutcome::Skipped => s.skipped += 1,
                TestOutcome::Other => s.other += 1,
            }
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestRunResult {
    pub summary: TestSummary,
    pub cases: Vec<TestCase>,
    pub duration_ms: Option<f64>,
}

/// A node in the project → suite → test hierarchy shown in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestNode {
    pub id: String,
    pub label: String,
    /// The worst outcome among this node's descendants, so a failing leaf
    /// colours every ancestor.
    pub outcome: TestOutcome,
    pub summary: TestSummary,
    pub duration_ms: Option<f64>,
    /// Set only on leaves.
    pub case: Option<TestCase>,
    pub children: Vec<TestNode>,
}
