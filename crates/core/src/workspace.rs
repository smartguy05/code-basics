//! Scanning a workspace for projects and building the configurations that can
//! run them.
//!
//! Detection is intentionally filesystem-only: no MSBuild evaluation, no `npm
//! ls`, no shelling out at all. Opening a workspace has to feel instant, and
//! everything the UI needs to show a list of things to run can be read
//! straight off disk.
//!
//! [`ScanOptions::msbuild_evaluation`] is the one opt-out, off unless a
//! workspace asks for it: some repositories hide the properties that matter
//! behind MSBuild conditions, which no amount of XML reading can resolve.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use walkdir::WalkDir;

use crate::adapters::{dotnet, manifest, msbuild, node, solution};
use crate::model::{Project, ProjectKind, RunConfig, TestRunner};

/// Directories never worth descending into.
///
/// `bin` and `obj` in particular contain copies of project files that would
/// otherwise be detected as projects in their own right.
const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "bin", "obj", "target", "dist", ".next", ".nuxt", ".vs", ".idea",
    ".vscode", "TestResults", ".code-basics",
];

/// How deep to descend. Deep enough for a conventional `src/Area/Project`
/// layout, shallow enough that a stray large directory cannot stall a scan.
const MAX_DEPTH: usize = 10;

/// Everything discovered about an opened workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    pub projects: Vec<Project>,
    pub configs: Vec<RunConfig>,
    /// .NET solutions found in the workspace, for grouping projects the way
    /// the repository already groups them. Purely presentational: nothing is
    /// run through a solution.
    #[serde(default)]
    pub solutions: Vec<solution::Solution>,
    /// Ids of starred configurations, from `.code-basics/config.json`. The
    /// scan leaves this empty; layering the saved file on fills it in.
    #[serde(default)]
    pub favorites: Vec<String>,
    /// The user's preferred config ordering, same source as `favorites`.
    #[serde(default)]
    pub order: Vec<String>,
}

/// Whether a directory name is in `SKIP_DIRS`. Shared with the file tree
/// (`files::list_dir`), so what the tree shows matches what the scan sees.
pub(crate) fn should_skip(name: &str) -> bool {
    SKIP_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
}

/// Project-relative id, stable across machines because it never contains an
/// absolute path.
fn project_id(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let id = relative.to_string_lossy().replace(['/', '\\'], "-");
    if id.is_empty() {
        "root".to_string()
    } else {
        id
    }
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Collect `Directory.Build.props` files that apply to a project, nearest first.
///
/// MSBuild only imports the closest one by default, but projects commonly
/// chain them explicitly, and for classification purposes seeing all of them
/// is strictly more informative than seeing one.
fn inherited_props(root: &Path, project_dir: &Path) -> Vec<dotnet::ProjectFile> {
    let mut props = Vec::new();
    let mut dir = Some(project_dir);

    while let Some(current) = dir {
        for name in ["Directory.Build.props", "Directory.Build.targets"] {
            if let Ok(content) = std::fs::read_to_string(current.join(name)) {
                props.push(dotnet::parse_project_file(&content));
            }
        }
        if current == root {
            break;
        }
        dir = current.parent();
    }

    props
}

/// Find the `dotnet.config` runner selection that applies to a project.
///
/// Searched from the project directory upwards, so a solution-level file
/// applies to everything beneath it.
fn configured_runner(root: &Path, project_dir: &Path) -> Option<dotnet::ConfiguredRunner> {
    let mut dir = Some(project_dir);

    while let Some(current) = dir {
        if let Ok(content) = std::fs::read_to_string(current.join("dotnet.config")) {
            if let Some(runner) = dotnet::parse_dotnet_config(&content) {
                return Some(runner);
            }
        }
        if current == root {
            break;
        }
        dir = current.parent();
    }
    None
}

/// Read the launch profiles beside a .NET project, if any.
pub fn launch_profiles(project_path: &Path) -> Vec<dotnet::LaunchProfile> {
    let Some(dir) = project_path.parent() else {
        return Vec::new();
    };
    match std::fs::read_to_string(dir.join("Properties").join("launchSettings.json")) {
        Ok(content) => dotnet::parse_launch_settings(&content),
        Err(_) => Vec::new(),
    }
}

/// How thoroughly a scan should inspect projects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanOptions {
    /// Refine .NET projects with `dotnet msbuild -getProperty` after reading
    /// them as XML. Opt-in; see [`crate::adapters::msbuild`].
    pub msbuild_evaluation: bool,
}

/// Scan a workspace root for projects and derive run configurations.
///
/// Filesystem-only. Use [`scan_with`] to opt into MSBuild evaluation.
pub fn scan(root: &Path) -> Result<Workspace> {
    scan_with(root, ScanOptions::default())
}

/// Scan a workspace root, choosing how thoroughly projects are inspected.
pub fn scan_with(root: &Path, options: ScanOptions) -> Result<Workspace> {
    // dunce keeps Windows paths in their familiar `C:\...` form instead of the
    // `\\?\C:\...` verbatim form std's canonicalize produces.
    let root = dunce::canonicalize(root)
        .with_context(|| format!("workspace root does not exist: {}", root.display()))?;

    let mut projects = Vec::new();
    let mut configs = Vec::new();
    let mut solutions = Vec::new();

    let walker = WalkDir::new(&root)
        .max_depth(MAX_DEPTH)
        .into_iter()
        .filter_entry(|e| {
            // Always accept the root itself, or nothing is scanned.
            if e.depth() == 0 {
                return true;
            }
            if e.file_name().to_str().is_some_and(should_skip) {
                return false;
            }
            // A directory with its own `.git` entry is a separate checkout — a
            // nested repository, submodule or worktree (worktrees keep `.git`
            // as a file). Its projects belong to that checkout, not to this
            // workspace, and detecting them duplicates every project once per
            // copy.
            !(e.file_type().is_dir() && e.path().join(".git").exists())
        });

    // Declarative adapters are workspace-local, so they are loaded once per
    // scan rather than baked in. A manifest that fails to parse is skipped
    // here; `invocation::build_from_manifest` reports the parse error when the
    // user tries to run something that needed it.
    let (manifests, _errors) = manifest::load_dir(&manifest::manifest_dir(&root));
    let mut manifest_dirs: Vec<(PathBuf, usize)> = Vec::new();

    for entry in walker.flatten() {
        let path = entry.path();

        if entry.file_type().is_dir() {
            // First manifest wins, so a directory matching two adapters does
            // not become two projects sharing one directory.
            if let Some(index) = manifests.iter().position(|m| manifest::matches(m, path)) {
                manifest_dirs.push((path.to_path_buf(), index));
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let is_dotnet_project = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "csproj" | "fsproj" | "vbproj"));

        if solution::is_solution_file(path) {
            if let Some(found) = scan_solution(&root, path) {
                solutions.push(found);
            }
        } else if is_dotnet_project {
            if let Some((project, mut project_configs)) = scan_dotnet_project(&root, path, options) {
                projects.push(project);
                configs.append(&mut project_configs);
            }
        } else if name == "package.json" {
            if let Some((project, mut project_configs)) = scan_node_project(&root, path) {
                projects.push(project);
                configs.append(&mut project_configs);
            }
        }
    }

    // Declarative adapters extend the built-in ones rather than override them:
    // a directory .NET or Node already claimed keeps its built-in project.
    for (dir, index) in manifest_dirs {
        if projects.iter().any(|p| p.dir == dir) {
            continue;
        }
        if let Some((project, mut project_configs)) =
            scan_manifest_project(&root, &dir, &manifests[index])
        {
            projects.push(project);
            configs.append(&mut project_configs);
        }
    }

    projects.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    configs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    solutions.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));

    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());

    Ok(Workspace {
        root,
        name,
        projects,
        configs,
        solutions,
        favorites: Vec::new(),
        order: Vec::new(),
    })
}

/// Read a `.sln` or `.slnx` for the projects it groups.
fn scan_solution(root: &Path, path: &Path) -> Option<solution::Solution> {
    let content = std::fs::read_to_string(path).ok()?;
    let name = path.file_stem()?.to_string_lossy().into_owned();
    let relative_path = relative(root, path);
    let relative_dir = relative_path.parent().unwrap_or(Path::new("")).to_path_buf();
    let is_xml = path.extension().and_then(|e| e.to_str()) == Some("slnx");

    let projects = solution::parse(&name, &content, &relative_dir, is_xml);
    if projects.is_empty() {
        return None;
    }

    Some(solution::Solution {
        name,
        path: relative_path,
        projects,
    })
}

fn scan_dotnet_project(
    root: &Path,
    path: &Path,
    options: ScanOptions,
) -> Option<(Project, Vec<RunConfig>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut parsed = dotnet::parse_project_file(&content);
    let dir = path.parent()?.to_path_buf();

    // The XML scan is always run first: MSBuild returns properties but not
    // items, so package references — which drive test-runner classification —
    // only ever come from the file itself.
    if options.msbuild_evaluation {
        if let Some(evaluated) = msbuild::evaluate(path) {
            msbuild::apply(&mut parsed, &evaluated);
        }
    }

    let props = inherited_props(root, &dir);
    let is_test = dotnet::is_test_project(&parsed, &props);
    let kind = dotnet::project_kind(&parsed, &props, is_test);

    let test_runner =
        is_test.then(|| dotnet::classify_runner(&parsed, &props, configured_runner(root, &dir)));

    let name = path.file_stem()?.to_string_lossy().into_owned();
    let id = project_id(root, path);
    let rel = relative(root, path);
    let configurations = dotnet::configurations(&parsed, &props);

    let configs = dotnet::configs_for_project(
        &id,
        &name,
        &rel,
        kind,
        &parsed.target_frameworks,
        &configurations,
        &launch_profiles(path),
    );

    Some((
        Project {
            id,
            name,
            manifest_path: path.to_path_buf(),
            dir,
            ecosystem: "dotnet".into(),
            kind,
            frameworks: parsed.target_frameworks,
            configurations,
            is_test_project: is_test,
            test_runner,
        },
        configs,
    ))
}

/// Build a project from a directory a declarative adapter claimed.
fn scan_manifest_project(
    root: &Path,
    dir: &Path,
    adapter: &manifest::AdapterManifest,
) -> Option<(Project, Vec<RunConfig>)> {
    let manifest_path = manifest::matched_file(adapter, dir)?;

    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| adapter.name.clone());
    let id = project_id(root, dir);
    let rel = relative(root, dir);

    let is_test = adapter.test.is_some();
    let kind = if is_test {
        ProjectKind::Test
    } else if !adapter.run.is_empty() {
        ProjectKind::Executable
    } else {
        ProjectKind::Unknown
    };

    let configs = manifest::configs_for_project(adapter, &id, &name, &rel);

    Some((
        Project {
            id,
            name,
            manifest_path,
            dir: dir.to_path_buf(),
            ecosystem: adapter.id.clone(),
            kind,
            frameworks: Vec::new(),
            configurations: Vec::new(),
            is_test_project: is_test,
            // The concrete runner lives in the manifest, not in a Rust enum.
            test_runner: is_test.then_some(TestRunner::Custom),
        },
        configs,
    ))
}

fn scan_node_project(root: &Path, path: &Path) -> Option<(Project, Vec<RunConfig>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed = node::parse_package_json(&content)?;

    // A monorepo root describes where the packages are rather than being a
    // project itself. Its own scripts are still worth offering when it has
    // any, so only a script-less root is skipped outright.
    if node::is_workspace_root(&parsed) && parsed.scripts.is_empty() {
        return None;
    }

    let dir = path.parent()?.to_path_buf();
    let kind = node::project_kind(&parsed);
    let test_runner = node::detect_runner(&parsed);

    let name = parsed.name.clone().unwrap_or_else(|| {
        dir.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "package".into())
    });
    let id = project_id(root, &dir);
    let rel = relative(root, &dir);

    let configs = node::configs_for_project(&id, &name, &rel, &parsed);

    Some((
        Project {
            id,
            name,
            manifest_path: path.to_path_buf(),
            dir,
            ecosystem: "node".into(),
            kind,
            frameworks: Vec::new(),
            configurations: Vec::new(),
            is_test_project: test_runner.is_some(),
            test_runner,
        },
        configs,
    ))
}

/// Look up a project by the path a configuration targets.
pub fn find_project<'a>(workspace: &'a Workspace, config: &RunConfig) -> Option<&'a Project> {
    let target = config.project.as_ref()?;
    let absolute = workspace.root.join(target);

    workspace
        .projects
        .iter()
        // .NET configs point at the project file, Node configs at its directory.
        .find(|p| p.manifest_path == absolute || p.dir == absolute)
}

/// Per-project context needed to build a .NET test invocation: which runner is
/// in use, and whether TRX reporting is available.
///
/// Read fresh from disk at run time rather than cached from the scan, so
/// editing a project file takes effect without reopening the workspace.
pub fn dotnet_test_context(root: &Path, project_path: &Path) -> (Option<TestRunner>, bool) {
    let Ok(content) = std::fs::read_to_string(project_path) else {
        return (None, false);
    };
    let parsed = dotnet::parse_project_file(&content);
    let Some(dir) = project_path.parent() else {
        return (None, false);
    };

    let props = inherited_props(root, dir);
    let runner = dotnet::classify_runner(&parsed, &props, configured_runner(root, dir));
    let trx = dotnet::has_trx_extension(&parsed, &props);

    (Some(runner), trx)
}

/// Group configurations by the project they belong to, for the UI's sidebar.
pub fn configs_by_project(workspace: &Workspace) -> BTreeMap<String, Vec<&RunConfig>> {
    let mut grouped: BTreeMap<String, Vec<&RunConfig>> = BTreeMap::new();
    for config in &workspace.configs {
        let key = find_project(workspace, config)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Workspace".to_string());
        grouped.entry(key).or_default().push(config);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RunKind;

    /// Build a workspace on disk from `(relative path, contents)` pairs.
    fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        dir
    }

    const XUNIT_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>
      <ItemGroup>
        <PackageReference Include="xunit" Version="2.9.0" />
        <PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.11.0" />
      </ItemGroup>
    </Project>"#;

    const EXE_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup>
        <OutputType>Exe</OutputType>
        <TargetFramework>net8.0</TargetFramework>
      </PropertyGroup>
    </Project>"#;

    #[test]
    fn finds_dotnet_projects_and_classifies_them() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("src/App.Tests/App.Tests.csproj", XUNIT_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 2);

        let app = ws.projects.iter().find(|p| p.name == "App").unwrap();
        assert_eq!(app.kind, ProjectKind::Executable);
        assert!(!app.is_test_project);

        let tests = ws.projects.iter().find(|p| p.name == "App.Tests").unwrap();
        assert_eq!(tests.kind, ProjectKind::Test);
        assert_eq!(tests.test_runner, Some(TestRunner::VsTest));
        assert_eq!(tests.frameworks, vec!["net8.0"]);
    }

    #[test]
    fn ignores_build_output_directories() {
        // bin/ and obj/ contain copies of project files; detecting them would
        // produce phantom projects that cannot be built.
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("src/App/bin/Debug/net8.0/App.csproj", EXE_CSPROJ),
            ("src/App/obj/App.csproj", EXE_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
    }

    #[test]
    fn ignores_nested_checkouts() {
        // A nested repository (`.git` directory) or worktree (`.git` file) is
        // a separate checkout; scanning into it duplicates every project once
        // per copy.
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("vendored/.git/HEAD", "ref: refs/heads/main"),
            ("vendored/App/App.csproj", EXE_CSPROJ),
            (".claude/worktrees/wt1/.git", "gitdir: elsewhere"),
            (".claude/worktrees/wt1/src/App/App.csproj", EXE_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].id, "src-App-App.csproj");
    }

    #[test]
    fn ignores_node_modules() {
        let dir = workspace_with(&[
            ("package.json", r#"{"name":"app","scripts":{"dev":"vite"}}"#),
            ("node_modules/dep/package.json", r#"{"name":"dep"}"#),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].name, "app");
    }

    #[test]
    fn mtp_detection_reads_a_solution_level_dotnet_config() {
        let dir = workspace_with(&[
            ("dotnet.config", "[dotnet.test:runner]\nname = \"Microsoft.Testing.Platform\"\n"),
            ("src/App.Tests/App.Tests.csproj", XUNIT_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        let tests = ws.projects.iter().find(|p| p.is_test_project).unwrap();
        assert_eq!(
            tests.test_runner,
            Some(TestRunner::MicrosoftTestingPlatform),
            "a dotnet.config above the project must apply to it"
        );
    }

    #[test]
    fn mtp_detection_reads_directory_build_props() {
        let dir = workspace_with(&[
            (
                "Directory.Build.props",
                r#"<Project><PropertyGroup>
                   <TestingPlatformDotnetTestSupport>true</TestingPlatformDotnetTestSupport>
                   </PropertyGroup></Project>"#,
            ),
            ("src/App.Tests/App.Tests.csproj", XUNIT_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        let tests = ws.projects.iter().find(|p| p.is_test_project).unwrap();
        assert_eq!(tests.test_runner, Some(TestRunner::MicrosoftTestingPlatform));
    }

    #[test]
    fn launch_profiles_become_run_configurations() {
        let dir = workspace_with(&[
            ("src/Api/Api.csproj", EXE_CSPROJ),
            (
                "src/Api/Properties/launchSettings.json",
                r#"{"profiles":{"https":{"commandName":"Project"}}}"#,
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.configs.iter().any(|c| c.launch_profile.as_deref() == Some("https")));
    }

    #[test]
    fn node_projects_are_detected_with_their_runner() {
        let dir = workspace_with(&[(
            "package.json",
            r#"{"name":"web","scripts":{"dev":"vite","test":"vitest"},"devDependencies":{"vitest":"^2"}}"#,
        )]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects[0].test_runner, Some(TestRunner::Vitest));
        assert!(ws.configs.iter().any(|c| c.kind == RunKind::Test));
        assert!(ws.configs.iter().any(|c| c.script.as_deref() == Some("dev")));
    }

    #[test]
    fn a_mixed_workspace_yields_both_ecosystems() {
        let dir = workspace_with(&[
            ("api/Api.csproj", EXE_CSPROJ),
            ("web/package.json", r#"{"name":"web","scripts":{"dev":"vite"}}"#),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.projects.iter().any(|p| p.ecosystem == "dotnet"));
        assert!(ws.projects.iter().any(|p| p.ecosystem == "node"));
    }

    #[test]
    fn a_monorepo_root_without_scripts_is_not_a_project() {
        let dir = workspace_with(&[
            ("package.json", r#"{"private":true,"workspaces":["apps/*"]}"#),
            ("apps/web/package.json", r#"{"name":"web","scripts":{"dev":"vite"}}"#),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].name, "web");
    }

    const PYTEST_ADAPTER: &str = r#"
id = "pytest"
name = "pytest"
detect = ["pyproject.toml", "pytest.ini"]

[test]
program = "pytest"
args = ["--junit-xml={report}"]
report_format = "junitXml"
"#;

    #[test]
    fn declarative_adapters_are_detected_during_a_normal_scan() {
        // The documented promise of `.code-basics/adapters/*.toml`: a manifest
        // makes its ecosystem appear like any built-in one, with no
        // hand-written config.json entry.
        let dir = workspace_with(&[
            (".code-basics/adapters/pytest.toml", PYTEST_ADAPTER),
            ("services/api/pyproject.toml", "[project]\nname = \"api\"\n"),
        ]);
        let ws = scan(dir.path()).unwrap();

        let api = ws
            .projects
            .iter()
            .find(|p| p.ecosystem == "pytest")
            .expect("the manifest must produce a project");

        assert_eq!(api.name, "api");
        assert_eq!(api.kind, ProjectKind::Test);
        assert_eq!(api.test_runner, Some(TestRunner::Custom));
        assert!(api.manifest_path.ends_with("pyproject.toml"));
        assert!(ws.configs.iter().any(|c| c.ecosystem == "pytest" && c.kind == RunKind::Test));
    }

    #[test]
    fn a_manifest_config_resolves_back_to_its_project() {
        // Manifest configs target the directory, like Node's.
        let dir = workspace_with(&[
            (".code-basics/adapters/pytest.toml", PYTEST_ADAPTER),
            ("services/api/pyproject.toml", ""),
        ]);
        let ws = scan(dir.path()).unwrap();

        for config in ws.configs.iter().filter(|c| c.ecosystem == "pytest") {
            assert!(find_project(&ws, config).is_some(), "config {} has no project", config.id);
        }
    }

    #[test]
    fn a_built_in_ecosystem_keeps_a_directory_a_manifest_also_matches() {
        // Manifests extend the built-in adapters; they do not shadow them.
        // `pyproject.toml` beside a package.json must not turn a Node project
        // into a pytest one.
        let dir = workspace_with(&[
            (".code-basics/adapters/pytest.toml", PYTEST_ADAPTER),
            ("app/package.json", r#"{"name":"app","scripts":{"dev":"vite"}}"#),
            ("app/pyproject.toml", ""),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].ecosystem, "node");
    }

    #[test]
    fn a_workspace_with_no_manifests_is_unaffected() {
        let dir = workspace_with(&[("services/api/pyproject.toml", "")]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.projects.is_empty(), "a manifest is required to claim a directory");
    }

    #[test]
    fn multi_targeted_projects_get_a_configuration_per_framework() {
        let dir = workspace_with(&[(
            "src/Lib/App.csproj",
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
                 <OutputType>Exe</OutputType>
                 <TargetFrameworks>net8.0;net9.0</TargetFrameworks>
               </PropertyGroup></Project>"#,
        )]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.configs.iter().any(|c| c.framework.as_deref() == Some("net8.0")));
        assert!(ws.configs.iter().any(|c| c.framework.as_deref() == Some("net9.0")));
    }

    #[test]
    fn custom_build_configurations_reach_the_ui() {
        let dir = workspace_with(&[(
            "src/App/App.csproj",
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
                 <OutputType>Exe</OutputType>
                 <TargetFramework>net9.0</TargetFramework>
                 <Configurations>Debug;Release;Staging</Configurations>
               </PropertyGroup></Project>"#,
        )]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects[0].configurations, vec!["Debug", "Release", "Staging"]);
        assert!(ws.configs.iter().any(|c| c.build_configuration.as_deref() == Some("Staging")));
    }

    #[test]
    fn solutions_are_read_and_their_projects_resolve_to_scanned_ones() {
        // The point of reading a solution is grouping, so its project paths
        // have to line up with the paths the scan produced.
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("tests/App.Tests/App.Tests.csproj", XUNIT_CSPROJ),
            (
                "Repo.sln",
                r#"Project("{2150E333-8FDC-42A3-9474-1AB1AEA671C7}") = "src", "src", "{11111111-1111-1111-1111-111111111111}"
EndProject
Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "App", "src\App\App.csproj", "{33333333-3333-3333-3333-333333333333}"
EndProject
Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "App.Tests", "tests\App.Tests\App.Tests.csproj", "{44444444-4444-4444-4444-444444444444}"
EndProject
Global
	GlobalSection(NestedProjects) = preSolution
		{33333333-3333-3333-3333-333333333333} = {11111111-1111-1111-1111-111111111111}
	EndGlobalSection
EndGlobal
"#,
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.solutions.len(), 1);
        let sln = &ws.solutions[0];
        assert_eq!(sln.name, "Repo");
        assert_eq!(sln.projects.len(), 2);

        for member in &sln.projects {
            let absolute = ws.root.join(&member.path);
            assert!(
                ws.projects.iter().any(|p| p.manifest_path == absolute),
                "solution member {} must match a scanned project",
                member.path.display()
            );
        }

        let app = sln.projects.iter().find(|p| p.name == "App").unwrap();
        assert_eq!(app.folder.as_deref(), Some("src"));
    }

    #[test]
    fn a_solution_file_is_not_itself_a_project() {
        let dir = workspace_with(&[("Repo.sln", "Microsoft Visual Studio Solution File")]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.projects.is_empty());
        assert!(ws.solutions.is_empty(), "a solution with no projects is not worth reporting");
    }

    #[test]
    fn project_ids_are_relative_so_they_survive_a_move() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let ws = scan(dir.path()).unwrap();

        assert!(
            !ws.projects[0].id.contains(&dir.path().display().to_string()),
            "id must not embed an absolute path"
        );
    }

    #[test]
    fn configurations_resolve_back_to_their_project() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("web/package.json", r#"{"name":"web","scripts":{"dev":"vite"}}"#),
        ]);
        let ws = scan(dir.path()).unwrap();

        for config in &ws.configs {
            assert!(find_project(&ws, config).is_some(), "config {} has no project", config.id);
        }
    }

    /// The contract the config editor's project dropdown is written against.
    ///
    /// The UI has to put *something* in `RunConfig.project`, and the only
    /// values that resolve are workspace-relative paths: the `.csproj` for
    /// .NET, the project directory for everything else. A `Project::id` looks
    /// close enough to tempt, but ids replace separators with `-`, so they
    /// never resolve — pin both halves so neither side drifts.
    #[test]
    fn relative_paths_resolve_but_ids_do_not() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("web/package.json", r#"{"name":"web","scripts":{"dev":"vite"}}"#),
        ]);
        let ws = scan(dir.path()).unwrap();

        for project in &ws.projects {
            let absolute = if project.ecosystem == "dotnet" {
                &project.manifest_path
            } else {
                &project.dir
            };
            let target = relative(&ws.root, absolute);

            let by_path = RunConfig {
                project: Some(target.clone()),
                ..ws.configs[0].clone()
            };
            assert_eq!(
                find_project(&ws, &by_path).map(|p| &p.id),
                Some(&project.id),
                "relative path {} must resolve to {}",
                target.display(),
                project.id
            );

            // For a project sitting directly at the root the id and the
            // relative path are the same string, so there is nothing to catch.
            // Everywhere else the id has had its separators flattened and must
            // not resolve.
            if PathBuf::from(&project.id) != target {
                let by_id = RunConfig {
                    project: Some(PathBuf::from(&project.id)),
                    ..ws.configs[0].clone()
                };
                assert!(
                    find_project(&ws, &by_id).is_none(),
                    "id {} must not resolve; the UI has to send a path",
                    project.id
                );
            }
        }
    }

    #[test]
    fn test_context_is_read_fresh_from_disk() {
        let dir = workspace_with(&[("src/T/T.csproj", XUNIT_CSPROJ)]);
        let project = dir.path().join("src/T/T.csproj");

        let (runner, trx) = dotnet_test_context(dir.path(), &project);
        assert_eq!(runner, Some(TestRunner::VsTest));
        assert!(!trx, "VSTest projects do not need the MTP TRX extension");

        // Switch the project to MTP and re-read without rescanning.
        std::fs::write(
            &project,
            r#"<Project Sdk="Microsoft.NET.Sdk"><ItemGroup>
                 <PackageReference Include="xunit.v3" Version="1.0.0" />
               </ItemGroup></Project>"#,
        )
        .unwrap();

        let (runner, trx) = dotnet_test_context(dir.path(), &project);
        assert_eq!(runner, Some(TestRunner::MicrosoftTestingPlatform));
        assert!(trx, "xunit.v3 bundles TRX reporting");
    }

    #[test]
    fn an_empty_workspace_scans_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let ws = scan(dir.path()).unwrap();

        assert!(ws.projects.is_empty());
        assert!(ws.configs.is_empty());
        assert!(!ws.name.is_empty());
    }

    #[test]
    fn workspace_serialises_with_the_keys_the_ui_reads() {
        // `src/ipc/types.ts` mirrors this by hand, like the model types.
        let dir = tempfile::tempdir().unwrap();
        let ws = scan(dir.path()).unwrap();

        let json = serde_json::to_value(&ws).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();

        assert_eq!(
            keys,
            ["configs", "favorites", "name", "order", "projects", "root", "solutions"]
        );
    }

    #[test]
    fn a_missing_root_is_an_error() {
        assert!(scan(Path::new("/nonexistent/workspace")).is_err());
    }

    #[test]
    fn results_are_ordered_deterministically() {
        let dir = workspace_with(&[
            ("z/Z.csproj", EXE_CSPROJ),
            ("a/A.csproj", EXE_CSPROJ),
            ("m/M.csproj", EXE_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        let names: Vec<&str> = ws.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["A", "M", "Z"], "scan order must not depend on the filesystem");
    }
}
