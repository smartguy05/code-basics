//! Tests for the .NET adapter.
//!
//! Kept in their own file because runner classification deserves exhaustive
//! coverage: a misclassification does not fail loudly, it produces a run that
//! exits zero and silently writes no report.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::dotnet::*;
use crate::model::{ConfigSource, CoverageFormat, ProjectKind, RunConfig, RunKind, TestRunner};

fn csproj(body: &str) -> ProjectFile {
    parse_project_file(&format!(
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>{body}</PropertyGroup></Project>"#
    ))
}

fn with_packages(packages: &[&str]) -> ProjectFile {
    let refs: String = packages
        .iter()
        .map(|p| format!(r#"<PackageReference Include="{p}" Version="1.0.0" />"#))
        .collect();
    parse_project_file(&format!(
        r#"<Project Sdk="Microsoft.NET.Sdk"><ItemGroup>{refs}</ItemGroup></Project>"#
    ))
}

// ---------------------------------------------------------------------------
// Project file parsing
// ---------------------------------------------------------------------------

#[test]
fn reads_single_target_framework() {
    let p = csproj("<TargetFramework>net8.0</TargetFramework>");
    assert_eq!(p.target_frameworks, vec!["net8.0"]);
}

#[test]
fn reads_multiple_target_frameworks() {
    let p = csproj("<TargetFrameworks>net8.0;net9.0</TargetFrameworks>");
    assert_eq!(p.target_frameworks, vec!["net8.0", "net9.0"]);
}

#[test]
fn reads_output_type_and_test_flag() {
    let p = csproj("<OutputType>Exe</OutputType><IsTestProject>true</IsTestProject>");
    assert_eq!(p.output_type.as_deref(), Some("Exe"));
    assert_eq!(p.is_test_project, Some(true));
}

#[test]
fn reads_package_references_from_both_element_forms() {
    // Self-closing and container forms both appear in real project files.
    let xml = r#"<Project>
      <ItemGroup>
        <PackageReference Include="xunit" Version="2.9.0" />
        <PackageReference Include="Microsoft.NET.Test.Sdk"><Version>17.11.0</Version></PackageReference>
      </ItemGroup>
    </Project>"#;
    let p = parse_project_file(xml);
    assert!(p.references("xunit"));
    assert!(p.references("Microsoft.NET.Test.Sdk"));
}

#[test]
fn package_matching_ignores_case() {
    let p = with_packages(&["XUnit"]);
    assert!(p.references("xunit"));
    assert!(p.references_prefix("XUNIT"));
}

#[test]
fn project_references_are_parsed_from_include() {
    let xml = r#"<Project Sdk="Microsoft.NET.Sdk">
      <ItemGroup>
        <ProjectReference Include="..\Lib\Lib.csproj" />
      </ItemGroup>
    </Project>"#;
    let p = parse_project_file(xml);
    assert_eq!(p.project_references, vec![r"..\Lib\Lib.csproj".to_string()]);
}

#[test]
fn a_project_reference_written_as_an_open_element_is_also_parsed() {
    // `parse_project_file` handles Start and Empty elements in two independent
    // arms, so a fix applied to only one of them silently misses half of the
    // project files in the wild.
    let xml = r#"<Project Sdk="Microsoft.NET.Sdk">
      <ItemGroup>
        <ProjectReference Include="..\Lib\Lib.csproj"></ProjectReference>
      </ItemGroup>
    </Project>"#;
    let p = parse_project_file(xml);
    assert_eq!(p.project_references, vec![r"..\Lib\Lib.csproj".to_string()]);
}

#[test]
fn a_project_reference_without_an_include_is_ignored() {
    let xml = r#"<Project Sdk="Microsoft.NET.Sdk">
      <ItemGroup>
        <ProjectReference Update="..\Lib\Lib.csproj" />
        <ProjectReference />
      </ItemGroup>
    </Project>"#;
    let p = parse_project_file(xml);
    assert_eq!(p.project_references, Vec::<String>::new());
}

#[test]
fn a_project_with_no_references_has_none() {
    let p = csproj("<TargetFramework>net8.0</TargetFramework>");
    assert_eq!(p.project_references, Vec::<String>::new());
}

#[test]
fn malformed_project_file_yields_empty_rather_than_panicking() {
    let p = parse_project_file("<Project><PropertyGroup>");
    assert_eq!(p.target_frameworks, Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// dotnet.config
// ---------------------------------------------------------------------------

#[test]
fn reads_mtp_selection_from_dotnet_config() {
    let content = "[dotnet.test:runner]\nname = \"Microsoft.Testing.Platform\"\n";
    assert_eq!(
        parse_dotnet_config(content),
        Some(ConfiguredRunner::MicrosoftTestingPlatform)
    );
}

#[test]
fn reads_vstest_selection_from_dotnet_config() {
    let content = "[dotnet.test:runner]\nname = \"VSTest\"\n";
    assert_eq!(parse_dotnet_config(content), Some(ConfiguredRunner::VsTest));
}

#[test]
fn ignores_name_keys_outside_the_runner_section() {
    let content = "[other]\nname = \"Microsoft.Testing.Platform\"\n";
    assert_eq!(parse_dotnet_config(content), None);
}

#[test]
fn tolerates_comments_and_unquoted_values() {
    let content =
        "# pick a runner\n[dotnet.test:runner]\n; comment\nname = Microsoft.Testing.Platform\n";
    assert_eq!(
        parse_dotnet_config(content),
        Some(ConfiguredRunner::MicrosoftTestingPlatform)
    );
}

// ---------------------------------------------------------------------------
// Runner classification — the part that must not get this wrong
// ---------------------------------------------------------------------------

#[test]
fn classic_xunit_project_is_vstest() {
    let p = with_packages(&[
        "xunit",
        "xunit.runner.visualstudio",
        "Microsoft.NET.Test.Sdk",
    ]);
    assert_eq!(classify_runner(&p, &[], None), TestRunner::VsTest);
}

#[test]
fn dotnet_config_overrides_every_other_signal() {
    // Packages say VSTest; the config says MTP. The config wins.
    let p = with_packages(&["xunit", "Microsoft.NET.Test.Sdk"]);
    assert_eq!(
        classify_runner(&p, &[], Some(ConfiguredRunner::MicrosoftTestingPlatform)),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn explicit_property_beats_package_evidence() {
    let p = parse_project_file(
        r#"<Project>
          <PropertyGroup><TestingPlatformDotnetTestSupport>true</TestingPlatformDotnetTestSupport></PropertyGroup>
          <ItemGroup><PackageReference Include="Microsoft.NET.Test.Sdk" /></ItemGroup>
        </Project>"#,
    );
    assert_eq!(
        classify_runner(&p, &[], None),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn property_set_to_false_forces_vstest() {
    let p = parse_project_file(
        r#"<Project>
          <PropertyGroup><TestingPlatformDotnetTestSupport>false</TestingPlatformDotnetTestSupport></PropertyGroup>
          <ItemGroup><PackageReference Include="xunit.v3" /></ItemGroup>
        </Project>"#,
    );
    assert_eq!(classify_runner(&p, &[], None), TestRunner::VsTest);
}

#[test]
fn property_inherited_from_directory_build_props_applies() {
    let project = with_packages(&["MSTest"]);
    let props = vec![csproj(
        "<TestingPlatformDotnetTestSupport>true</TestingPlatformDotnetTestSupport>",
    )];
    assert_eq!(
        classify_runner(&project, &props, None),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn project_property_overrides_the_inherited_one() {
    let project =
        csproj("<TestingPlatformDotnetTestSupport>false</TestingPlatformDotnetTestSupport>");
    let props = vec![csproj(
        "<TestingPlatformDotnetTestSupport>true</TestingPlatformDotnetTestSupport>",
    )];
    assert_eq!(classify_runner(&project, &props, None), TestRunner::VsTest);
}

#[test]
fn enable_mstest_runner_selects_mtp() {
    let p = parse_project_file(
        r#"<Project>
          <PropertyGroup><EnableMSTestRunner>true</EnableMSTestRunner></PropertyGroup>
          <ItemGroup><PackageReference Include="MSTest" /></ItemGroup>
        </Project>"#,
    );
    assert_eq!(
        classify_runner(&p, &[], None),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn xunit_v3_is_mtp_native() {
    let p = with_packages(&["xunit.v3"]);
    assert_eq!(
        classify_runner(&p, &[], None),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn tunit_is_mtp_native() {
    let p = with_packages(&["TUnit"]);
    assert_eq!(
        classify_runner(&p, &[], None),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn mtp_extensions_without_a_vstest_host_means_mtp() {
    let p = with_packages(&[
        "Microsoft.Testing.Platform",
        "Microsoft.Testing.Extensions.TrxReport",
    ]);
    assert_eq!(
        classify_runner(&p, &[], None),
        TestRunner::MicrosoftTestingPlatform
    );
}

#[test]
fn mtp_extensions_alongside_a_vstest_host_stays_vstest() {
    // Microsoft.NET.Test.Sdk is what VSTest needs; its presence means the
    // classic path is still viable and is what `dotnet test` will choose.
    let p = with_packages(&["Microsoft.Testing.Platform", "Microsoft.NET.Test.Sdk"]);
    assert_eq!(classify_runner(&p, &[], None), TestRunner::VsTest);
}

#[test]
fn a_project_with_no_evidence_defaults_to_vstest() {
    assert_eq!(
        classify_runner(&ProjectFile::default(), &[], None),
        TestRunner::VsTest
    );
}

// ---------------------------------------------------------------------------
// Test project detection
// ---------------------------------------------------------------------------

#[test]
fn recognises_test_projects_by_framework_package() {
    for package in ["xunit", "NUnit", "MSTest.TestFramework", "TUnit"] {
        let p = with_packages(&[package]);
        assert!(
            is_test_project(&p, &[]),
            "{package} should mark a test project"
        );
    }
}

#[test]
fn explicit_is_test_project_false_wins_over_packages() {
    let p = parse_project_file(
        r#"<Project>
          <PropertyGroup><IsTestProject>false</IsTestProject></PropertyGroup>
          <ItemGroup><PackageReference Include="xunit" /></ItemGroup>
        </Project>"#,
    );
    assert!(
        !is_test_project(&p, &[]),
        "a shared test-helper library is not a test project"
    );
}

#[test]
fn a_plain_library_is_not_a_test_project() {
    let p = with_packages(&["Newtonsoft.Json"]);
    assert!(!is_test_project(&p, &[]));
}

// ---------------------------------------------------------------------------
// TRX extension availability
// ---------------------------------------------------------------------------

#[test]
fn detects_the_trx_extension_package() {
    let p = with_packages(&["Microsoft.Testing.Extensions.TrxReport"]);
    assert!(has_trx_extension(&p, &[]));
}

#[test]
fn mtp_native_frameworks_bundle_trx_reporting() {
    assert!(has_trx_extension(&with_packages(&["xunit.v3"]), &[]));
    assert!(has_trx_extension(&with_packages(&["TUnit"]), &[]));
}

#[test]
fn plain_mtp_project_lacks_trx_reporting() {
    let p = with_packages(&["Microsoft.Testing.Platform"]);
    assert!(!has_trx_extension(&p, &[]));
}

// ---------------------------------------------------------------------------
// Invocation building
// ---------------------------------------------------------------------------

fn test_config() -> RunConfig {
    let mut c = RunConfig::new(
        "proj:test:debug",
        "tests",
        RunKind::Test,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/Calc.Tests/Calc.Tests.csproj"));
    c.build_configuration = Some("Debug".into());
    c
}

fn ctx<'a>(root: &'a Path, results: &'a Path, runner: TestRunner) -> BuildContext<'a> {
    BuildContext {
        workspace_root: root,
        results_dir: results,
        runner: Some(runner),
        trx_extension_available: true,
        has_launch_settings: false,
        filter: None,
        coverage: false,
        dumps_dir: None,
        dump_env: None,
    }
}

fn coverage_ctx<'a>(root: &'a Path, results: &'a Path, runner: TestRunner) -> BuildContext<'a> {
    BuildContext {
        coverage: true,
        ..ctx(root, results, runner)
    }
}

#[test]
fn coverage_mode_collects_xplat_coverage_and_sets_the_spec() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/.code-basics/results");
    let inv = test_invocation(
        &test_config(),
        &coverage_ctx(root, results, TestRunner::VsTest),
    );

    assert!(inv
        .args
        .iter()
        .any(|a| a == "--collect:XPlat Code Coverage"));
    assert!(inv.args.iter().any(|a| a == "--results-directory"));

    let spec = inv.coverage.expect("coverage spec set");
    assert_eq!(spec.format, CoverageFormat::Cobertura);
    // Cobertura's spec points at the results *directory*; the consumer finds
    // the newest coverage.cobertura.xml under a GUID subfolder.
    assert_eq!(spec.path, results.to_path_buf());
}

#[test]
fn a_non_coverage_test_run_is_byte_identical_to_today() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/.code-basics/results");
    let plain = test_invocation(&test_config(), &ctx(root, results, TestRunner::VsTest));

    assert!(plain.coverage.is_none());
    assert!(!plain.args.iter().any(|a| a.contains("XPlat Code Coverage")));
}

fn app_config() -> RunConfig {
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c
}

#[test]
fn vstest_invocation_uses_the_logger_flag() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/.code-basics/results");
    let inv = test_invocation(&test_config(), &ctx(root, results, TestRunner::VsTest));

    assert_eq!(inv.program, "dotnet");
    assert!(inv.args.contains(&"--logger".to_string()));
    let logger = inv
        .args
        .iter()
        .find(|a| a.starts_with("trx;"))
        .expect("trx logger");
    assert!(logger.contains("LogFileName="));
    // Per-test console lines feed the UI's live progress counter.
    assert!(inv.args.contains(&"console;verbosity=normal".to_string()));
    // The MTP separator must not appear, or the logger would be ignored.
    assert!(!inv.args.contains(&"--".to_string()));
}

#[test]
fn mtp_invocation_passes_report_options_after_the_separator() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/.code-basics/results");
    let inv = test_invocation(
        &test_config(),
        &ctx(root, results, TestRunner::MicrosoftTestingPlatform),
    );

    let sep = inv
        .args
        .iter()
        .position(|a| a == "--")
        .expect("MTP separator");
    let report = inv
        .args
        .iter()
        .position(|a| a == "--report-trx")
        .expect("--report-trx");

    assert!(report > sep, "MTP options before `--` would be ignored");
    // VSTest's flag must never be used here: MTP accepts and ignores it.
    assert!(!inv.args.iter().any(|a| a == "--logger"));
}

#[test]
fn mtp_invocation_asks_for_per_test_output() {
    // MTP's default `Normal` verbosity prints only a run summary, so without
    // this the live progress counter stays at zero until the run finishes.
    // `Detailed` prints `passed TestName (5ms)` as each test completes —
    // verified against MSTest 4.0.2 on the .NET 10 SDK.
    let root = Path::new("/repo");
    let results = Path::new("/repo/.code-basics/results");
    let inv = test_invocation(
        &test_config(),
        &ctx(root, results, TestRunner::MicrosoftTestingPlatform),
    );

    let sep = inv
        .args
        .iter()
        .position(|a| a == "--")
        .expect("MTP separator");
    let output = inv
        .args
        .iter()
        .position(|a| a == "--output")
        .expect("--output");

    assert!(output > sep, "MTP options must follow the separator");
    assert_eq!(inv.args[output + 1], "Detailed");
}

#[test]
fn both_runners_agree_on_where_the_report_lands() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/.code-basics/results");

    let vstest = test_invocation(&test_config(), &ctx(root, results, TestRunner::VsTest));
    let mtp = test_invocation(
        &test_config(),
        &ctx(root, results, TestRunner::MicrosoftTestingPlatform),
    );

    // One parser reads both, so both must write to the same place.
    assert_eq!(vstest.report, mtp.report);
    let report = vstest.report.unwrap();
    assert_eq!(report.path.parent().unwrap(), results);
    assert_eq!(report.format, crate::model::ReportFormat::Trx);
}

#[test]
fn warns_when_mtp_cannot_write_a_report() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = ctx(root, results, TestRunner::MicrosoftTestingPlatform);
    c.trx_extension_available = false;

    let inv = test_invocation(&test_config(), &c);
    assert!(
        inv.warnings.iter().any(|w| w.contains("TrxReport")),
        "a silent no-report run is exactly what the warning exists to prevent"
    );
}

#[test]
fn does_not_warn_when_the_trx_extension_is_present() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let inv = test_invocation(
        &test_config(),
        &ctx(root, results, TestRunner::MicrosoftTestingPlatform),
    );
    assert!(inv.warnings.is_empty());
}

#[test]
fn rerun_filter_lists_each_failing_test() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = ctx(root, results, TestRunner::VsTest);
    c.filter = Some(vec!["N.C.Alpha".into(), "N.C.Beta".into()]);

    let inv = test_invocation(&test_config(), &c);
    let filter = inv.args[inv.args.iter().position(|a| a == "--filter").unwrap() + 1].clone();

    assert_eq!(
        filter,
        "FullyQualifiedName=N.C.Alpha|FullyQualifiedName=N.C.Beta"
    );
}

#[test]
fn rerun_filter_strips_theory_arguments() {
    // FullyQualifiedName matches the method, so leaving the data row's
    // arguments attached would match nothing and re-run zero tests.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = ctx(root, results, TestRunner::VsTest);
    c.filter = Some(vec!["N.C.IsEven(value: 2)".into()]);

    let inv = test_invocation(&test_config(), &c);
    let filter = inv.args[inv.args.iter().position(|a| a == "--filter").unwrap() + 1].clone();

    assert_eq!(filter, "FullyQualifiedName=N.C.IsEven");
}

#[test]
fn mtp_filtering_warns_that_it_may_not_apply() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = ctx(root, results, TestRunner::MicrosoftTestingPlatform);
    c.filter = Some(vec!["N.C.Alpha".into()]);

    let inv = test_invocation(&test_config(), &c);
    assert!(inv.warnings.iter().any(|w| w.contains("VSTest bridge")));
}

#[test]
fn run_invocation_leaves_the_default_profile_to_dotnet_run() {
    // No profile named means `dotnet run` behaves as it does from a terminal:
    // the first Project profile applies, environment and applicationUrl
    // included. Suppressing it silently cost real users their user secrets
    // and their URLs.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));

    let inv = run_invocation(&c, &ctx(root, results, TestRunner::VsTest));
    assert!(!inv.args.contains(&"--no-launch-profile".to_string()));
    assert!(!inv.args.contains(&"--launch-profile".to_string()));
}

#[test]
fn run_invocation_can_opt_out_of_launch_settings() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.ignore_launch_settings = true;

    let inv = run_invocation(&c, &ctx(root, results, TestRunner::VsTest));
    assert!(inv.args.contains(&"--no-launch-profile".to_string()));
}

#[test]
fn run_invocation_selects_a_named_launch_profile() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.launch_profile = Some("https".into());

    let inv = run_invocation(&c, &ctx(root, results, TestRunner::VsTest));
    let idx = inv
        .args
        .iter()
        .position(|a| a == "--launch-profile")
        .unwrap();
    assert_eq!(inv.args[idx + 1], "https");
    assert!(!inv.args.contains(&"--no-launch-profile".to_string()));
}

#[test]
fn skipping_launch_settings_warns_about_the_environment_it_loses() {
    // `--no-launch-profile` means no ASPNETCORE_ENVIRONMENT=Development,
    // which silently disables .NET user secrets — apps that keep connection
    // strings there fail at startup with no visible reason.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.ignore_launch_settings = true;

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.has_launch_settings = true;

    let inv = run_invocation(&c, &build);
    assert!(
        inv.warnings.iter().any(|w| w.contains("user secrets")),
        "{:?}",
        inv.warnings
    );
}

#[test]
fn the_launch_settings_warning_stays_quiet_when_it_does_not_apply() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.ignore_launch_settings = true;

    // No launchSettings.json at all: nothing is being ignored.
    let inv = run_invocation(&c, &ctx(root, results, TestRunner::VsTest));
    assert!(inv.warnings.is_empty(), "{:?}", inv.warnings);

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.has_launch_settings = true;

    // Not opting out: `dotnet run` applies its default profile.
    c.ignore_launch_settings = false;
    let inv = run_invocation(&c, &build);
    assert!(inv.warnings.is_empty(), "{:?}", inv.warnings);

    // A profile is selected: launchSettings.json applies.
    c.launch_profile = Some("https".into());
    let inv = run_invocation(&c, &build);
    assert!(inv.warnings.is_empty(), "{:?}", inv.warnings);

    // Opted out but the environment is set explicitly: the user has taken over.
    c.launch_profile = None;
    c.ignore_launch_settings = true;
    c.env
        .insert("ASPNETCORE_ENVIRONMENT".into(), "Development".into());
    let inv = run_invocation(&c, &build);
    assert!(inv.warnings.is_empty(), "{:?}", inv.warnings);
}

#[test]
fn build_actions_produce_the_expected_dotnet_verbs() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.build_configuration = Some("Release".into());

    let build = build_action_invocation(&c, BuildAction::Build, root);
    assert_eq!(build.program, "dotnet");
    assert_eq!(build.args[0], "build");
    assert!(!build.args.contains(&"--no-incremental".to_string()));
    assert!(build.args.contains(&"Release".to_string()));

    // Rebuild is a full compile, not an incremental one.
    let rebuild = build_action_invocation(&c, BuildAction::Rebuild, root);
    assert_eq!(rebuild.args[0], "build");
    assert_eq!(rebuild.args[1], "--no-incremental");

    let clean = build_action_invocation(&c, BuildAction::Clean, root);
    assert_eq!(clean.args[0], "clean");
    assert!(clean.args.iter().any(|a| a.ends_with("App.csproj")));
}

/// A configuration that names no project builds whatever the working
/// directory contains, rather than emitting a bare `dotnet build` with a
/// stray flag where the project should be.
#[test]
fn a_build_with_no_project_passes_no_target() {
    let root = Path::new("/repo");
    let c = RunConfig::new(
        "loose",
        "loose",
        RunKind::App,
        "dotnet",
        ConfigSource::UserFile,
    );

    let inv = build_action_invocation(&c, BuildAction::Build, root);

    assert_eq!(inv.args, vec!["build"]);
    assert_eq!(inv.cwd, PathBuf::from("/repo"));
}

/// Every action names the project, or a rebuild would quietly compile
/// something other than what the user selected.
#[test]
fn every_action_names_the_project_it_was_given() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));

    for action in [BuildAction::Build, BuildAction::Rebuild, BuildAction::Clean] {
        let inv = build_action_invocation(&c, action, root);
        assert!(
            inv.args.iter().any(|a| a.ends_with("App.csproj")),
            "{action:?} lost the project: {:?}",
            inv.args
        );
    }
}

#[test]
fn a_build_carries_the_target_framework_across() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.framework = Some("net8.0".into());

    let inv = build_action_invocation(&c, BuildAction::Build, root);

    let i = inv.args.iter().position(|a| a == "-f").expect("-f");
    assert_eq!(inv.args[i + 1], "net8.0");
}

/// A multi-targeted project built in a named configuration needs both flags,
/// and neither may displace the other.
#[test]
fn a_configuration_and_a_framework_are_both_passed() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.build_configuration = Some("Release".into());
    c.framework = Some("net8.0".into());

    let inv = build_action_invocation(&c, BuildAction::Rebuild, root);

    let ci = inv.args.iter().position(|a| a == "-c").expect("-c");
    let fi = inv.args.iter().position(|a| a == "-f").expect("-f");
    assert_eq!(inv.args[ci + 1], "Release");
    assert_eq!(inv.args[fi + 1], "net8.0");
    assert_eq!(inv.args[0..2], ["build", "--no-incremental"]);
}

/// Rebuild and Build differ by exactly one flag; a clean is a different verb
/// entirely and must never carry it.
#[test]
fn only_a_rebuild_disables_incremental_compilation() {
    let root = Path::new("/repo");
    let c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );

    let flag = "--no-incremental".to_string();
    assert!(!build_action_invocation(&c, BuildAction::Build, root)
        .args
        .contains(&flag));
    assert!(build_action_invocation(&c, BuildAction::Rebuild, root)
        .args
        .contains(&flag));
    assert!(!build_action_invocation(&c, BuildAction::Clean, root)
        .args
        .contains(&flag));
    assert_eq!(
        build_action_invocation(&c, BuildAction::Clean, root).args[0],
        "clean"
    );
}

/// The default working directory is the project's own, so relative paths in
/// build hooks behave the way they do when launched from an IDE.
#[test]
fn a_build_runs_in_the_projects_directory_unless_told_otherwise() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));

    assert_eq!(
        build_action_invocation(&c, BuildAction::Build, root).cwd,
        PathBuf::from("/repo/src/App")
    );

    c.cwd = Some(PathBuf::from("tools"));
    assert_eq!(
        build_action_invocation(&c, BuildAction::Build, root).cwd,
        PathBuf::from("/repo/tools")
    );
}

/// A build produces no test report, and nothing about it is worth warning
/// over — an empty warning list is what lets the UI stay quiet.
#[test]
fn a_build_leaves_no_report_and_raises_no_warnings() {
    let root = Path::new("/repo");
    let c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );

    for action in [BuildAction::Build, BuildAction::Rebuild, BuildAction::Clean] {
        let inv = build_action_invocation(&c, action, root);
        assert!(inv.report.is_none(), "{action:?}");
        assert!(inv.warnings.is_empty(), "{action:?}");
        assert_eq!(inv.program, "dotnet");
    }
}

/// The configuration's environment reaches the build, not just the run: a
/// project that needs `NUGET_PACKAGES` set to restore needs it here too.
#[test]
fn the_configurations_environment_reaches_the_build() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.env.insert("NUGET_PACKAGES".into(), "/packages".into());

    let inv = build_action_invocation(&c, BuildAction::Build, root);

    assert_eq!(
        inv.env.get("NUGET_PACKAGES").map(String::as_str),
        Some("/packages")
    );
}

/// Program arguments belong to the application being launched, not to
/// `dotnet build` — passing them through would fail the build with an
/// unrecognised option.
#[test]
fn program_arguments_do_not_leak_into_a_build() {
    let root = Path::new("/repo");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.args = vec!["--serve".into(), "--port=8080".into()];

    let inv = build_action_invocation(&c, BuildAction::Build, root);

    assert_eq!(inv.args, vec!["build"]);
}

#[test]
fn build_action_names_cross_ipc_in_camel_case() {
    assert_eq!(
        serde_json::to_string(&BuildAction::Rebuild).unwrap(),
        "\"rebuild\""
    );
    assert_eq!(
        serde_json::from_str::<BuildAction>("\"clean\"").unwrap(),
        BuildAction::Clean
    );
}

#[test]
fn program_arguments_go_after_the_separator() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut c = RunConfig::new(
        "p:run",
        "app",
        RunKind::App,
        "dotnet",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("src/App/App.csproj"));
    c.args = vec!["--verbose".into(), "input.txt".into()];

    let inv = run_invocation(&c, &ctx(root, results, TestRunner::VsTest));
    let sep = inv.args.iter().position(|a| a == "--").unwrap();

    // Without the separator these would be parsed by `dotnet run` itself.
    assert_eq!(&inv.args[sep + 1..], ["--verbose", "input.txt"]);
}

#[test]
fn working_directory_defaults_to_the_project_directory() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let inv = test_invocation(&test_config(), &ctx(root, results, TestRunner::VsTest));
    assert_eq!(inv.cwd, PathBuf::from("/repo/src/Calc.Tests"));
}

#[test]
fn explicit_working_directory_is_resolved_against_the_workspace() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let mut config = test_config();
    config.cwd = Some(PathBuf::from("fixtures"));

    let inv = test_invocation(&config, &ctx(root, results, TestRunner::VsTest));
    assert_eq!(inv.cwd, PathBuf::from("/repo/fixtures"));
}

// ---------------------------------------------------------------------------
// launchSettings.json
// ---------------------------------------------------------------------------

const LAUNCH_SETTINGS: &str = r#"{
  "profiles": {
    "http": {
      "commandName": "Project",
      "dotnetRunMessages": true,
      "launchBrowser": true,
      "applicationUrl": "http://localhost:5051",
      "environmentVariables": { "ASPNETCORE_ENVIRONMENT": "Development" }
    },
    "https": {
      "commandName": "Project",
      "applicationUrl": "https://localhost:7051;http://localhost:5051",
      "commandLineArgs": "--seed \"first run\"",
      "environmentVariables": { "ASPNETCORE_ENVIRONMENT": "Development" }
    },
    "IIS Express": {
      "commandName": "IISExpress",
      "launchBrowser": true
    }
  }
}"#;

#[test]
fn reads_project_launch_profiles() {
    let profiles = parse_launch_settings(LAUNCH_SETTINGS);
    let launchable: Vec<&str> = profiles
        .iter()
        .filter(|p| p.launchable)
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(launchable, vec!["http", "https"]);
}

#[test]
fn hosting_profiles_this_app_cannot_launch_are_reported_not_dropped() {
    // Dropping them made a project whose only profile is IIS Express look like
    // it had no profiles at all, with nothing to explain why.
    let profiles = parse_launch_settings(LAUNCH_SETTINGS);
    let iis = profiles
        .iter()
        .find(|p| p.name == "IIS Express")
        .expect("the profile must survive parsing");

    assert!(
        !iis.launchable,
        "`dotnet run` cannot apply an IISExpress profile"
    );
    assert_eq!(iis.command_name.as_deref(), Some("IISExpress"));
}

#[test]
fn unlaunchable_profiles_produce_no_run_configuration() {
    let profiles = parse_launch_settings(LAUNCH_SETTINGS);
    let configs = configs_for_project(
        "app",
        "App",
        Path::new("src/App/App.csproj"),
        ProjectKind::Executable,
        &["net8.0".into()],
        &["Debug".into(), "Release".into()],
        &profiles,
    );

    assert!(
        !configs
            .iter()
            .any(|c| c.launch_profile.as_deref() == Some("IIS Express")),
        "offering a profile we cannot launch would fail at run time"
    );
}

#[test]
fn reads_environment_and_urls_from_a_profile() {
    let profiles = parse_launch_settings(LAUNCH_SETTINGS);
    let http = profiles.iter().find(|p| p.name == "http").unwrap();

    assert_eq!(
        http.env.get("ASPNETCORE_ENVIRONMENT").map(String::as_str),
        Some("Development")
    );
    assert_eq!(
        http.application_url.as_deref(),
        Some("http://localhost:5051")
    );
}

#[test]
fn splits_quoted_command_line_arguments() {
    let profiles = parse_launch_settings(LAUNCH_SETTINGS);
    let https = profiles.iter().find(|p| p.name == "https").unwrap();
    assert_eq!(https.args, vec!["--seed", "first run"]);
}

#[test]
fn argument_splitting_handles_the_awkward_cases() {
    assert_eq!(split_args(""), Vec::<String>::new());
    assert_eq!(split_args("  a   b  "), vec!["a", "b"]);
    assert_eq!(
        split_args(r#"--name "two words" --flag"#),
        vec!["--name", "two words", "--flag"]
    );
    // A quoted empty string is a real argument and must survive.
    assert_eq!(
        split_args(r#"--name "" --flag"#),
        vec!["--name", "", "--flag"]
    );
}

#[test]
fn malformed_launch_settings_yields_no_profiles() {
    assert!(parse_launch_settings("{ not json").is_empty());
    assert!(parse_launch_settings("{}").is_empty());
}

// ---------------------------------------------------------------------------
// Generated configurations
// ---------------------------------------------------------------------------

#[test]
fn test_projects_get_a_single_debug_configuration() {
    // Debug only, like Rider: `#if !DEBUG` code paths make an auto-offered
    // Release test run a trap. A Release config can still be saved by hand.
    let configs = configs_for_project(
        "calc-tests",
        "Calc.Tests",
        Path::new("src/Calc.Tests/Calc.Tests.csproj"),
        ProjectKind::Test,
        &["net8.0".into()],
        &["Debug".into(), "Release".into()],
        &[],
    );

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].kind, RunKind::Test);
    assert_eq!(configs[0].build_configuration.as_deref(), Some("Debug"));
    assert_eq!(
        configs[0].id, "calc-tests:test:debug",
        "the id existing favourites reference"
    );
}

#[test]
fn executables_get_a_configuration_per_launch_profile() {
    let profiles = parse_launch_settings(LAUNCH_SETTINGS);
    let configs = configs_for_project(
        "app",
        "App",
        Path::new("src/App/App.csproj"),
        ProjectKind::Executable,
        &["net8.0".into()],
        &["Debug".into(), "Release".into()],
        &profiles,
    );

    assert!(configs
        .iter()
        .any(|c| c.launch_profile.as_deref() == Some("http")));
    assert!(configs
        .iter()
        .any(|c| c.launch_profile.as_deref() == Some("https")));
    // Plus the plain Debug/Release pair.
    assert!(configs.iter().any(|c| c.launch_profile.is_none()));
}

#[test]
fn libraries_produce_no_configurations() {
    let configs = configs_for_project(
        "lib",
        "Lib",
        Path::new("src/Lib/Lib.csproj"),
        ProjectKind::Library,
        &["net8.0".into()],
        &["Debug".into(), "Release".into()],
        &[],
    );
    assert!(configs.is_empty(), "a library cannot be launched or tested");
}

#[test]
fn framework_is_pinned_only_when_a_project_multi_targets() {
    // Passing -f to a single-target project is noise; to a multi-target one it
    // is required, since `dotnet run` refuses to guess.
    let single = configs_for_project(
        "a",
        "A",
        Path::new("a.csproj"),
        ProjectKind::Test,
        &["net8.0".into()],
        &["Debug".into()],
        &[],
    );
    assert!(single.iter().all(|c| c.framework.is_none()));

    let multi = configs_for_project(
        "b",
        "B",
        Path::new("b.csproj"),
        ProjectKind::Test,
        &["net8.0".into(), "net9.0".into()],
        &["Debug".into()],
        &[],
    );
    // Every framework gets its own configuration: pinning the first silently
    // hid the others, and `dotnet test -f` runs exactly one.
    let frameworks: Vec<&str> = multi
        .iter()
        .filter_map(|c| c.framework.as_deref())
        .collect();
    assert_eq!(frameworks, vec!["net8.0", "net9.0"]);
    assert_eq!(multi.len(), 2);
    assert!(
        multi.iter().all(|c| c.id.starts_with("b:test:debug:net")),
        "ids must stay unique"
    );
}

#[test]
fn custom_build_configurations_become_run_configurations() {
    // `<Configurations>Debug;Release;Staging</Configurations>` is the only way
    // a non-default configuration is visible without evaluating MSBuild.
    let configs = configs_for_project(
        "app",
        "App",
        Path::new("src/App/App.csproj"),
        ProjectKind::Executable,
        &["net8.0".into()],
        &["Debug".into(), "Release".into(), "Staging".into()],
        &[],
    );

    let names: Vec<&str> = configs
        .iter()
        .filter_map(|c| c.build_configuration.as_deref())
        .collect();
    assert_eq!(names, vec!["Debug", "Release", "Staging"]);
}

#[test]
fn declared_configurations_replace_the_default_pair() {
    let declared = csproj("<Configurations>Debug;Release;Staging</Configurations>");
    assert_eq!(
        configurations(&declared, &[]),
        vec!["Debug", "Release", "Staging"]
    );

    // Nothing declared means the MSBuild default.
    let plain = csproj("<TargetFramework>net9.0</TargetFramework>");
    assert_eq!(configurations(&plain, &[]), vec!["Debug", "Release"]);

    // A Directory.Build.props declaration applies to the projects beneath it.
    let inherited = csproj("<Configurations>Debug;QA</Configurations>");
    assert_eq!(configurations(&plain, &[inherited]), vec!["Debug", "QA"]);
}

#[test]
fn project_kind_follows_output_type() {
    let exe = csproj("<OutputType>Exe</OutputType>");
    assert_eq!(project_kind(&exe, &[], false), ProjectKind::Executable);

    let winexe = csproj("<OutputType>WinExe</OutputType>");
    assert_eq!(project_kind(&winexe, &[], false), ProjectKind::Executable);

    let lib = csproj("<OutputType>Library</OutputType>");
    assert_eq!(project_kind(&lib, &[], false), ProjectKind::Library);

    // A test project is a test project regardless of what it compiles to.
    assert_eq!(project_kind(&exe, &[], true), ProjectKind::Test);
}

#[test]
fn output_type_is_inherited_from_directory_build_props() {
    // A folder of tools commonly sets OutputType once, for all of them.
    let plain = csproj("<TargetFramework>net9.0</TargetFramework>");
    let props = csproj("<OutputType>Exe</OutputType>");

    assert_eq!(project_kind(&plain, &[], false), ProjectKind::Library);
    assert_eq!(
        project_kind(&plain, &[props], false),
        ProjectKind::Executable
    );
}

#[test]
fn workload_projects_are_executables_without_an_output_type() {
    // MAUI heads and Aspire app hosts are launched with `dotnet run`, but the
    // workload — not the template — decides what they compile to.
    let maui = csproj("<UseMaui>true</UseMaui>");
    assert_eq!(project_kind(&maui, &[], false), ProjectKind::Executable);

    let aspire = csproj("<IsAspireHost>true</IsAspireHost>");
    assert_eq!(project_kind(&aspire, &[], false), ProjectKind::Executable);

    // Aspire hosts layer their SDK in as a nested <Sdk Name="..." /> import.
    let sdk_import = parse_project_file(
        r#"<Project Sdk="Microsoft.NET.Sdk">
             <Sdk Name="Aspire.AppHost.Sdk" Version="9.0.0" />
             <PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup>
           </Project>"#,
    );
    assert_eq!(sdk_import.sdk_imports, vec!["Aspire.AppHost.Sdk"]);
    assert_eq!(
        project_kind(&sdk_import, &[], false),
        ProjectKind::Executable
    );
}

#[test]
fn web_sdk_projects_are_executables_without_declaring_an_output_type() {
    // ASP.NET Core templates never write <OutputType>; the SDK implies Exe.
    for sdk in [
        "Microsoft.NET.Sdk.Web",
        "Microsoft.NET.Sdk.Worker",
        "Microsoft.NET.Sdk.BlazorWebAssembly",
    ] {
        let p = parse_project_file(&format!(
            r#"<Project Sdk="{sdk}"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>"#
        ));
        assert_eq!(p.sdk.as_deref(), Some(sdk));
        assert_eq!(
            project_kind(&p, &[], false),
            ProjectKind::Executable,
            "{sdk}"
        );
    }

    // The plain SDK still defaults to Library.
    let plain = csproj("<TargetFramework>net9.0</TargetFramework>");
    assert_eq!(project_kind(&plain, &[], false), ProjectKind::Library);

    // So does the Razor SDK: it builds Razor *class libraries*, and Blazor
    // Server apps use the Web SDK instead.
    let razor = parse_project_file(r#"<Project Sdk="Microsoft.NET.Sdk.Razor"></Project>"#);
    assert_eq!(project_kind(&razor, &[], false), ProjectKind::Library);
}

// ---------------------------------------------------------------------------
// Crash dump capture
// ---------------------------------------------------------------------------

#[test]
fn arming_dumps_sets_the_three_runtime_variables() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dumps_dir = Some(dumps);

    let inv = run_invocation(&app_config(), &build);
    assert_eq!(
        inv.env.get("DOTNET_DbgEnableMiniDump").map(String::as_str),
        Some("1")
    );
    // Heap. Mini and Triage omit the object data the inspector exists to read.
    assert_eq!(
        inv.env.get("DOTNET_DbgMiniDumpType").map(String::as_str),
        Some("2")
    );
    // `%e` expands with the extension, so the name carries `App.exe` — that is
    // what later attributes a dump to the process that was actually run.
    let name = inv.env.get("DOTNET_DbgMiniDumpName").expect("dump name");
    assert!(name.contains("%e_%p_%t"), "{name}");
    assert!(name.ends_with(".dmp"), "{name}");
}

#[test]
fn nothing_is_armed_unless_the_workspace_asked() {
    // Capture is opt-in per workspace: a dump is a verbatim copy of process
    // memory, and a run must never start writing hundreds of megabytes of it
    // because a default said so.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");

    let app = run_invocation(&app_config(), &ctx(root, results, TestRunner::VsTest));
    let tests = test_invocation(&test_config(), &ctx(root, results, TestRunner::VsTest));

    for inv in [&app, &tests] {
        assert!(
            !inv.env.keys().any(|k| k.starts_with("DOTNET_Dbg")),
            "{:?}",
            inv.env
        );
    }
    assert!(!tests
        .args
        .contains(&"--blame-crash-collect-always".to_string()));
}

#[test]
fn a_configurations_own_dump_settings_win() {
    // The same precedence `process` uses for its colour defaults: these are
    // defaults layered underneath, not an override applied on top.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut config = app_config();
    config
        .env
        .insert("DOTNET_DbgMiniDumpName".into(), "D:/mine/%p.dmp".into());
    config
        .env
        .insert("DOTNET_DbgMiniDumpType".into(), "4".into());

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dumps_dir = Some(dumps);

    let inv = run_invocation(&config, &build);
    assert_eq!(
        inv.env.get("DOTNET_DbgMiniDumpName").map(String::as_str),
        Some("D:/mine/%p.dmp")
    );
    assert_eq!(
        inv.env.get("DOTNET_DbgMiniDumpType").map(String::as_str),
        Some("4")
    );
    // Untouched keys still get their default.
    assert_eq!(
        inv.env.get("DOTNET_DbgEnableMiniDump").map(String::as_str),
        Some("1")
    );
}

#[test]
fn vstest_collects_a_dump_even_on_a_tidy_test_host_exit() {
    // A failed assertion is not an unhandled crash, so the runtime variables
    // alone produce nothing for the run that most needs a dump.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dumps_dir = Some(dumps);

    let inv = test_invocation(&test_config(), &build);
    assert!(inv
        .args
        .contains(&"--blame-crash-collect-always".to_string()));
}

#[test]
fn a_vstest_run_says_where_the_blame_collectors_dump_lands() {
    // Blame writes into --results-directory under a name of its own, so the
    // dump does not appear in the Objects tab. A user who is not told that
    // reads the empty list as "the capture did not work".
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dumps_dir = Some(dumps);

    let inv = test_invocation(&test_config(), &build);

    assert!(
        inv.warnings
            .iter()
            .any(|w| w.contains(".code-basics/results/")
                && w.contains("--blame-crash-collect-always")),
        "{:?}",
        inv.warnings
    );
}

#[test]
fn every_armed_run_warns_that_its_memory_may_be_written_to_disk() {
    // `captureDumps` lives in a file that is checked in, so the opt-in is one
    // person's edit and everybody's runs. The run itself has to say so: a
    // teammate who never opens the Objects tab would otherwise get hundreds of
    // megabytes of their process memory on disk with nothing on screen.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    for runner in [TestRunner::VsTest, TestRunner::MicrosoftTestingPlatform] {
        let mut build = ctx(root, results, runner);
        build.dumps_dir = Some(dumps);

        for inv in [
            run_invocation(&app_config(), &build),
            test_invocation(&test_config(), &build),
        ] {
            assert!(
                inv.warnings
                    .iter()
                    .any(|w| w.contains("captureDumps") && w.contains("copy of its memory")),
                "{runner:?} produced {:?}",
                inv.warnings
            );
        }
    }
}

#[test]
fn a_run_that_captures_nothing_warns_about_nothing() {
    // The counterpart of the test above: the warning must be tied to being
    // armed, not to running .NET.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");

    let app = run_invocation(&app_config(), &ctx(root, results, TestRunner::VsTest));
    let tests = test_invocation(&test_config(), &ctx(root, results, TestRunner::VsTest));

    for inv in [&app, &tests] {
        assert!(
            !inv.warnings.iter().any(|w| w.contains("captureDumps")),
            "{:?}",
            inv.warnings
        );
    }
}

#[test]
fn the_workspace_inspector_env_reaches_a_capturing_run() {
    // `inspector.env` is documented as the way to ask for a different dump
    // type. Persisting a setting nothing reads is worse than not offering it.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut workspace_env = BTreeMap::new();
    workspace_env.insert("DOTNET_DbgMiniDumpType".to_string(), "4".to_string());
    workspace_env.insert("CB_EXTRA".to_string(), "yes".to_string());

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dumps_dir = Some(dumps);
    build.dump_env = Some(&workspace_env);

    let inv = run_invocation(&app_config(), &build);

    assert_eq!(
        inv.env.get("DOTNET_DbgMiniDumpType").map(String::as_str),
        Some("4")
    );
    assert_eq!(inv.env.get("CB_EXTRA").map(String::as_str), Some("yes"));
    // The defaults it did not mention still apply.
    assert_eq!(
        inv.env.get("DOTNET_DbgEnableMiniDump").map(String::as_str),
        Some("1")
    );
}

#[test]
fn a_run_configurations_env_still_beats_the_workspace_inspector_env() {
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut workspace_env = BTreeMap::new();
    workspace_env.insert("DOTNET_DbgMiniDumpType".to_string(), "4".to_string());

    let mut config = app_config();
    config
        .env
        .insert("DOTNET_DbgMiniDumpType".into(), "1".into());

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dumps_dir = Some(dumps);
    build.dump_env = Some(&workspace_env);

    let inv = run_invocation(&config, &build);

    assert_eq!(
        inv.env.get("DOTNET_DbgMiniDumpType").map(String::as_str),
        Some("1")
    );
}

#[test]
fn the_workspace_inspector_env_is_not_applied_to_a_run_that_captures_nothing() {
    // It exists to tune a capture; with no capture there is nothing to tune,
    // and leaking `DOTNET_Dbg*` into an unarmed run would arm it by accident.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");

    let mut workspace_env = BTreeMap::new();
    workspace_env.insert("DOTNET_DbgMiniDumpType".to_string(), "4".to_string());

    let mut build = ctx(root, results, TestRunner::VsTest);
    build.dump_env = Some(&workspace_env);

    let inv = run_invocation(&app_config(), &build);

    assert!(
        !inv.env.keys().any(|k| k.starts_with("DOTNET_Dbg")),
        "{:?}",
        inv.env
    );
}

#[test]
fn mtp_gets_the_package_name_instead_of_a_flag_it_ignores() {
    // Microsoft.Testing.Platform ignores every `--blame-*` option rather than
    // rejecting it, so passing one would look like it worked and collect
    // nothing. The replacement needs a package reference, which is the user's
    // edit to their project — not ours to make.
    let root = Path::new("/repo");
    let results = Path::new("/repo/results");
    let dumps = Path::new("/repo/.code-basics/dumps");

    let mut build = ctx(root, results, TestRunner::MicrosoftTestingPlatform);
    build.dumps_dir = Some(dumps);

    let inv = test_invocation(&test_config(), &build);
    assert!(!inv.args.iter().any(|a| a.starts_with("--blame")));
    assert!(
        inv.warnings
            .iter()
            .any(|w| w.contains("Microsoft.Testing.Extensions.CrashDump")),
        "{:?}",
        inv.warnings
    );
}
