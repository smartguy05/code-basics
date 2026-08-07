//! Declarative adapters: adding an ecosystem without writing Rust.
//!
//! Because every supported runner already boils down to "run this command, then
//! read this report file", a new ecosystem rarely needs code — it needs a
//! command template and a report format. A manifest supplies both.
//!
//! JUnit XML is what makes this practical: pytest, cargo-nextest,
//! go-junit-report, Gradle, PHPUnit and RSpec can all emit it, so the parser
//! already exists for whatever gets added next.
//!
//! ```toml
//! id = "pytest"
//! name = "pytest"
//! detect = ["pytest.ini", "pyproject.toml", "tox.ini"]
//!
//! [test]
//! program = "pytest"
//! args = ["--junit-xml={report}"]
//! report_format = "junitXml"
//! report_extension = "xml"
//! filter_template = "-k"
//! filter_separator = " or "
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{ConfigSource, Invocation, ReportFormat, ReportSpec, RunConfig, RunKind};

/// A user-supplied ecosystem definition.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AdapterManifest {
    /// Stable identifier, used as the config's `ecosystem`.
    pub id: String,
    pub name: String,
    /// File names whose presence marks a project directory.
    #[serde(default)]
    pub detect: Vec<String>,
    #[serde(default)]
    pub test: Option<CommandTemplate>,
    /// Named application launches.
    #[serde(default)]
    pub run: BTreeMap<String, CommandTemplate>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CommandTemplate {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Format of the report this command leaves behind.
    #[serde(default)]
    pub report_format: Option<ReportFormat>,
    /// Extension for the generated report path. Defaults to `xml`.
    #[serde(default)]
    pub report_extension: Option<String>,
    /// Where the runner writes its report, when that location is fixed rather
    /// than passed on the command line.
    ///
    /// cargo-nextest is the motivating case: its JUnit path comes from
    /// `.config/nextest.toml`, so `{report}` never reaches it. Supports the
    /// same `{project}` and `{root}` substitutions as `args`.
    #[serde(default)]
    pub report_path: Option<String>,
    /// Flag introducing a test-name filter, e.g. `-k` for pytest.
    #[serde(default)]
    pub filter_template: Option<String>,
    /// How multiple names are joined, e.g. `" or "` for pytest.
    #[serde(default)]
    pub filter_separator: Option<String>,
}

/// Parse a manifest from TOML.
pub fn parse(toml_text: &str) -> Result<AdapterManifest> {
    let manifest: AdapterManifest =
        toml::from_str(toml_text).context("adapter manifest is not valid TOML")?;

    anyhow::ensure!(
        !manifest.id.trim().is_empty(),
        "adapter manifest needs an id"
    );
    anyhow::ensure!(
        manifest.test.is_some() || !manifest.run.is_empty(),
        "adapter manifest `{}` defines neither a test command nor any run commands",
        manifest.id
    );

    Ok(manifest)
}

/// Load every `*.toml` manifest in a directory.
///
/// A malformed manifest is skipped with its error returned alongside the ones
/// that loaded, so one bad file cannot disable the rest.
pub fn load_dir(dir: &Path) -> (Vec<AdapterManifest>, Vec<String>) {
    let mut manifests = Vec::new();
    let mut errors = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return (manifests, errors);
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|c| parse(&c))
        {
            Ok(manifest) => manifests.push(manifest),
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }

    manifests.sort_by(|a, b| a.id.cmp(&b.id));
    (manifests, errors)
}

/// Whether a manifest's detection files are present in a directory.
pub fn matches(manifest: &AdapterManifest, dir: &Path) -> bool {
    matched_file(manifest, dir).is_some()
}

/// The detection file that made a manifest match, in the order the manifest
/// lists them.
///
/// The scan records this as the project's manifest path, so a pytest project
/// points at its `pyproject.toml` the way a .NET one points at its `.csproj`.
pub fn matched_file(manifest: &AdapterManifest, dir: &Path) -> Option<PathBuf> {
    manifest
        .detect
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.exists())
}

/// Substitute `{report}`, `{project}` and `{root}` in a template argument.
fn substitute(arg: &str, report: &Path, project_dir: &Path, root: &Path) -> String {
    arg.replace("{report}", &report.display().to_string())
        .replace("{project}", &project_dir.display().to_string())
        .replace("{root}", &root.display().to_string())
}

/// Build an invocation from a manifest command template.
pub fn build_invocation(
    template: &CommandTemplate,
    config: &RunConfig,
    workspace_root: &Path,
    project_dir: &Path,
    results_dir: &Path,
    filter: Option<&[String]>,
) -> Invocation {
    let extension = template.report_extension.as_deref().unwrap_or("xml");
    let default_report = results_dir.join(format!("{}.{extension}", sanitise(&config.id)));

    // A runner that decides its own report location is read from there
    // instead; `{report}` would never reach it.
    let report_path = match &template.report_path {
        Some(fixed) => {
            let expanded = fixed
                .replace("{project}", &project_dir.display().to_string())
                .replace("{root}", &workspace_root.display().to_string());
            let path = PathBuf::from(expanded);
            if path.is_absolute() {
                path
            } else {
                project_dir.join(path)
            }
        }
        None => default_report.clone(),
    };

    let mut args: Vec<String> = template
        .args
        .iter()
        .map(|a| substitute(a, &default_report, project_dir, workspace_root))
        .collect();

    let mut warnings = Vec::new();

    match (filter, &template.filter_template) {
        (Some(names), Some(flag)) if !names.is_empty() => {
            let separator = template.filter_separator.as_deref().unwrap_or(" or ");
            args.push(flag.clone());
            args.push(names.join(separator));
        }
        // Asking to re-run failures when the manifest cannot express a filter
        // would silently run everything, which looks like the filter was
        // ignored rather than unsupported.
        (Some(names), None) if !names.is_empty() => warnings.push(format!(
            "The `{}` adapter does not define a test filter, so the whole suite will run.",
            config.ecosystem
        )),
        _ => {}
    }

    args.extend(config.args.iter().cloned());

    let mut env = template.env.clone();
    // The configuration's own environment wins over the manifest's defaults.
    env.extend(config.env.clone());

    Invocation {
        program: template.program.clone(),
        args,
        cwd: config
            .cwd
            .as_ref()
            .map(|c| workspace_root.join(c))
            .unwrap_or_else(|| project_dir.to_path_buf()),
        env,
        report: template.report_format.map(|format| ReportSpec {
            path: report_path,
            format,
        }),
        warnings,
    }
}

/// Create configurations for a project matched by a manifest.
pub fn configs_for_project(
    manifest: &AdapterManifest,
    project_id: &str,
    project_name: &str,
    relative_dir: &Path,
) -> Vec<RunConfig> {
    let mut out = Vec::new();

    if manifest.test.is_some() {
        let mut config = RunConfig::new(
            format!("{project_id}:{}:test", manifest.id),
            format!("{project_name} tests ({})", manifest.name),
            RunKind::Test,
            manifest.id.clone(),
            ConfigSource::Detected,
        );
        config.project = Some(relative_dir.to_path_buf());
        out.push(config);
    }

    for name in manifest.run.keys() {
        let mut config = RunConfig::new(
            format!("{project_id}:{}:{}", manifest.id, sanitise(name)),
            format!("{project_name}: {name}"),
            RunKind::App,
            manifest.id.clone(),
            ConfigSource::Detected,
        );
        config.project = Some(relative_dir.to_path_buf());
        config.script = Some(name.clone());
        out.push(config);
    }

    out
}

fn sanitise(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Where workspace-local manifests live.
pub fn manifest_dir(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(crate::config::CONFIG_DIR)
        .join("adapters")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PYTEST: &str = r#"
id = "pytest"
name = "pytest"
detect = ["pytest.ini", "pyproject.toml"]

[test]
program = "pytest"
args = ["--junit-xml={report}", "-q"]
report_format = "junitXml"
report_extension = "xml"
filter_template = "-k"
filter_separator = " or "
"#;

    fn config() -> RunConfig {
        let mut c = RunConfig::new(
            "api:pytest:test",
            "api tests",
            RunKind::Test,
            "pytest",
            ConfigSource::Detected,
        );
        c.project = Some(PathBuf::from("services/api"));
        c
    }

    #[test]
    fn parses_a_manifest() {
        let manifest = parse(PYTEST).unwrap();

        assert_eq!(manifest.id, "pytest");
        assert_eq!(manifest.detect, vec!["pytest.ini", "pyproject.toml"]);
        let test = manifest.test.unwrap();
        assert_eq!(test.program, "pytest");
        assert_eq!(test.report_format, Some(ReportFormat::JunitXml));
    }

    #[test]
    fn a_manifest_that_does_nothing_is_rejected() {
        let err = parse(
            r#"id = "x"
name = "X""#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("neither a test command nor any run commands"),
            "got {err}"
        );
    }

    #[test]
    fn a_manifest_without_an_id_is_rejected() {
        assert!(parse(
            r#"id = ""
name = "X"
[test]
program = "x""#
        )
        .is_err());
    }

    #[test]
    fn substitutes_the_report_path_into_arguments() {
        let manifest = parse(PYTEST).unwrap();
        let inv = build_invocation(
            manifest.test.as_ref().unwrap(),
            &config(),
            Path::new("/repo"),
            Path::new("/repo/services/api"),
            Path::new("/repo/.code-basics/results"),
            None,
        );

        assert_eq!(inv.program, "pytest");
        let report = inv.report.clone().unwrap();
        assert!(inv
            .args
            .iter()
            .any(|a| a == &format!("--junit-xml={}", report.path.display())));
        assert_eq!(report.format, ReportFormat::JunitXml);
        assert_eq!(report.path.extension().unwrap(), "xml");
    }

    #[test]
    fn runs_in_the_project_directory_by_default() {
        let manifest = parse(PYTEST).unwrap();
        let inv = build_invocation(
            manifest.test.as_ref().unwrap(),
            &config(),
            Path::new("/repo"),
            Path::new("/repo/services/api"),
            Path::new("/repo/results"),
            None,
        );
        assert_eq!(inv.cwd, PathBuf::from("/repo/services/api"));
    }

    #[test]
    fn applies_a_filter_using_the_manifests_own_syntax() {
        let manifest = parse(PYTEST).unwrap();
        let names = vec!["test_a".to_string(), "test_b".to_string()];
        let inv = build_invocation(
            manifest.test.as_ref().unwrap(),
            &config(),
            Path::new("/repo"),
            Path::new("/repo/services/api"),
            Path::new("/repo/results"),
            Some(&names),
        );

        let idx = inv
            .args
            .iter()
            .position(|a| a == "-k")
            .expect("filter flag");
        assert_eq!(inv.args[idx + 1], "test_a or test_b");
    }

    #[test]
    fn warns_when_a_manifest_cannot_express_a_filter() {
        // Silently running everything looks like the filter was ignored.
        let manifest = parse(
            r#"
id = "go"
name = "Go"
[test]
program = "gotestsum"
args = ["--junitfile={report}"]
report_format = "junitXml"
"#,
        )
        .unwrap();

        let names = vec!["TestThing".to_string()];
        let inv = build_invocation(
            manifest.test.as_ref().unwrap(),
            &config(),
            Path::new("/repo"),
            Path::new("/repo"),
            Path::new("/repo/results"),
            Some(&names),
        );

        assert!(inv
            .warnings
            .iter()
            .any(|w| w.contains("does not define a test filter")));
    }

    #[test]
    fn configuration_environment_overrides_the_manifests() {
        let manifest = parse(
            r#"
id = "x"
name = "X"
[test]
program = "x"
env = { MODE = "manifest", KEEP = "yes" }
"#,
        )
        .unwrap();

        let mut c = config();
        c.env.insert("MODE".into(), "config".into());

        let inv = build_invocation(
            manifest.test.as_ref().unwrap(),
            &c,
            Path::new("/repo"),
            Path::new("/repo"),
            Path::new("/repo/results"),
            None,
        );

        assert_eq!(inv.env.get("MODE").map(String::as_str), Some("config"));
        assert_eq!(inv.env.get("KEEP").map(String::as_str), Some("yes"));
    }

    #[test]
    fn detection_matches_on_any_listed_file() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = parse(PYTEST).unwrap();

        assert!(!matches(&manifest, dir.path()));
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert!(matches(&manifest, dir.path()));
    }

    /// The scan records this as the project's manifest path, so which file
    /// matched is the difference between pointing at `pyproject.toml` and
    /// pointing at nothing.
    #[test]
    fn detection_reports_which_file_matched() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = parse(PYTEST).unwrap();

        assert_eq!(matched_file(&manifest, dir.path()), None);

        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(
            matched_file(&manifest, dir.path()),
            Some(dir.path().join("pyproject.toml"))
        );
    }

    /// The manifest lists `pytest.ini` before `pyproject.toml`, and a project
    /// with both must report the more specific one the manifest named first.
    #[test]
    fn the_first_listed_detection_file_wins_when_several_are_present() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = parse(PYTEST).unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        std::fs::write(dir.path().join("pytest.ini"), "").unwrap();

        assert_eq!(
            matched_file(&manifest, dir.path()),
            Some(dir.path().join("pytest.ini"))
        );
    }

    #[test]
    fn a_manifest_that_declares_no_detection_files_never_matches() {
        // Without this, an adapter that forgot `detect` would claim every
        // directory in the workspace.
        let dir = tempfile::tempdir().unwrap();
        let manifest = parse(
            r#"
id = "x"
name = "X"
[test]
program = "x"
"#,
        )
        .unwrap();

        assert_eq!(matched_file(&manifest, dir.path()), None);
        assert!(!matches(&manifest, dir.path()));
    }

    /// `matches` is defined as "something matched", so the two must never
    /// disagree — a directory that matches has a file to point at.
    #[test]
    fn matching_and_the_matched_file_always_agree() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = parse(PYTEST).unwrap();

        assert_eq!(
            matches(&manifest, dir.path()),
            matched_file(&manifest, dir.path()).is_some()
        );
        std::fs::write(dir.path().join("pytest.ini"), "").unwrap();
        assert_eq!(
            matches(&manifest, dir.path()),
            matched_file(&manifest, dir.path()).is_some()
        );
    }

    #[test]
    fn manifests_are_read_from_the_workspaces_own_config_directory() {
        // Declarative adapters are per-workspace, not global: a manifest is
        // part of the repository that needs it.
        let root = Path::new("/repo");

        assert_eq!(manifest_dir(root), Path::new("/repo/.code-basics/adapters"));
        assert!(manifest_dir(root).starts_with(crate::config::config_dir(root)));
    }

    #[test]
    fn loads_manifests_from_a_directory_reporting_bad_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pytest.toml"), PYTEST).unwrap();
        std::fs::write(dir.path().join("broken.toml"), "id = ").unwrap();
        std::fs::write(dir.path().join("notes.md"), "ignored").unwrap();

        let (manifests, errors) = load_dir(dir.path());

        assert_eq!(manifests.len(), 1, "the valid manifest must still load");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("broken.toml"));
    }

    #[test]
    fn a_missing_directory_loads_nothing_without_erroring() {
        let (manifests, errors) = load_dir(Path::new("/nonexistent/adapters"));
        assert!(manifests.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn generates_a_test_configuration_and_one_per_run_command() {
        let manifest = parse(
            r#"
id = "pytest"
name = "pytest"
[test]
program = "pytest"
[run.serve]
program = "uvicorn"
args = ["app:main"]
"#,
        )
        .unwrap();

        let configs = configs_for_project(&manifest, "api", "api", Path::new("services/api"));

        assert_eq!(configs.len(), 2);
        assert!(configs.iter().any(|c| c.kind == RunKind::Test));
        assert!(configs.iter().any(|c| c.script.as_deref() == Some("serve")));
        assert!(configs.iter().all(|c| c.ecosystem == "pytest"));
    }
}
