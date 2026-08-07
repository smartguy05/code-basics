//! Tests for dispatching a configuration to the adapter that owns it.

use super::invocation::*;
use crate::config;
use crate::model::{ConfigSource, RunConfig, RunKind};
use crate::workspace::{self, Workspace};

/// Build a workspace on disk from `(relative path, contents)` pairs and scan it.
fn workspace_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    let ws = workspace::scan(dir.path()).unwrap();
    (dir, ws)
}

const EXE_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net8.0</TargetFramework>
  </PropertyGroup>
</Project>"#;

const XUNIT_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>
  <ItemGroup>
    <PackageReference Include="xunit" Version="2.9.0" />
    <PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.11.0" />
  </ItemGroup>
</Project>"#;

const PYTEST_MANIFEST: &str = r#"
id = "pytest"
name = "pytest"
detect = ["pytest.ini"]

[test]
program = "pytest"
args = ["--junit-xml={report}", "-q"]
report_format = "junitXml"

[run.serve]
program = "uvicorn"
args = ["app:app"]
"#;

fn config(id: &str, kind: RunKind, ecosystem: &str) -> RunConfig {
    RunConfig::new(id, id, kind, ecosystem, ConfigSource::Detected)
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

#[test]
fn a_compound_configuration_has_no_command_of_its_own() {
    let (_dir, ws) = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
    let mut compound = config("all", RunKind::App, "compound");
    compound.name = "Everything".into();
    compound.compound = vec!["a".into(), "b".into()];

    let err = build(&ws, &compound, None).unwrap_err();
    assert_eq!(
        err,
        "Everything is a compound configuration with no command of its own"
    );
}

#[test]
fn creates_the_results_directory_before_running() {
    let (_dir, ws) = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
    let results = config::results_dir(&ws.root);
    assert!(!results.exists());

    let mut cfg = config("app", RunKind::App, "dotnet");
    cfg.project = Some("src/App/App.csproj".into());
    build(&ws, &cfg, None).unwrap();

    assert!(results.is_dir());
}

#[test]
fn dispatches_a_dotnet_application_to_dotnet_run() {
    let (_dir, ws) = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
    let mut cfg = config("app", RunKind::App, "dotnet");
    cfg.project = Some("src/App/App.csproj".into());

    let inv = build(&ws, &cfg, None).unwrap();
    assert_eq!(inv.program, "dotnet");
    assert_eq!(inv.args.first().map(String::as_str), Some("run"));
    assert!(inv.report.is_none());
}

#[test]
fn dispatches_a_dotnet_test_configuration_and_asks_for_a_report() {
    let (_dir, ws) = workspace_with(&[("tests/App.Tests/App.Tests.csproj", XUNIT_CSPROJ)]);
    let mut cfg = config("tests", RunKind::Test, "dotnet");
    cfg.project = Some("tests/App.Tests/App.Tests.csproj".into());

    let inv = build(&ws, &cfg, None).unwrap();
    assert_eq!(inv.program, "dotnet");
    assert_eq!(inv.args.first().map(String::as_str), Some("test"));
    assert!(inv.report.is_some(), "a test run must name its report");
}

#[test]
fn a_dotnet_test_filter_reaches_the_command_line() {
    let (_dir, ws) = workspace_with(&[("tests/App.Tests/App.Tests.csproj", XUNIT_CSPROJ)]);
    let mut cfg = config("tests", RunKind::Test, "dotnet");
    cfg.project = Some("tests/App.Tests/App.Tests.csproj".into());

    let inv = build(&ws, &cfg, Some(&["Ns.One".to_string()])).unwrap();
    assert!(
        inv.args.iter().any(|a| a.contains("Ns.One")),
        "expected the filter in {:?}",
        inv.args
    );
}

#[test]
fn dispatches_a_node_script_to_the_package_manager() {
    let (_dir, ws) = workspace_with(&[(
        "web/package.json",
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )]);
    let mut cfg = config("web-dev", RunKind::App, "node");
    cfg.project = Some("web".into());
    cfg.script = Some("dev".into());

    let inv = build(&ws, &cfg, None).unwrap();
    assert_eq!(inv.program, "npm");
    assert!(
        inv.args.iter().any(|a| a == "dev"),
        "expected the script name in {:?}",
        inv.args
    );
    assert_eq!(inv.cwd, ws.root.join("web"));
}

#[test]
fn dispatches_a_node_test_configuration_to_its_detected_runner() {
    let (_dir, ws) = workspace_with(&[(
        "web/package.json",
        r#"{"name":"web","scripts":{"test":"vitest"},"devDependencies":{"vitest":"^2.0.0"}}"#,
    )]);
    let mut cfg = config("web-test", RunKind::Test, "node");
    cfg.project = Some("web".into());

    let inv = build(&ws, &cfg, None).unwrap();
    assert!(inv.report.is_some(), "a test run must name its report");
}

#[test]
fn a_node_test_configuration_without_a_detected_runner_is_refused() {
    let (_dir, ws) = workspace_with(&[("web/package.json", r#"{"name":"web"}"#)]);
    let mut cfg = config("web-test", RunKind::Test, "node");
    cfg.name = "web tests".into();
    cfg.project = Some("web".into());

    let err = build(&ws, &cfg, None).unwrap_err();
    assert_eq!(
        err,
        "no test runner was detected for web tests. Add vitest or jest to its \
         package.json, then rescan."
    );
}

// ---------------------------------------------------------------------------
// Declarative adapters
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_ecosystem_with_no_manifests_reports_it_as_undefined() {
    let (_dir, ws) = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
    let cfg = config("py", RunKind::Test, "pytest");

    let err = build(&ws, &cfg, None).unwrap_err();
    assert_eq!(
        err,
        "no adapter named `pytest` is defined in .code-basics/adapters"
    );
}

#[test]
fn an_unparseable_manifest_is_named_as_the_likely_cause() {
    let (_dir, ws) = workspace_with(&[
        ("src/App/App.csproj", EXE_CSPROJ),
        (".code-basics/adapters/broken.toml", "id = ["),
    ]);
    let cfg = config("py", RunKind::Test, "pytest");

    let err = build(&ws, &cfg, None).unwrap_err();
    assert!(
        err.starts_with(
            "no adapter named `pytest` could be loaded. One or more manifests failed to parse:"
        ),
        "unexpected wording: {err}"
    );
    assert!(err.contains("broken.toml"), "unexpected wording: {err}");
}

#[test]
fn dispatches_to_a_declarative_adapter() {
    let (_dir, ws) = workspace_with(&[
        ("api/pytest.ini", ""),
        (".code-basics/adapters/pytest.toml", PYTEST_MANIFEST),
    ]);
    let mut cfg = config("api-tests", RunKind::Test, "pytest");
    cfg.project = Some("api".into());

    let inv = build(&ws, &cfg, None).unwrap();
    assert_eq!(inv.program, "pytest");
    assert_eq!(inv.cwd, ws.root.join("api"));
    assert!(inv.report.is_some());
}

#[test]
fn a_declarative_adapter_without_a_test_command_is_refused() {
    // A manifest with no commands at all is rejected at load; this one is
    // valid and simply has nothing to run tests with.
    let manifest = "id = \"runonly\"\nname = \"run only\"\n\n\
        [run.serve]\nprogram = \"serve\"\n";
    let (_dir, ws) = workspace_with(&[(".code-basics/adapters/runonly.toml", manifest)]);
    let cfg = config("t", RunKind::Test, "runonly");

    let err = build(&ws, &cfg, None).unwrap_err();
    assert_eq!(err, "the `runonly` adapter does not define a test command");
}

#[test]
fn a_declarative_adapter_without_the_named_run_command_is_refused() {
    let (_dir, ws) = workspace_with(&[(".code-basics/adapters/pytest.toml", PYTEST_MANIFEST)]);
    let mut cfg = config("py-run", RunKind::App, "pytest");
    cfg.script = Some("missing".into());

    let err = build(&ws, &cfg, None).unwrap_err();
    assert_eq!(
        err,
        "the `pytest` adapter does not define a run command named `missing`"
    );
}

#[test]
fn a_declarative_run_configuration_with_no_project_falls_back_to_the_root() {
    let (_dir, ws) = workspace_with(&[(".code-basics/adapters/pytest.toml", PYTEST_MANIFEST)]);
    let mut cfg = config("serve", RunKind::App, "pytest");
    cfg.script = Some("serve".into());

    let inv = build(&ws, &cfg, None).unwrap();
    assert_eq!(inv.program, "uvicorn");
    assert_eq!(inv.cwd, ws.root);
}

// ---------------------------------------------------------------------------
// Compound configurations
// ---------------------------------------------------------------------------

/// A compound over the two configurations `alpha` (dotnet) and `beta` (node),
/// in a workspace where both projects exist.
fn compound_workspace() -> (tempfile::TempDir, Workspace) {
    let (dir, mut ws) = workspace_with(&[
        ("api/Api.csproj", EXE_CSPROJ),
        (
            "web/package.json",
            r#"{"name":"web","scripts":{"dev":"vite"}}"#,
        ),
    ]);

    let mut alpha = config("alpha", RunKind::App, "dotnet");
    alpha.project = Some("api/Api.csproj".into());
    alpha.env.insert("FROM_CONFIG".into(), "config".into());
    alpha.env.insert("SHARED".into(), "config".into());

    let mut beta = config("beta", RunKind::App, "node");
    beta.project = Some("web".into());
    beta.script = Some("dev".into());

    let mut all = config("all", RunKind::App, "compound");
    all.name = "Everything".into();
    all.compound = vec!["alpha".into(), "beta".into()];

    ws.configs = vec![alpha, beta, all];
    (dir, ws)
}

fn compound_of(ws: &Workspace) -> RunConfig {
    ws.configs.iter().find(|c| c.id == "all").unwrap().clone()
}

#[test]
fn plans_every_member_of_a_compound_in_order() {
    let (_dir, ws) = compound_workspace();
    let planned = plan_compound(&ws, &compound_of(&ws), None).unwrap();

    let ids: Vec<&str> = planned.iter().map(|(c, _)| c.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "beta"]);
    assert_eq!(planned[0].1.program, "dotnet");
    assert_eq!(planned[1].1.program, "npm");
}

#[test]
fn a_member_that_no_longer_exists_stops_the_launch() {
    let (_dir, ws) = compound_workspace();
    let mut compound = compound_of(&ws);
    compound.compound = vec!["alpha".into(), "gone".into()];

    let err = plan_compound(&ws, &compound, None).unwrap_err();
    assert_eq!(err, "compound member `gone` no longer exists");
}

#[test]
fn a_nested_compound_is_refused() {
    let (_dir, ws) = compound_workspace();
    let mut inner = compound_of(&ws);
    inner.id = "inner".into();
    inner.name = "Inner".into();

    let mut ws = ws;
    ws.configs.push(inner);
    let mut outer = compound_of(&ws);
    outer.id = "outer".into();
    outer.compound = vec!["inner".into()];

    let err = plan_compound(&ws, &outer, None).unwrap_err();
    assert_eq!(
        err,
        "`Inner` is itself a compound configuration; nesting is not supported"
    );
}

#[test]
fn every_broken_member_is_reported_not_just_the_first() {
    let (_dir, ws) = compound_workspace();
    let mut compound = compound_of(&ws);
    compound.compound = vec!["gone".into(), "alpha".into(), "missing".into()];

    let err = plan_compound(&ws, &compound, None).unwrap_err();
    assert_eq!(
        err,
        "compound member `gone` no longer exists; compound member `missing` no longer exists"
    );
}

#[test]
fn the_runs_environment_is_layered_over_each_members_own() {
    let (_dir, ws) = compound_workspace();
    let env = std::collections::BTreeMap::from([
        ("SHARED".to_string(), "run".to_string()),
        ("FROM_RUN".to_string(), "run".to_string()),
    ]);

    let planned = plan_compound(&ws, &compound_of(&ws), Some(&env)).unwrap();
    let (alpha, invocation) = &planned[0];

    // The run's environment wins over the configuration's own value.
    assert_eq!(alpha.env.get("SHARED").map(String::as_str), Some("run"));
    assert_eq!(
        alpha.env.get("FROM_CONFIG").map(String::as_str),
        Some("config")
    );
    assert_eq!(alpha.env.get("FROM_RUN").map(String::as_str), Some("run"));
    // And it reaches the built command, not just the planned config.
    assert_eq!(
        invocation.env.get("SHARED").map(String::as_str),
        Some("run")
    );
}

#[test]
fn a_member_that_cannot_be_built_stops_the_launch() {
    let (_dir, ws) = compound_workspace();
    let mut ws = ws;
    let mut broken = config("broken", RunKind::Test, "node");
    broken.name = "web tests".into();
    broken.project = Some("web".into());
    ws.configs.push(broken);

    let mut compound = compound_of(&ws);
    compound.compound = vec!["broken".into()];

    let err = plan_compound(&ws, &compound, None).unwrap_err();
    assert!(
        err.starts_with("no test runner was detected for web tests."),
        "unexpected wording: {err}"
    );
}

// ---------------------------------------------------------------------------
// Re-run failed
// ---------------------------------------------------------------------------

fn case(full_name: &str, outcome: crate::model::TestOutcome) -> crate::model::TestCase {
    crate::model::TestCase {
        id: full_name.to_string(),
        name: full_name.to_string(),
        full_name: full_name.to_string(),
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
fn a_normal_run_has_no_filter_even_with_failures_on_record() {
    use crate::model::TestOutcome;
    let previous = vec![case("A", TestOutcome::Failed)];

    assert_eq!(rerun_filter(false, Some(&previous)), Ok(None));
}

#[test]
fn re_running_failures_filters_to_exactly_those_names() {
    use crate::model::TestOutcome;
    let previous = vec![
        case("Ns.A", TestOutcome::Failed),
        case("Ns.B", TestOutcome::Passed),
        case("Ns.C", TestOutcome::Failed),
    ];

    assert_eq!(
        rerun_filter(true, Some(&previous)),
        Ok(Some(vec!["Ns.A".to_string(), "Ns.C".to_string()]))
    );
}

#[test]
fn re_running_failures_with_no_previous_run_is_refused() {
    let err = rerun_filter(true, None).unwrap_err();
    assert_eq!(
        err,
        "there are no failed tests from a previous run of this configuration to re-run"
    );
}

#[test]
fn re_running_failures_when_everything_passed_is_refused() {
    use crate::model::TestOutcome;
    let previous = vec![
        case("A", TestOutcome::Passed),
        case("B", TestOutcome::Skipped),
    ];

    let err = rerun_filter(true, Some(&previous)).unwrap_err();
    assert_eq!(
        err,
        "there are no failed tests from a previous run of this configuration to re-run"
    );
}
