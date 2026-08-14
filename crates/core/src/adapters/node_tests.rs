//! Tests for the JS/TS adapter.

use std::path::{Path, PathBuf};

use super::node::*;
use crate::model::{ConfigSource, ProjectKind, RunConfig, RunKind, TestRunner};

fn pkg(json: &str) -> PackageJson {
    parse_package_json(json).expect("valid package.json")
}

// ---------------------------------------------------------------------------
// package.json parsing and classification
// ---------------------------------------------------------------------------

#[test]
fn reads_scripts_and_dependencies() {
    let p = pkg(r#"{
      "name": "web",
      "scripts": { "dev": "vite", "test": "vitest" },
      "devDependencies": { "vitest": "^2.0.0" }
    }"#);

    assert_eq!(p.name.as_deref(), Some("web"));
    assert_eq!(p.scripts.get("dev").map(String::as_str), Some("vite"));
    assert!(p.depends_on("vitest"));
}

#[test]
fn finds_dependencies_in_either_section() {
    let dev = pkg(r#"{"devDependencies":{"jest":"^29"}}"#);
    let prod = pkg(r#"{"dependencies":{"jest":"^29"}}"#);
    assert!(dev.depends_on("jest"));
    assert!(prod.depends_on("jest"));
}

#[test]
fn identifies_the_test_runner() {
    assert_eq!(
        detect_runner(&pkg(r#"{"devDependencies":{"vitest":"^2"}}"#)),
        Some(TestRunner::Vitest)
    );
    assert_eq!(
        detect_runner(&pkg(r#"{"devDependencies":{"jest":"^29"}}"#)),
        Some(TestRunner::Jest)
    );
    assert_eq!(
        detect_runner(&pkg(r#"{"devDependencies":{"ts-jest":"^29"}}"#)),
        Some(TestRunner::Jest)
    );
    assert_eq!(
        detect_runner(&pkg(r#"{"dependencies":{"react":"^19"}}"#)),
        None
    );
}

#[test]
fn prefers_vitest_when_both_runners_are_present() {
    // Mid-migration repositories have both; vitest is the one being moved to.
    let p = pkg(r#"{"devDependencies":{"vitest":"^2","jest":"^29"}}"#);
    assert_eq!(detect_runner(&p), Some(TestRunner::Vitest));
}

#[test]
fn recognises_a_monorepo_root() {
    let root = pkg(r#"{"private":true,"workspaces":["packages/*"]}"#);
    let leaf = pkg(r#"{"name":"pkg"}"#);
    assert!(is_workspace_root(&root));
    assert!(!is_workspace_root(&leaf));
}

#[test]
fn workspace_globs_reads_the_array_form() {
    let p = pkg(r#"{"private":true,"workspaces":["packages/*","apps/*"]}"#);
    assert_eq!(workspace_globs(&p), vec!["packages/*", "apps/*"]);
}

#[test]
fn workspace_globs_reads_the_object_form() {
    // Yarn's object form nests the patterns under `packages` and adds keys
    // that are not globs at all; only `packages` describes membership.
    let p = pkg(
        r#"{"workspaces":{"packages":["packages/*","tools/cli"],"nohoist":["**/react-native"]}}"#,
    );
    assert_eq!(workspace_globs(&p), vec!["packages/*", "tools/cli"]);
}

#[test]
fn workspace_globs_returns_nothing_for_a_plain_package() {
    assert!(workspace_globs(&pkg(r#"{"name":"leaf"}"#)).is_empty());
    assert!(workspace_globs(&pkg(r#"{"workspaces":{}}"#)).is_empty());
    assert!(workspace_globs(&pkg(r#"{"workspaces":[]}"#)).is_empty());
    // Hand-edited nonsense must not take down a workspace scan.
    assert!(workspace_globs(&pkg(r#"{"workspaces":"packages/*"}"#)).is_empty());
    assert!(workspace_globs(&pkg(r#"{"workspaces":7}"#)).is_empty());
    assert!(workspace_globs(&pkg(r#"{"workspaces":null}"#)).is_empty());
    assert!(workspace_globs(&pkg(r#"{"workspaces":{"packages":"packages/*"}}"#)).is_empty());
}

#[test]
fn workspace_globs_skips_non_string_entries_rather_than_failing() {
    let p = pkg(r#"{"workspaces":["packages/*",42,null,{"a":1},"apps/*"]}"#);
    assert_eq!(workspace_globs(&p), vec!["packages/*", "apps/*"]);

    let y = pkg(r#"{"workspaces":{"packages":["packages/*",false,"apps/*"]}}"#);
    assert_eq!(workspace_globs(&y), vec!["packages/*", "apps/*"]);
}

#[test]
fn classifies_project_kind_from_scripts_and_deps() {
    assert_eq!(
        project_kind(&pkg(r#"{"devDependencies":{"vitest":"^2"}}"#)),
        ProjectKind::Test
    );
    assert_eq!(
        project_kind(&pkg(r#"{"scripts":{"dev":"vite"}}"#)),
        ProjectKind::Executable
    );
    assert_eq!(
        project_kind(&pkg(r#"{"name":"lib"}"#)),
        ProjectKind::Library
    );
}

#[test]
fn invalid_package_json_is_rejected() {
    assert!(parse_package_json("{ not json").is_none());
}

// ---------------------------------------------------------------------------
// Package manager detection
// ---------------------------------------------------------------------------

#[test]
fn infers_the_package_manager_from_the_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(detect_package_manager(dir.path()), PackageManager::Npm);

    std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
    assert_eq!(detect_package_manager(dir.path()), PackageManager::Yarn);

    std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
    assert_eq!(detect_package_manager(dir.path()), PackageManager::Pnpm);

    // bun wins over everything: a repo mid-migration keeps stale lockfiles.
    std::fs::write(dir.path().join("bun.lockb"), "").unwrap();
    assert_eq!(detect_package_manager(dir.path()), PackageManager::Bun);
}

#[test]
fn each_manager_runs_scripts_its_own_way() {
    assert_eq!(
        PackageManager::Npm.run_script_args("dev"),
        vec!["run", "dev"]
    );
    assert_eq!(
        PackageManager::Pnpm.run_script_args("dev"),
        vec!["run", "dev"]
    );
    // yarn and bun take the script name directly.
    assert_eq!(PackageManager::Yarn.run_script_args("dev"), vec!["dev"]);
    assert_eq!(PackageManager::Bun.run_script_args("dev"), vec!["dev"]);
}

// ---------------------------------------------------------------------------
// Test invocations
// ---------------------------------------------------------------------------

fn test_config() -> RunConfig {
    let mut c = RunConfig::new(
        "web:test",
        "web tests",
        RunKind::Test,
        "node",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("apps/web"));
    c
}

fn build(
    runner: TestRunner,
    manager: PackageManager,
    filter: Option<&[String]>,
) -> crate::model::Invocation {
    test_invocation(
        &test_config(),
        Path::new("/repo"),
        Path::new("/repo/apps/web"),
        manager,
        runner,
        Path::new("/repo/.code-basics/results"),
        filter,
    )
}

#[test]
fn vitest_keeps_the_console_reporter_alongside_the_json_one() {
    // Requesting only the JSON reporter would silence the live output the
    // console pane exists to show.
    let inv = build(TestRunner::Vitest, PackageManager::Pnpm, None);

    assert!(inv.args.iter().any(|a| a == "--reporter=default"));
    assert!(inv.args.iter().any(|a| a == "--reporter=json"));
}

#[test]
fn vitest_qualifies_the_output_file_by_reporter() {
    // With two reporters, a bare --outputFile would receive the console
    // output instead of the JSON report.
    let inv = build(TestRunner::Vitest, PackageManager::Pnpm, None);

    let output = inv.args.iter().find(|a| a.contains("outputFile")).unwrap();
    assert!(output.starts_with("--outputFile.json="), "got {output}");
}

#[test]
fn vitest_runs_once_rather_than_watching() {
    let inv = build(TestRunner::Vitest, PackageManager::Npm, None);
    assert!(
        inv.args.iter().any(|a| a == "run"),
        "watch mode would never exit"
    );
}

#[test]
fn jest_disables_watch_mode_explicitly() {
    let inv = build(TestRunner::Jest, PackageManager::Npm, None);
    assert!(inv.args.iter().any(|a| a == "--watchAll=false"));
    assert!(inv.args.iter().any(|a| a == "--json"));
}

#[test]
fn report_path_and_format_are_set_for_both_runners() {
    for runner in [TestRunner::Vitest, TestRunner::Jest] {
        let inv = build(runner, PackageManager::Npm, None);
        let report = inv.report.expect("a test run must produce a report");
        assert_eq!(report.format, crate::model::ReportFormat::JestLike);
        assert_eq!(
            report.path.parent().unwrap(),
            Path::new("/repo/.code-basics/results")
        );
    }
}

#[test]
fn each_manager_executes_binaries_its_own_way() {
    assert_eq!(
        build(TestRunner::Vitest, PackageManager::Npm, None).program,
        "npx"
    );
    assert_eq!(
        build(TestRunner::Vitest, PackageManager::Pnpm, None).program,
        "pnpm"
    );
    assert_eq!(
        build(TestRunner::Vitest, PackageManager::Bun, None).program,
        "bunx"
    );
}

#[test]
fn rerun_filter_anchors_and_escapes_test_names() {
    let names = vec!["math add adds numbers".to_string()];
    let inv = build(TestRunner::Vitest, PackageManager::Npm, Some(&names));

    let idx = inv.args.iter().position(|a| a == "-t").expect("-t filter");
    assert_eq!(inv.args[idx + 1], "^(math add adds numbers)$");
}

#[test]
fn rerun_filter_escapes_regex_metacharacters_in_test_names() {
    // `-t` takes a regular expression, so a name containing `.` or `(` would
    // otherwise match tests it should not.
    let names = vec!["handles a.b (edge case)".to_string()];
    let inv = build(TestRunner::Jest, PackageManager::Npm, Some(&names));

    let idx = inv.args.iter().position(|a| a == "-t").unwrap();
    let pattern = &inv.args[idx + 1];
    assert!(pattern.contains(r"a\.b"), "got {pattern}");
    assert!(pattern.contains(r"\(edge case\)"), "got {pattern}");
}

#[test]
fn rerun_filter_joins_multiple_names_as_alternatives() {
    let names = vec!["alpha".to_string(), "beta".to_string()];
    let inv = build(TestRunner::Vitest, PackageManager::Npm, Some(&names));

    let idx = inv.args.iter().position(|a| a == "-t").unwrap();
    assert_eq!(inv.args[idx + 1], "^(alpha|beta)$");
}

#[test]
fn rerun_warns_that_name_matching_can_over_select() {
    let names = vec!["alpha".to_string()];
    let inv = build(TestRunner::Vitest, PackageManager::Npm, Some(&names));
    assert!(inv.warnings.iter().any(|w| w.contains("share a name")));
}

#[test]
fn no_filter_means_no_name_argument() {
    let inv = build(TestRunner::Vitest, PackageManager::Npm, Some(&[]));
    assert!(!inv.args.iter().any(|a| a == "-t"));
    assert!(inv.warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Script invocations
// ---------------------------------------------------------------------------

#[test]
fn runs_a_named_script_in_the_project_directory() {
    let mut c = RunConfig::new(
        "web:dev",
        "dev",
        RunKind::App,
        "node",
        ConfigSource::Detected,
    );
    c.project = Some(PathBuf::from("apps/web"));
    c.script = Some("dev".into());

    let inv = script_invocation(
        &c,
        Path::new("/repo"),
        Path::new("/repo/apps/web"),
        PackageManager::Pnpm,
    );

    assert_eq!(inv.program, "pnpm");
    assert_eq!(inv.args, vec!["run", "dev"]);
    assert_eq!(inv.cwd, PathBuf::from("/repo/apps/web"));
}

#[test]
fn npm_needs_a_separator_before_script_arguments() {
    // Without `--`, npm consumes the arguments itself instead of forwarding.
    let mut c = RunConfig::new(
        "web:dev",
        "dev",
        RunKind::App,
        "node",
        ConfigSource::Detected,
    );
    c.script = Some("dev".into());
    c.args = vec!["--port".into(), "3001".into()];

    let inv = script_invocation(
        &c,
        Path::new("/repo"),
        Path::new("/repo"),
        PackageManager::Npm,
    );
    assert_eq!(inv.args, vec!["run", "dev", "--", "--port", "3001"]);
}

#[test]
fn yarn_forwards_script_arguments_without_a_separator() {
    let mut c = RunConfig::new(
        "web:dev",
        "dev",
        RunKind::App,
        "node",
        ConfigSource::Detected,
    );
    c.script = Some("dev".into());
    c.args = vec!["--port".into(), "3001".into()];

    let inv = script_invocation(
        &c,
        Path::new("/repo"),
        Path::new("/repo"),
        PackageManager::Yarn,
    );
    assert_eq!(inv.args, vec!["dev", "--port", "3001"]);
}

// ---------------------------------------------------------------------------
// Generated configurations
// ---------------------------------------------------------------------------

#[test]
fn generates_a_test_config_and_one_per_runnable_script() {
    let p = pkg(r#"{
      "name": "web",
      "scripts": { "dev": "vite", "build": "vite build", "test": "vitest" },
      "devDependencies": { "vitest": "^2" }
    }"#);

    let configs = configs_for_project("web", "web", Path::new("apps/web"), &p);

    assert_eq!(
        configs.iter().filter(|c| c.kind == RunKind::Test).count(),
        1
    );
    assert!(configs.iter().any(|c| c.script.as_deref() == Some("dev")));
    assert!(configs.iter().any(|c| c.script.as_deref() == Some("build")));
}

#[test]
fn the_test_script_is_not_offered_as_an_app_configuration() {
    // Running `npm test` directly would skip the reporting arguments, so the
    // run would produce no results in the Tests view.
    let p = pkg(r#"{"scripts":{"test":"vitest"},"devDependencies":{"vitest":"^2"}}"#);
    let configs = configs_for_project("web", "web", Path::new("."), &p);

    assert!(!configs.iter().any(|c| c.script.as_deref() == Some("test")));
}

#[test]
fn lifecycle_hook_scripts_are_not_offered() {
    let p = pkg(r#"{"scripts":{"prebuild":"x","build":"y","postbuild":"z"}}"#);
    let configs = configs_for_project("web", "web", Path::new("."), &p);
    let scripts: Vec<&str> = configs.iter().filter_map(|c| c.script.as_deref()).collect();

    assert_eq!(
        scripts,
        vec!["build"],
        "npm runs pre/post hooks automatically"
    );
}

#[test]
fn a_package_without_tests_gets_no_test_config() {
    let p = pkg(r#"{"scripts":{"build":"tsc"}}"#);
    let configs = configs_for_project("lib", "lib", Path::new("."), &p);
    assert!(!configs.iter().any(|c| c.kind == RunKind::Test));
}
