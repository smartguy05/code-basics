//! Types shared between the Rust core and the TypeScript frontend.
//!
//! `src/ipc/types.ts` mirrors these by hand. The `tests` module at the bottom
//! pins the exact JSON keys each type serialises to, so renaming a field here
//! fails a test that names the TypeScript file rather than silently producing
//! `undefined` in the UI.
//!
//! Everything also derives [`specta::Type`], which keeps the door open to
//! generating the TypeScript instead.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

// ---------------------------------------------------------------------------
// Workspace and projects
// ---------------------------------------------------------------------------

/// A project discovered inside a workspace by one of the ecosystem adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
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
    /// Build configurations the project offers (`Debug`, `Release`, and
    /// anything a .NET project adds via `<Configurations>`). Empty for
    /// ecosystems with no such concept.
    #[serde(default)]
    pub configurations: Vec<String>,
    /// True when the project appears to contain tests.
    pub is_test_project: bool,
    /// Which test runner this project uses, when it is a test project.
    pub test_runner: Option<TestRunner>,
    /// Why this project could not be fully read — a manifest that will not
    /// parse, or a file that could not be opened. `None` for a healthy project.
    ///
    /// A project carrying a reason is listed but inert: no configurations, no
    /// framework list, [`ProjectKind::Unknown`]. It exists because the
    /// alternative the scan used to take — dropping the project entirely — is
    /// the one outcome a user cannot act on. A shorter list looks exactly like
    /// a correct list, so a typo in a `package.json` silently removed a project
    /// from the Run tab with no error anywhere. The same rule is already
    /// applied to a missing sidecar (reports itself unavailable, with a reason)
    /// and to an object the inspector cannot read (`Unavailable`, carrying a
    /// sentence): the app abstains out loud rather than quietly.
    ///
    /// `skip_serializing_if` keeps the key **absent** rather than null for a
    /// healthy project, which is what `src/ipc/types.ts` mirrors as optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
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

    /// Skip `launchSettings.json` entirely (`dotnet run --no-launch-profile`).
    /// When false and no profile is named, `dotnet run` applies its default —
    /// the first Project profile, including its environment and URLs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_launch_settings: bool,

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

    /// Ids of the member configurations a compound configuration launches
    /// together. Non-empty only for compounds (`ecosystem == "compound"`),
    /// which have no invocation of their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound: Vec<String>,
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
            ignore_launch_settings: false,
            script: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            warnings: Vec::new(),
            compound: Vec::new(),
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

/// The code-coverage report format a run is expected to leave behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CoverageFormat {
    /// Cobertura XML — what coverlet's `XPlat Code Coverage` collector writes.
    Cobertura,
    /// LCOV text — what Vitest's / Jest's `lcov` reporter writes.
    Lcov,
}

/// Where a run's code-coverage report lands, and in which format.
///
/// Unlike [`ReportSpec`], `path` is **not always a file**: for
/// [`CoverageFormat::Cobertura`] it is the `--results-directory` coverlet was
/// told to use, and the consumer must locate the newest `coverage.cobertura.xml`
/// inside a GUID subfolder under it. For [`CoverageFormat::Lcov`] it is the
/// `lcov.info` file itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CoverageSpec {
    pub path: PathBuf,
    pub format: CoverageFormat,
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
    /// Where this run's code-coverage report will land, when coverage was
    /// requested. Absent — the default — for an ordinary run, which leaves the
    /// command line unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageSpec>,
    /// Problems detected while building the invocation that do not prevent it
    /// from running, but that the user should see (e.g. a missing TRX package).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Test results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TestOutcome {
    Passed,
    Failed,
    Skipped,
    /// Ran but produced no clear verdict (inconclusive, aborted, timed out).
    /// Also the default, so an unrecognised outcome is never silently counted
    /// as a pass.
    #[default]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact JSON keys each model produces.
    ///
    /// The TypeScript in `src/ipc/types.ts` is written by hand against these
    /// names, so a rename on the Rust side has to fail somewhere visible.
    /// Without this, a renamed field would serialise happily and only show up
    /// as an undefined value in the UI.
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
    fn test_case_serialises_with_the_keys_the_ui_reads() {
        let case = TestCase {
            id: "id".into(),
            name: "name".into(),
            full_name: "full".into(),
            suite: None,
            project: None,
            outcome: TestOutcome::Passed,
            duration_ms: None,
            message: None,
            stack_trace: None,
            stdout: None,
        };

        assert_eq!(
            keys(&serde_json::to_value(&case).unwrap()),
            [
                "durationMs",
                "fullName",
                "id",
                "message",
                "name",
                "outcome",
                "project",
                "stackTrace",
                "stdout",
                "suite"
            ]
        );
    }

    #[test]
    fn project_serialises_with_camel_case_keys() {
        let project = Project {
            id: "id".into(),
            name: "name".into(),
            manifest_path: "a.csproj".into(),
            dir: ".".into(),
            ecosystem: "dotnet".into(),
            kind: ProjectKind::Test,
            frameworks: vec![],
            configurations: vec![],
            is_test_project: true,
            test_runner: Some(TestRunner::VsTest),
            unreadable: None,
        };

        // A healthy project has no `unreadable` key at all — `types.ts` mirrors
        // it as optional, not as `string | null`.
        assert_eq!(
            keys(&serde_json::to_value(&project).unwrap()),
            [
                "configurations",
                "dir",
                "ecosystem",
                "frameworks",
                "id",
                "isTestProject",
                "kind",
                "manifestPath",
                "name",
                "testRunner"
            ]
        );

        let broken = Project {
            unreadable: Some("expected value at line 1 column 3".into()),
            ..project
        };
        let json = serde_json::to_value(&broken).unwrap();
        assert!(
            keys(&json).contains(&"unreadable".to_string()),
            "got {json}"
        );
        assert_eq!(json["unreadable"], "expected value at line 1 column 3");
    }

    #[test]
    fn enum_variants_serialise_in_camel_case() {
        assert_eq!(
            serde_json::to_string(&TestOutcome::Passed).unwrap(),
            "\"passed\""
        );
        assert_eq!(
            serde_json::to_string(&TestRunner::MicrosoftTestingPlatform).unwrap(),
            "\"microsoftTestingPlatform\""
        );
        assert_eq!(serde_json::to_string(&RunKind::Test).unwrap(), "\"test\"");
        assert_eq!(
            serde_json::to_string(&ConfigSource::RiderImport).unwrap(),
            "\"riderImport\""
        );
        assert_eq!(
            serde_json::to_string(&ReportFormat::JestLike).unwrap(),
            "\"jestLike\""
        );
        assert_eq!(
            serde_json::to_string(&CoverageFormat::Cobertura).unwrap(),
            "\"cobertura\""
        );
        assert_eq!(
            serde_json::to_string(&CoverageFormat::Lcov).unwrap(),
            "\"lcov\""
        );
    }

    #[test]
    fn optional_config_fields_are_omitted_rather_than_null() {
        // The config file is checked in, so absent fields should not appear.
        let config = RunConfig::new("id", "name", RunKind::App, "dotnet", ConfigSource::Detected);
        let json = serde_json::to_value(&config).unwrap();

        assert_eq!(keys(&json), ["ecosystem", "id", "kind", "name", "source"]);
    }

    #[test]
    fn ignoring_launch_settings_serialises_under_the_key_the_ui_reads() {
        // False is the default and stays out of the checked-in file entirely
        // (pinned by `optional_config_fields_are_omitted_rather_than_null`).
        let mut config =
            RunConfig::new("id", "name", RunKind::App, "dotnet", ConfigSource::UserFile);
        config.ignore_launch_settings = true;
        let json = serde_json::to_value(&config).unwrap();

        assert_eq!(json["ignoreLaunchSettings"], serde_json::json!(true));
    }

    // -- summarising a run ---------------------------------------------------

    fn case(outcome: TestOutcome) -> TestCase {
        TestCase {
            id: "id".into(),
            name: "name".into(),
            full_name: "full".into(),
            suite: None,
            project: None,
            outcome,
            duration_ms: None,
            message: None,
            stack_trace: None,
            stdout: None,
        }
    }

    #[test]
    fn summarising_no_cases_counts_nothing() {
        assert_eq!(TestSummary::from_cases(&[]), TestSummary::default());
        assert_eq!(TestSummary::from_cases(&[]).total, 0);
    }

    #[test]
    fn each_outcome_is_counted_in_its_own_bucket() {
        let summary = TestSummary::from_cases(&[
            case(TestOutcome::Passed),
            case(TestOutcome::Passed),
            case(TestOutcome::Failed),
            case(TestOutcome::Skipped),
            case(TestOutcome::Other),
        ]);

        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.other, 1);
    }

    /// Every case lands in exactly one bucket, so the parts always add back up
    /// to the total. A run where they disagree is a run the UI cannot explain.
    #[test]
    fn the_buckets_always_add_up_to_the_total() {
        let summary = TestSummary::from_cases(&[
            case(TestOutcome::Passed),
            case(TestOutcome::Failed),
            case(TestOutcome::Skipped),
            case(TestOutcome::Other),
            case(TestOutcome::Failed),
        ]);

        assert_eq!(summary.total, 5);
        assert_eq!(
            summary.passed + summary.failed + summary.skipped + summary.other,
            summary.total
        );
    }

    /// `Other` is the default outcome precisely so an unrecognised verdict is
    /// never quietly counted as a pass.
    #[test]
    fn an_unrecognised_outcome_is_never_counted_as_a_pass() {
        let summary = TestSummary::from_cases(&[case(TestOutcome::default())]);

        assert_eq!(summary.passed, 0);
        assert_eq!(summary.other, 1);
    }

    #[test]
    fn a_summary_serialises_with_the_keys_the_ui_reads() {
        assert_eq!(
            keys(&serde_json::to_value(TestSummary::default()).unwrap()),
            ["failed", "other", "passed", "skipped", "total"]
        );
    }

    // -- invocations ---------------------------------------------------------

    fn invocation() -> Invocation {
        Invocation {
            program: "dotnet".into(),
            args: vec!["build".into()],
            cwd: PathBuf::from("/repo"),
            env: BTreeMap::new(),
            report: None,
            coverage: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn an_invocation_serialises_with_the_keys_the_ui_reads() {
        assert_eq!(
            keys(&serde_json::to_value(invocation()).unwrap()),
            ["args", "cwd", "env", "program"]
        );
    }

    /// An application launch has no report and no warnings; both stay out of
    /// the payload rather than crossing as nulls the UI has to guard against.
    #[test]
    fn an_invocation_with_nothing_to_report_omits_those_fields() {
        let json = serde_json::to_value(invocation()).unwrap();

        assert!(json.get("report").is_none());
        assert!(json.get("coverage").is_none());
        assert!(json.get("warnings").is_none());
    }

    #[test]
    fn a_coverage_invocation_carries_its_coverage_spec_across() {
        let mut inv = invocation();
        inv.coverage = Some(CoverageSpec {
            path: PathBuf::from(".code-basics/results"),
            format: CoverageFormat::Cobertura,
        });

        let json = serde_json::to_value(&inv).unwrap();

        assert_eq!(keys(&json["coverage"]), ["format", "path"]);
        assert_eq!(json["coverage"]["format"], serde_json::json!("cobertura"));
    }

    #[test]
    fn a_test_invocation_carries_its_report_spec_across() {
        let mut inv = invocation();
        inv.report = Some(ReportSpec {
            path: PathBuf::from("results/api.trx"),
            format: ReportFormat::Trx,
        });
        inv.warnings = vec!["no TRX package".into()];

        let json = serde_json::to_value(&inv).unwrap();

        assert_eq!(keys(&json["report"]), ["format", "path"]);
        assert_eq!(json["report"]["format"], serde_json::json!("trx"));
        assert_eq!(json["warnings"], serde_json::json!(["no TRX package"]));
    }

    #[test]
    fn an_invocation_round_trips_through_json() {
        let mut inv = invocation();
        inv.env.insert("MODE".into(), "test".into());
        inv.report = Some(ReportSpec {
            path: PathBuf::from("results/api.xml"),
            format: ReportFormat::JunitXml,
        });

        let json = serde_json::to_string(&inv).unwrap();

        assert_eq!(serde_json::from_str::<Invocation>(&json).unwrap(), inv);
    }

    #[test]
    fn compound_members_serialise_under_the_key_the_ui_reads() {
        let mut config = RunConfig::new(
            "id",
            "name",
            RunKind::App,
            "compound",
            ConfigSource::RiderImport,
        );
        config.compound = vec!["member".into()];
        let json = serde_json::to_value(&config).unwrap();

        assert_eq!(json["compound"], serde_json::json!(["member"]));
    }
}
