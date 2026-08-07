//! The JavaScript / TypeScript ecosystem adapter.
//!
//! The one detail worth knowing here: Vitest's JSON reporter *replaces* the
//! console reporter, so asking for a machine-readable report the obvious way
//! silences the live output entirely. Both reporters are therefore requested
//! together, which is also why the report filename has to use the
//! `--outputFile.json=` form rather than plain `--outputFile=`.
//!
//! Jest does not have this problem — its default reporter writes to stderr —
//! but the resulting JSON is the same shape, so one parser serves both.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::{
    ConfigSource, Invocation, ProjectKind, ReportFormat, ReportSpec, RunConfig, RunKind, TestRunner,
};

/// The parts of a `package.json` this adapter uses.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
pub struct PackageJson {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    pub dev_dependencies: BTreeMap<String, String>,
    /// Workspace globs, used to avoid treating a monorepo root as a project.
    #[serde(default)]
    pub workspaces: Option<serde_json::Value>,
}

impl PackageJson {
    pub fn depends_on(&self, package: &str) -> bool {
        self.dependencies.contains_key(package) || self.dev_dependencies.contains_key(package)
    }
}

pub fn parse_package_json(json: &str) -> Option<PackageJson> {
    serde_json::from_str(json).ok()
}

/// Which package manager to drive, inferred from the lockfile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    /// The program to invoke.
    pub fn program(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
            PackageManager::Bun => "bun",
        }
    }

    /// Arguments that run a named script.
    pub fn run_script_args(&self, script: &str) -> Vec<String> {
        match self {
            // yarn and bun take the script name directly; npm and pnpm need
            // the `run` verb for scripts whose names collide with built-ins.
            PackageManager::Yarn | PackageManager::Bun => vec![script.to_string()],
            _ => vec!["run".to_string(), script.to_string()],
        }
    }

    /// Arguments that execute a binary from `node_modules/.bin`.
    pub fn exec_args(&self, binary: &str) -> (String, Vec<String>) {
        match self {
            PackageManager::Npm => ("npx".to_string(), vec![binary.to_string()]),
            PackageManager::Pnpm => ("pnpm".to_string(), vec!["exec".into(), binary.to_string()]),
            PackageManager::Yarn => ("yarn".to_string(), vec![binary.to_string()]),
            PackageManager::Bun => ("bunx".to_string(), vec![binary.to_string()]),
        }
    }

    /// Extra arguments needed before a script's own arguments.
    ///
    /// npm requires an explicit `--` separator or it consumes them itself;
    /// the others forward everything after the script name.
    pub fn script_arg_separator(&self) -> Option<&'static str> {
        match self {
            PackageManager::Npm | PackageManager::Pnpm => Some("--"),
            _ => None,
        }
    }
}

/// Infer the package manager from the lockfiles present in a directory.
///
/// Checked most-specific first, since a repository migrating between managers
/// often has more than one lockfile lying around.
pub fn detect_package_manager(dir: &Path) -> PackageManager {
    if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
        PackageManager::Bun
    } else if dir.join("pnpm-lock.yaml").exists() {
        PackageManager::Pnpm
    } else if dir.join("yarn.lock").exists() {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

/// Identify the test runner a package uses.
pub fn detect_runner(pkg: &PackageJson) -> Option<TestRunner> {
    if pkg.depends_on("vitest") {
        Some(TestRunner::Vitest)
    } else if pkg.depends_on("jest") || pkg.depends_on("ts-jest") {
        Some(TestRunner::Jest)
    } else {
        None
    }
}

/// Whether a package.json describes a monorepo root rather than a project.
pub fn is_workspace_root(pkg: &PackageJson) -> bool {
    pkg.workspaces.is_some()
}

pub fn project_kind(pkg: &PackageJson) -> ProjectKind {
    if detect_runner(pkg).is_some() {
        ProjectKind::Test
    } else if pkg.scripts.contains_key("start") || pkg.scripts.contains_key("dev") {
        ProjectKind::Executable
    } else {
        ProjectKind::Library
    }
}

/// Build the test invocation for a Node project.
pub fn test_invocation(
    config: &RunConfig,
    workspace_root: &Path,
    project_dir: &Path,
    manager: PackageManager,
    runner: TestRunner,
    results_dir: &Path,
    filter: Option<&[String]>,
) -> Invocation {
    let report_path = results_dir.join(format!("{}.json", sanitise(&config.id)));
    let mut warnings = Vec::new();

    let (program, mut args) = match runner {
        TestRunner::Vitest => manager.exec_args("vitest"),
        _ => manager.exec_args("jest"),
    };

    match runner {
        TestRunner::Vitest => {
            // `run` disables watch mode, which would otherwise never exit.
            args.push("run".into());
            // Asking for the JSON reporter alone would replace the console
            // reporter and silence the live output the console pane shows.
            args.push("--reporter=default".into());
            args.push("--reporter=json".into());
            // With more than one reporter the output file must be qualified
            // by reporter name, or Vitest writes the console output there.
            args.push(format!("--outputFile.json={}", report_path.display()));

            if let Some(names) = filter {
                if !names.is_empty() {
                    args.push("-t".into());
                    args.push(name_pattern(names));
                }
            }
        }
        _ => {
            args.push("--json".into());
            args.push(format!("--outputFile={}", report_path.display()));
            // Jest watches by default only in a git repo with no CI env var;
            // being explicit avoids a run that never returns.
            args.push("--watchAll=false".into());

            if let Some(names) = filter {
                if !names.is_empty() {
                    args.push("-t".into());
                    args.push(name_pattern(names));
                }
            }
        }
    }

    if let Some(names) = filter {
        if !names.is_empty() {
            warnings.push(
                "Re-running failed tests matches by test name. Tests in different files that \
                 share a name will all run."
                    .to_string(),
            );
        }
    }

    args.extend(config.args.iter().cloned());

    Invocation {
        program,
        args,
        cwd: config
            .cwd
            .as_ref()
            .map(|c| workspace_root.join(c))
            .unwrap_or_else(|| project_dir.to_path_buf()),
        env: config.env.clone(),
        report: Some(ReportSpec {
            path: report_path,
            format: ReportFormat::JestLike,
        }),
        warnings,
    }
}

/// Build the invocation that runs a package.json script.
pub fn script_invocation(
    config: &RunConfig,
    workspace_root: &Path,
    project_dir: &Path,
    manager: PackageManager,
) -> Invocation {
    let script = config.script.clone().unwrap_or_else(|| "start".to_string());
    let mut args = manager.run_script_args(&script);

    if !config.args.is_empty() {
        if let Some(separator) = manager.script_arg_separator() {
            args.push(separator.to_string());
        }
        args.extend(config.args.iter().cloned());
    }

    Invocation {
        program: manager.program().to_string(),
        args,
        cwd: config
            .cwd
            .as_ref()
            .map(|c| workspace_root.join(c))
            .unwrap_or_else(|| project_dir.to_path_buf()),
        env: config.env.clone(),
        report: None,
        warnings: Vec::new(),
    }
}

/// Build a regular expression matching exactly the given test names.
///
/// Jest and Vitest both filter by name with `-t`, treating the value as a
/// regular expression, so every name has to be escaped or a test called
/// `handles a.b` would match far more than itself.
fn name_pattern(full_names: &[String]) -> String {
    let alternatives: Vec<String> = full_names.iter().map(|n| regex_escape(n)).collect();
    format!("^({})$", alternatives.join("|"))
}

fn regex_escape(input: &str) -> String {
    const SPECIAL: &[char] = &[
        '\\', '.', '+', '*', '?', '(', ')', '|', '[', ']', '{', '}', '^', '$', '/', '-',
    ];
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if SPECIAL.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
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

/// Scripts that are worth offering as run configurations.
///
/// Test scripts are excluded: tests are launched through the Tests view,
/// which needs the reporting arguments this adapter adds.
fn is_runnable_script(name: &str) -> bool {
    !matches!(name, "test" | "test:watch" | "test:ci" | "coverage")
        && !name.starts_with("pre")
        && !name.starts_with("post")
}

/// Create configurations for a discovered Node project.
pub fn configs_for_project(
    project_id: &str,
    project_name: &str,
    relative_dir: &Path,
    pkg: &PackageJson,
) -> Vec<RunConfig> {
    let mut out = Vec::new();

    if detect_runner(pkg).is_some() {
        let mut config = RunConfig::new(
            format!("{project_id}:test"),
            format!("{project_name} tests"),
            RunKind::Test,
            "node",
            ConfigSource::Detected,
        );
        config.project = Some(relative_dir.to_path_buf());
        out.push(config);
    }

    for name in pkg.scripts.keys().filter(|n| is_runnable_script(n)) {
        let mut config = RunConfig::new(
            format!("{project_id}:script:{}", sanitise(name)),
            format!("{project_name}: {name}"),
            RunKind::App,
            "node",
            ConfigSource::Detected,
        );
        config.project = Some(relative_dir.to_path_buf());
        config.script = Some(name.clone());
        out.push(config);
    }

    out
}

/// Resolve a project directory relative to the workspace root.
pub fn project_dir(workspace_root: &Path, relative: Option<&PathBuf>) -> PathBuf {
    match relative {
        Some(r) => workspace_root.join(r),
        None => workspace_root.to_path_buf(),
    }
}
