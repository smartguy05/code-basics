//! Scanning a workspace for projects and building the configurations that can
//! run them.
//!
//! Detection is intentionally filesystem-only: no MSBuild evaluation, no `npm
//! ls`, no shelling out at all. Opening a workspace has to feel instant, and
//! everything the UI needs to show a list of things to run can be read
//! straight off disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use walkdir::WalkDir;

use crate::adapters::{dotnet, node};
use crate::model::{Project, RunConfig, TestRunner};

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

/// Scan a workspace root for projects and derive run configurations.
pub fn scan(root: &Path) -> Result<Workspace> {
    // dunce keeps Windows paths in their familiar `C:\...` form instead of the
    // `\\?\C:\...` verbatim form std's canonicalize produces.
    let root = dunce::canonicalize(root)
        .with_context(|| format!("workspace root does not exist: {}", root.display()))?;

    let mut projects = Vec::new();
    let mut configs = Vec::new();

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

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let is_dotnet_project = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "csproj" | "fsproj" | "vbproj"));

        if is_dotnet_project {
            if let Some((project, mut project_configs)) = scan_dotnet_project(&root, path) {
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

    projects.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    configs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));

    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());

    Ok(Workspace { root, name, projects, configs, favorites: Vec::new(), order: Vec::new() })
}

fn scan_dotnet_project(root: &Path, path: &Path) -> Option<(Project, Vec<RunConfig>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed = dotnet::parse_project_file(&content);
    let dir = path.parent()?.to_path_buf();

    let props = inherited_props(root, &dir);
    let is_test = dotnet::is_test_project(&parsed, &props);
    let kind = dotnet::project_kind(&parsed, is_test);

    let test_runner =
        is_test.then(|| dotnet::classify_runner(&parsed, &props, configured_runner(root, &dir)));

    let name = path.file_stem()?.to_string_lossy().into_owned();
    let id = project_id(root, path);
    let rel = relative(root, path);

    let configs = dotnet::configs_for_project(
        &id,
        &name,
        &rel,
        kind,
        &parsed.target_frameworks,
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
            is_test_project: is_test,
            test_runner,
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
    use crate::model::{ProjectKind, RunKind};

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

        assert_eq!(keys, ["configs", "favorites", "name", "order", "projects", "root"]);
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
