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

use crate::adapters::{cargo, dotnet, manifest, msbuild, node, solution};
use crate::model::{Project, ProjectKind, RunConfig, TestRunner};

/// Directories never worth descending into.
///
/// `bin` and `obj` in particular contain copies of project files that would
/// otherwise be detected as projects in their own right.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "bin",
    "obj",
    "target",
    "dist",
    ".next",
    ".nuxt",
    ".vs",
    ".idea",
    ".vscode",
    "TestResults",
    ".code-basics",
];

/// How deep to descend. Generous enough for the buried layouts monorepos and
/// generated trees produce — well past a conventional `src/Area/Project` — yet
/// shallow enough that a stray large directory cannot stall a scan. The runaway
/// cases are bounded elsewhere regardless of this number: `SKIP_DIRS` prunes
/// build output and vendored trees, nested checkouts are excluded, and
/// [`crate::symbols`] caps the index at `Limits::max_files` (50_000).
const MAX_DEPTH: usize = 50;

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

/// The one walker over a workspace's source tree.
///
/// Every consumer that wants "the files this workspace consists of" goes
/// through here: the project scan below, and the symbol index in
/// [`crate::symbols`]. That is the whole point of it being a function rather
/// than a block inlined at each call site. If the index walked the tree with
/// its own filter, the two would drift — the index would offer to jump to a
/// symbol in `bin/` or in a vendored checkout that the project list has never
/// heard of, or, worse, silently miss files that a scan does see. The rules
/// are not obvious enough to be re-derived correctly twice: `SKIP_DIRS`,
/// `MAX_DEPTH` of 50, and the exclusion of any directory carrying its own
/// `.git` entry.
///
/// The iterator is returned unconsumed and unfiltered by file type. Callers
/// want different things from the entries — the scan looks at directories to
/// match declarative adapters, an indexer only wants files — and deciding that
/// here would just push a second predicate into every caller anyway.
///
/// The predicate is a named `fn` rather than a closure purely so the return
/// type can be written down: `filter_entry` bakes its predicate into the type,
/// and a closure's type is unnameable.
pub(crate) fn source_walker(root: &Path) -> SourceWalker {
    WalkDir::new(root)
        .max_depth(MAX_DEPTH)
        .into_iter()
        .filter_entry(is_source_tree_entry as fn(&walkdir::DirEntry) -> bool)
}

/// The type [`source_walker`] returns.
pub(crate) type SourceWalker =
    walkdir::FilterEntry<walkdir::IntoIter, fn(&walkdir::DirEntry) -> bool>;

/// Whether the walk should descend into (and yield) this entry.
fn is_source_tree_entry(e: &walkdir::DirEntry) -> bool {
    // Always accept the root itself, or nothing is scanned.
    if e.depth() == 0 {
        return true;
    }
    if e.file_name().to_str().is_some_and(should_skip) {
        return false;
    }
    // A directory with its own `.git` entry is a separate checkout — a nested
    // repository, submodule or worktree (worktrees keep `.git` as a file). Its
    // projects belong to that checkout, not to this workspace, and detecting
    // them duplicates every project once per copy.
    !(e.file_type().is_dir() && e.path().join(".git").exists())
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

/// Open a directory as a workspace: scan it, then layer the saved
/// configuration over what was detected.
///
/// The two steps belong together — a scan alone loses the user's own
/// configurations, favourites and ordering — so every entry point that opens a
/// directory goes through here rather than calling [`scan`] and
/// [`crate::config::apply`] itself.
///
/// A missing saved configuration is not an error: a workspace that has never
/// been configured opens with exactly what was detected.
pub fn workspace_from_dir(path: &Path) -> Result<Workspace, String> {
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }

    let mut workspace = scan(path).map_err(|e| format!("{e:#}"))?;
    if let Ok(saved) = crate::config::load(&workspace.root) {
        crate::config::apply(&mut workspace, saved);
    }
    Ok(workspace)
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

    let walker = source_walker(&root);

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
            if let Some((project, mut project_configs)) = scan_dotnet_project(&root, path, options)
            {
                projects.push(project);
                configs.append(&mut project_configs);
            }
        } else if name == "package.json" {
            if let Some((project, mut project_configs)) = scan_node_project(&root, path) {
                projects.push(project);
                configs.append(&mut project_configs);
            }
        } else if name == "Cargo.toml" {
            if let Some((project, mut project_configs)) = scan_cargo_project(&root, path) {
                projects.push(project);
                configs.append(&mut project_configs);
            }
        }
    }

    // Declarative adapters extend the built-in ones rather than override them:
    // a directory .NET or Node already claimed keeps its built-in project.
    for (dir, index) in manifest_dirs {
        let claimants: Vec<&Project> = projects.iter().filter(|p| p.dir == dir).collect();

        if claimants.iter().any(|p| p.ecosystem != "cargo") {
            continue;
        }
        // Cargo is the one built-in adapter that produces a project and no
        // configurations at all, so shadowing a manifest with it would leave
        // the directory with nothing to run. `examples/adapters/
        // cargo-nextest.toml` detects `Cargo.toml` and is the documented way to
        // run Rust today; before this branch existed, adding built-in cargo
        // detection would have silently deleted those configurations from every
        // workspace using it on the next scan. The crate keeps the project —
        // the architecture graph needs a node with `ecosystem == "cargo"` — and
        // the manifest supplies the configurations, which target the directory
        // and so resolve back to it. An unreadable crate is left alone: it
        // carries a reason precisely because nothing about it can be trusted.
        if let Some(existing) = claimants.first() {
            if existing.unreadable.is_none() {
                let rel = relative(&root, &dir);
                configs.append(&mut manifest::configs_for_project(
                    &manifests[index],
                    &existing.id,
                    &existing.name,
                    &rel,
                ));
            }
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
    let relative_dir = relative_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
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

/// A project that is on disk but could not be read, carrying the reason.
///
/// The scan used to answer a broken manifest by returning `None`, which removed
/// the project from the list with no trace: the user saw a shorter Run tab and
/// no error, and nothing anywhere named the file that failed. This is the
/// deliberate opposite — the project keeps its identity (an id, a path, the
/// name of the file or directory, which are all facts that survive a parse
/// failure) and loses only the parts that came out of the manifest.
///
/// Everything derived is therefore left at its most non-committal value:
/// [`ProjectKind::Unknown`], no frameworks, no build configurations, not a test
/// project, and — the point of the exercise — no run configurations. A
/// configuration assembled from half a parse would be a command line built on
/// a guess, which is exactly the failure mode the rest of the crate abstains
/// from.
fn unreadable_project(
    id: String,
    name: String,
    manifest_path: PathBuf,
    dir: PathBuf,
    ecosystem: &str,
    reason: String,
) -> (Project, Vec<RunConfig>) {
    (
        Project {
            id,
            name,
            manifest_path,
            dir,
            ecosystem: ecosystem.into(),
            kind: ProjectKind::Unknown,
            frameworks: Vec::new(),
            configurations: Vec::new(),
            is_test_project: false,
            test_runner: None,
            unreadable: Some(reason),
        },
        Vec::new(),
    )
}

/// The directory's own name, for a project whose manifest could not supply one.
fn dir_name(dir: &Path, fallback: &str) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

/// Why an XML document would not parse, or `None` if it parses.
///
/// [`dotnet::parse_project_file`] is deliberately lenient — it stops at the
/// first error and returns whatever it had read by then, which is the right
/// behaviour for `Directory.Build.props` files and for a file someone is
/// halfway through editing. That leniency is also why the scan cannot tell a
/// well-formed project from a truncated one by looking at the result, so the
/// document is passed over once more here purely to ask whether it was
/// well-formed. Reading it twice costs a few microseconds on a file that is
/// already in the page cache; the alternative — changing the parser's
/// signature — would push an error case into every one of its other callers,
/// none of which want it.
///
/// The reader's own errors are not enough on their own. quick-xml reaches
/// `Event::Eof` *cleanly* on a document that simply stops — a half-saved file,
/// which is the commonest way a user meets this — so asking only "did the
/// reader error?" caught exactly one malformed shape (a mismatched end tag)
/// out of six. A file truncated after `<OutputType>Exe</OutputType>` parsed as
/// a perfectly good executable and produced Debug and Release configurations
/// from half a document. Three further checks close that:
///
/// * **Depth.** Every `Start` opens an element and every `End` closes one, so
///   reaching `Eof` at non-zero depth means the document stopped mid-element.
/// * **Attributes.** The reader does not look inside a tag until someone asks
///   for its attributes, so an unquoted value like `Sdk=Microsoft.NET.Sdk` is
///   invisible unless they are walked here.
/// * **A `Project` root.** A well-formed document that is not an MSBuild
///   project is the same wrong answer wearing a valid hat: it would be read as
///   a library with no properties and listed as a healthy project. Reporting
///   it as unreadable keeps it visible with a reason, which is the whole point
///   of [`unreadable_project`] — returning `None` instead would drop it from
///   the list silently, the failure mode this module abstains from.
///
/// The checks are deliberately about the document's *shape*, never its
/// contents: comments, processing instructions, doctypes, CDATA, namespaces
/// and prefixed element names are all left alone, because a valid `.csproj` in
/// the wild contains all of them and rejecting one would cost a healthy
/// project its run configurations.
fn xml_error(xml: &str) -> Option<String> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut depth: usize = 0;
    let mut root: Option<String> = None;

    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(e) => return Some(e.to_string()),
        };
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                if let Err(err) = e.attributes().try_for_each(|a| a.map(|_| ())) {
                    return Some(err.to_string());
                }
                if depth == 0 && root.is_none() {
                    root = Some(String::from_utf8_lossy(e.local_name().as_ref()).into_owned());
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            // `check_end_names` is on by default, so an unmatched end tag has
            // already come back as an error above; this cannot underflow.
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => {
                if depth > 0 {
                    return Some(format!(
                        "unexpected end of file: {depth} unclosed element(s)"
                    ));
                }
                return match root.as_deref() {
                    Some("Project") => None,
                    Some(other) => Some(format!(
                        "root element is <{other}>, not <Project>: not an MSBuild project file"
                    )),
                    None => Some("no <Project> root element".into()),
                };
            }
            _ => {}
        }
    }
}

fn scan_dotnet_project(
    root: &Path,
    path: &Path,
    options: ScanOptions,
) -> Option<(Project, Vec<RunConfig>)> {
    let dir = path.parent()?.to_path_buf();
    // The file stem, not a parsed property: a project that will not parse still
    // has the name the user sees in their file tree.
    let name = path.file_stem()?.to_string_lossy().into_owned();
    let id = project_id(root, path);

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return Some(unreadable_project(
                id,
                name,
                path.to_path_buf(),
                dir,
                "dotnet",
                e.to_string(),
            ))
        }
    };
    if let Some(reason) = xml_error(&content) {
        return Some(unreadable_project(
            id,
            name,
            path.to_path_buf(),
            dir,
            "dotnet",
            reason,
        ));
    }

    let mut parsed = dotnet::parse_project_file(&content);

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
            unreadable: None,
        },
        configs,
    ))
}

/// Build a project from a `Cargo.toml`.
///
/// Detection only: no run or test configurations are produced. The reasoning is
/// [`cargo`]'s, and the short version is that emitting them would change the
/// Run and Tests tabs for every Rust repository on the next scan.
///
/// The kind is decided here rather than in the parser because half of the
/// answer is on disk. Cargo infers a binary from `src/main.rs` and a library
/// from `src/lib.rs` with no manifest section at all, which is how most crates
/// are written — `cb-core` declares no `[lib]` and is a library. A crate that
/// is both is reported as an executable, because the executable is the half a
/// user can act on.
fn scan_cargo_project(root: &Path, path: &Path) -> Option<(Project, Vec<RunConfig>)> {
    let dir = path.parent()?.to_path_buf();
    let id = project_id(root, &dir);

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return Some(unreadable_project(
                id,
                dir_name(&dir, "crate"),
                path.to_path_buf(),
                dir,
                "cargo",
                e.to_string(),
            ))
        }
    };

    let Some(manifest) = cargo::parse(&content) else {
        // `cargo::parse` returns `None` for exactly one reason — the TOML would
        // not parse — so the message from a second parse describes the same
        // failure, and it names a line and column the user can go to.
        let reason = toml::from_str::<toml::Table>(&content)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Cargo.toml could not be read".to_string());

        return Some(unreadable_project(
            id,
            dir_name(&dir, "crate"),
            path.to_path_buf(),
            dir,
            "cargo",
            reason,
        ));
    };

    // A virtual manifest describes where the members are without being a crate
    // itself — this repository's own root manifest is one. Treating it as a
    // project puts an empty box at the repository root, the same phantom
    // `is_workspace_root` already keeps out of the Node scan.
    if manifest.is_virtual_manifest() {
        return None;
    }

    let kind = if manifest.has_bin || dir.join("src").join("main.rs").is_file() {
        ProjectKind::Executable
    } else if manifest.has_lib || dir.join("src").join("lib.rs").is_file() {
        ProjectKind::Library
    } else {
        ProjectKind::Unknown
    };

    // `package_name` is also `None` for `name.workspace = true`, which resolves
    // against a file the parser was not given. The directory is then the
    // honest answer rather than a guess at the inherited name.
    let name = manifest
        .package_name
        .unwrap_or_else(|| dir_name(&dir, "crate"));

    Some((
        Project {
            id,
            name,
            manifest_path: path.to_path_buf(),
            dir,
            ecosystem: "cargo".into(),
            kind,
            frameworks: Vec::new(),
            configurations: Vec::new(),
            is_test_project: false,
            test_runner: None,
            unreadable: None,
        },
        Vec::new(),
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
            unreadable: None,
        },
        configs,
    ))
}

fn scan_node_project(root: &Path, path: &Path) -> Option<(Project, Vec<RunConfig>)> {
    let dir = path.parent()?.to_path_buf();
    let id = project_id(root, &dir);

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return Some(unreadable_project(
                id,
                dir_name(&dir, "package"),
                path.to_path_buf(),
                dir,
                "node",
                e.to_string(),
            ))
        }
    };

    let Some(parsed) = node::parse_package_json(&content) else {
        // Re-parsing as a bare value is what turns "it did not deserialise"
        // into a sentence with a line and column in it. The fallback covers
        // JSON that is valid but not an object — `parse_package_json` refuses
        // that too, and serde_json has no complaint to offer about it.
        let reason = serde_json::from_str::<serde_json::Value>(&content)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "package.json is not a JSON object".to_string());

        return Some(unreadable_project(
            id,
            dir_name(&dir, "package"),
            path.to_path_buf(),
            dir,
            "node",
            reason,
        ));
    };

    // A monorepo root describes where the packages are rather than being a
    // project itself. Its own scripts are still worth offering when it has
    // any, so only a script-less root is skipped outright.
    if node::is_workspace_root(&parsed) && parsed.scripts.is_empty() {
        return None;
    }

    let kind = node::project_kind(&parsed);
    let test_runner = node::detect_runner(&parsed);

    let name = parsed
        .name
        .clone()
        .unwrap_or_else(|| dir_name(&dir, "package"));
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
            unreadable: None,
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
    use crate::model::{ConfigSource, RunKind};

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

    /// The symbol index and the project scan must see exactly the same files,
    /// so the walker they share is tested directly rather than only through
    /// `scan`.
    #[test]
    fn the_source_walker_skips_build_output_and_nested_checkouts() {
        let dir = workspace_with(&[
            ("src/a.rs", "fn a() {}"),
            ("node_modules/x.js", "export const x = 1;"),
            ("vendored/.git/HEAD", "ref: refs/heads/main\n"),
            ("vendored/b.rs", "fn b() {}"),
        ]);

        let seen: Vec<String> = source_walker(dir.path())
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                relative(dir.path(), e.path())
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert!(seen.contains(&"src/a.rs".to_string()), "saw {seen:?}");
        assert!(
            !seen.contains(&"node_modules/x.js".to_string()),
            "saw {seen:?}"
        );
        assert!(!seen.contains(&"vendored/b.rs".to_string()), "saw {seen:?}");
    }

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
    fn a_project_nested_deeper_than_ten_levels_is_discovered() {
        // Monorepos and generated trees bury real projects well past a
        // conventional `src/Area/Project` depth. A package twelve directories
        // down (past the old limit of 10) must still be found.
        let dir = workspace_with(&[(
            "d01/d02/d03/d04/d05/d06/d07/d08/d09/d10/d11/d12/package.json",
            r#"{"name":"deep","scripts":{"dev":"vite"}}"#,
        )]);
        let ws = scan(dir.path()).unwrap();

        assert!(
            ws.projects.iter().any(|p| p.name == "deep"),
            "deeply nested project must be discovered: {:?}",
            ws.projects.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn mtp_detection_reads_a_solution_level_dotnet_config() {
        let dir = workspace_with(&[
            (
                "dotnet.config",
                "[dotnet.test:runner]\nname = \"Microsoft.Testing.Platform\"\n",
            ),
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
        assert_eq!(
            tests.test_runner,
            Some(TestRunner::MicrosoftTestingPlatform)
        );
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

        assert!(ws
            .configs
            .iter()
            .any(|c| c.launch_profile.as_deref() == Some("https")));
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
        assert!(ws
            .configs
            .iter()
            .any(|c| c.script.as_deref() == Some("dev")));
    }

    #[test]
    fn a_mixed_workspace_yields_both_ecosystems() {
        let dir = workspace_with(&[
            ("api/Api.csproj", EXE_CSPROJ),
            (
                "web/package.json",
                r#"{"name":"web","scripts":{"dev":"vite"}}"#,
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.projects.iter().any(|p| p.ecosystem == "dotnet"));
        assert!(ws.projects.iter().any(|p| p.ecosystem == "node"));
    }

    #[test]
    fn a_monorepo_root_without_scripts_is_not_a_project() {
        let dir = workspace_with(&[
            (
                "package.json",
                r#"{"private":true,"workspaces":["apps/*"]}"#,
            ),
            (
                "apps/web/package.json",
                r#"{"name":"web","scripts":{"dev":"vite"}}"#,
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].name, "web");
    }

    // -- cargo ---------------------------------------------------------------

    #[test]
    fn cargo_crates_are_detected_and_classified_from_manifest_and_layout() {
        // `has_bin`/`has_lib` only report what the manifest *declares*; the
        // conventional `src/main.rs` and `src/lib.rs` are facts about the
        // directory, which is why the scan — not the parser — decides the kind.
        let dir = workspace_with(&[
            ("cli/Cargo.toml", "[package]\nname = \"cli\"\n"),
            ("cli/src/main.rs", "fn main() {}"),
            ("lib/Cargo.toml", "[package]\nname = \"the-lib\"\n"),
            ("lib/src/lib.rs", "pub fn f() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        let cli = ws.projects.iter().find(|p| p.name == "cli").unwrap();
        assert_eq!(cli.ecosystem, "cargo");
        assert_eq!(cli.kind, ProjectKind::Executable);

        let lib = ws.projects.iter().find(|p| p.name == "the-lib").unwrap();
        assert_eq!(lib.ecosystem, "cargo");
        assert_eq!(lib.kind, ProjectKind::Library);
        assert_eq!(
            lib.name, "the-lib",
            "the crate name, not the directory, names the project"
        );
    }

    #[test]
    fn a_declared_bin_makes_a_crate_executable_without_a_main_rs_on_disk() {
        let dir = workspace_with(&[
            (
                "app/Cargo.toml",
                "[package]\nname = \"app\"\n\n[[bin]]\nname = \"app\"\npath = \"other.rs\"\n",
            ),
            ("app/other.rs", "fn main() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects[0].kind, ProjectKind::Executable);
    }

    /// A crate that is both a library and a binary is offered as the thing that
    /// can be launched, because that is the only one of the two a user can act
    /// on. `src-tauri` in this repository is exactly this shape.
    #[test]
    fn a_crate_with_both_a_lib_and_a_bin_is_executable() {
        let dir = workspace_with(&[
            ("both/Cargo.toml", "[package]\nname = \"both\"\n"),
            ("both/src/lib.rs", "pub fn f() {}"),
            ("both/src/main.rs", "fn main() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects[0].kind, ProjectKind::Executable);
    }

    #[test]
    fn a_virtual_cargo_manifest_is_a_workspace_root_rather_than_a_project() {
        // This repository's own root manifest is exactly this shape; treating
        // it as a project puts a phantom, empty box at the repository root.
        let dir = workspace_with(&[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/core\"]\nresolver = \"2\"\n",
            ),
            ("crates/core/Cargo.toml", "[package]\nname = \"core\"\n"),
            ("crates/core/src/lib.rs", "pub fn f() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        let names: Vec<&str> = ws.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["core"]);
    }

    /// A root crate that is itself a package is both a workspace root and a
    /// real crate, so it stays.
    #[test]
    fn a_root_crate_that_is_also_a_workspace_root_is_still_a_project() {
        let dir = workspace_with(&[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"sub\"]\n\n[package]\nname = \"root-crate\"\n",
            ),
            ("src/lib.rs", "pub fn f() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert!(ws.projects.iter().any(|p| p.name == "root-crate"));
    }

    /// Detection only. Emitting `cargo run`/`cargo test` configurations would
    /// change the Run and Tests tabs for every Rust repository on the next
    /// scan — a product decision nobody has made; see `adapters::cargo`.
    #[test]
    fn a_cargo_crate_contributes_no_run_or_test_configurations() {
        let dir = workspace_with(&[
            ("cli/Cargo.toml", "[package]\nname = \"cli\"\n"),
            ("cli/src/main.rs", "fn main() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert!(
            ws.configs.is_empty(),
            "cargo detection must add no configurations: {:?}",
            ws.configs.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        assert!(!ws.projects[0].is_test_project);
        assert_eq!(ws.projects[0].test_runner, None);
    }

    #[test]
    fn cargo_build_output_is_not_scanned_for_crates() {
        // `target/` holds vendored and generated manifests; SKIP_DIRS already
        // excludes it, which this pins rather than assumes.
        let dir = workspace_with(&[
            ("app/Cargo.toml", "[package]\nname = \"app\"\n"),
            ("app/src/main.rs", "fn main() {}"),
            (
                "app/target/debug/build/dep/Cargo.toml",
                "[package]\nname = \"generated\"\n",
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        let names: Vec<&str> = ws.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["app"]);
    }

    #[test]
    fn a_cargo_config_resolves_back_to_its_project_by_directory() {
        let dir = workspace_with(&[
            ("app/Cargo.toml", "[package]\nname = \"app\"\n"),
            ("app/src/main.rs", "fn main() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        let project = &ws.projects[0];
        let by_path = RunConfig {
            project: Some(relative(&ws.root, &project.dir)),
            ..RunConfig::new("x", "X", RunKind::App, "cargo", ConfigSource::UserFile)
        };
        assert_eq!(
            find_project(&ws, &by_path).map(|p| &p.id),
            Some(&project.id)
        );
    }

    const CARGO_NEXTEST_ADAPTER: &str = r#"
id = "cargo-nextest"
name = "cargo nextest"
detect = ["Cargo.toml"]

[test]
program = "cargo"
args = ["nextest", "run"]
report_format = "junitXml"
"#;

    /// `examples/adapters/cargo-nextest.toml` is the documented way to get Rust
    /// runs today, and it detects `Cargo.toml` — the very file the built-in
    /// adapter now claims. Since the built-in one emits no configurations,
    /// letting it shadow the manifest would silently delete the user's only
    /// Rust run and test configurations on the next scan.
    #[test]
    fn a_declarative_adapter_still_supplies_configurations_for_a_cargo_crate() {
        let dir = workspace_with(&[
            (
                ".code-basics/adapters/cargo-nextest.toml",
                CARGO_NEXTEST_ADAPTER,
            ),
            ("app/Cargo.toml", "[package]\nname = \"app\"\n"),
            ("app/src/main.rs", "fn main() {}"),
        ]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1, "still one project, not two");
        assert_eq!(
            ws.projects[0].ecosystem, "cargo",
            "the built-in adapter keeps the project, so the architecture graph still sees a crate"
        );
        assert!(
            ws.configs.iter().any(|c| c.kind == RunKind::Test),
            "the manifest's test configuration must survive: {:?}",
            ws.configs.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        for config in &ws.configs {
            assert!(
                find_project(&ws, config).is_some(),
                "config {} has no project",
                config.id
            );
        }
    }

    // -- manifests that will not parse ---------------------------------------
    //
    // Every one of these builds the broken tree *before* scanning. An earlier
    // round corrupted a file after scanning a healthy tree, so the assertions
    // passed while the case a user actually hits — opening a workspace that is
    // already broken — went unexercised.

    /// The reported reason, for a project the scan could not fully read.
    fn unreadable_reason<'a>(ws: &'a Workspace, name: &str) -> &'a str {
        let project = ws
            .projects
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "{name} must still be listed; got {:?}",
                    ws.projects.iter().map(|p| &p.name).collect::<Vec<_>>()
                )
            });
        project
            .unreadable
            .as_deref()
            .unwrap_or_else(|| panic!("{name} must carry a reason"))
    }

    #[test]
    fn a_package_json_that_will_not_parse_is_listed_with_its_reason() {
        let dir = workspace_with(&[("web/package.json", "{ \"name\": \"web\", oops }")]);
        let ws = scan(dir.path()).unwrap();

        let reason = unreadable_reason(&ws, "web");
        assert!(!reason.is_empty(), "the reason must say something");

        let project = &ws.projects[0];
        assert_eq!(project.ecosystem, "node");
        assert_eq!(project.kind, ProjectKind::Unknown);
        assert!(project.manifest_path.ends_with("package.json"));
        assert!(ws.configs.is_empty(), "an unreadable project cannot be run");
    }

    #[test]
    fn a_cargo_toml_that_will_not_parse_is_listed_with_its_reason() {
        let dir = workspace_with(&[("app/Cargo.toml", "[package\nname = \"app\"\n")]);
        let ws = scan(dir.path()).unwrap();

        assert!(!unreadable_reason(&ws, "app").is_empty());
        assert_eq!(ws.projects[0].ecosystem, "cargo");
        assert_eq!(ws.projects[0].kind, ProjectKind::Unknown);
        assert!(ws.configs.is_empty());
    }

    /// Assert the one guarantee `unreadable_project` makes, over a `.csproj`
    /// whose text is broken in some particular way: the project is still
    /// listed, it carries a reason, it claims nothing about itself, and — the
    /// point — it contributes no run configurations.
    ///
    /// quick-xml reports only one of the six malformed shapes below as an
    /// error; the other five reached `Event::Eof` cleanly, so the single test
    /// that existed here passed over a guarantee that held in one case out of
    /// six. Each shape therefore gets its own test rather than a loop, so a
    /// regression names the shape that regressed.
    fn assert_csproj_is_unreadable(contents: &str) {
        let dir = workspace_with(&[("src/App/App.csproj", contents)]);
        let ws = scan(dir.path()).unwrap();

        assert!(
            !unreadable_reason(&ws, "App").is_empty(),
            "the reason must say something"
        );
        assert_eq!(ws.projects[0].ecosystem, "dotnet");
        assert_eq!(
            ws.projects[0].kind,
            ProjectKind::Unknown,
            "a document that did not parse cannot have told us its kind"
        );
        assert!(
            ws.configs.is_empty(),
            "a project read from broken XML must not offer configurations \
             built out of half of it; got {:?}",
            ws.configs.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_csproj_with_a_mismatched_end_tag_is_listed_with_its_reason() {
        assert_csproj_is_unreadable("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup></Project>");
    }

    #[test]
    fn a_csproj_truncated_after_an_opening_property_group_is_listed_with_its_reason() {
        assert_csproj_is_unreadable("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup>");
    }

    /// The commonest way a user meets this state: a half-saved file. Enough of
    /// it parsed to look like an executable targeting net8.0, which is exactly
    /// enough to build `App (Debug)` and `App (Release)` out of a guess.
    #[test]
    fn a_csproj_truncated_mid_property_is_listed_rather_than_yielding_run_configurations() {
        assert_csproj_is_unreadable(
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    \
             <OutputType>Exe</OutputType>\n    <TargetFramework>net8.0",
        );
    }

    #[test]
    fn a_csproj_that_is_only_an_unclosed_root_element_is_listed_with_its_reason() {
        assert_csproj_is_unreadable("<Project>");
    }

    #[test]
    fn a_csproj_with_an_unquoted_attribute_value_is_listed_with_its_reason() {
        assert_csproj_is_unreadable("<Project Sdk=Microsoft.NET.Sdk></Project>");
    }

    /// Not XML at all. Whatever this file is, it is not the project we would be
    /// building a command line for.
    #[test]
    fn a_csproj_holding_binary_junk_is_listed_with_its_reason() {
        assert_csproj_is_unreadable("\u{1}\u{2}not xml at all <<< >>>");
    }

    /// Well-formed XML that is not an MSBuild project. Silently treating it as
    /// an empty library would be the same wrong answer wearing a valid hat.
    #[test]
    fn a_csproj_whose_root_element_is_not_project_is_listed_with_its_reason() {
        assert_csproj_is_unreadable("<Solution><Item Name=\"App\" /></Solution>");
    }

    /// The other half of the bargain: real `.csproj` files carry comments,
    /// processing instructions, CDATA, namespaces and prefixes, and none of
    /// those may cost a project its configurations.
    #[test]
    fn a_csproj_with_comments_declarations_cdata_and_namespaces_stays_healthy() {
        let dir = workspace_with(&[(
            "src/App/App.csproj",
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!-- a comment, and a <not-a-tag> inside it -->\n\
             <Project Sdk=\"Microsoft.NET.Sdk\" \
             xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\n  \
             <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    \
             <TargetFramework>net8.0</TargetFramework>\n    \
             <PostBuild><![CDATA[echo <hi> && echo done]]></PostBuild>\n  \
             </PropertyGroup>\n</Project>\n",
        )]);
        let ws = scan(dir.path()).unwrap();

        assert_eq!(ws.projects[0].unreadable, None, "this file is well-formed");
        assert_eq!(ws.projects[0].kind, ProjectKind::Executable);
        // One runnable entry for the one runnable project. What this test is
        // really guarding is that the parse survived the comment, the CDATA and
        // the namespace at all — a project it could not read produces *no*
        // configurations, so a count of zero here is the failure worth catching.
        assert_eq!(
            ws.configs.len(),
            1,
            "one entry for one project; got {:?}",
            ws.configs.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
        assert_eq!(
            ws.projects[0].configurations,
            vec!["Debug", "Release"],
            "the default pair is still what the toolbar's picker offers"
        );
    }

    /// The broken project costs the scan that project and nothing else.
    #[test]
    fn one_broken_manifest_does_not_cost_the_workspace_its_healthy_projects() {
        let dir = workspace_with(&[
            ("web/package.json", "{ nope"),
            (
                "api/package.json",
                r#"{"name":"api","scripts":{"dev":"vite"}}"#,
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        let api = ws.projects.iter().find(|p| p.name == "api").unwrap();
        assert_eq!(api.unreadable, None);
        assert!(ws
            .configs
            .iter()
            .any(|c| c.script.as_deref() == Some("dev")));
    }

    /// `skip_serializing_if` means a healthy project has no `unreadable` key at
    /// all, which is what `src/ipc/types.ts` mirrors as an optional field.
    #[test]
    fn a_healthy_project_serialises_without_an_unreadable_key() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let ws = scan(dir.path()).unwrap();

        let json = serde_json::to_value(&ws.projects[0]).unwrap();
        assert!(
            !json.as_object().unwrap().contains_key("unreadable"),
            "got {json}"
        );
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
        assert!(ws
            .configs
            .iter()
            .any(|c| c.ecosystem == "pytest" && c.kind == RunKind::Test));
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
            assert!(
                find_project(&ws, config).is_some(),
                "config {} has no project",
                config.id
            );
        }
    }

    #[test]
    fn a_built_in_ecosystem_keeps_a_directory_a_manifest_also_matches() {
        // Manifests extend the built-in adapters; they do not shadow them.
        // `pyproject.toml` beside a package.json must not turn a Node project
        // into a pytest one.
        let dir = workspace_with(&[
            (".code-basics/adapters/pytest.toml", PYTEST_ADAPTER),
            (
                "app/package.json",
                r#"{"name":"app","scripts":{"dev":"vite"}}"#,
            ),
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

        assert!(
            ws.projects.is_empty(),
            "a manifest is required to claim a directory"
        );
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

        assert!(ws
            .configs
            .iter()
            .any(|c| c.framework.as_deref() == Some("net8.0")));
        assert!(ws
            .configs
            .iter()
            .any(|c| c.framework.as_deref() == Some("net9.0")));
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

        assert_eq!(
            ws.projects[0].configurations,
            vec!["Debug", "Release", "Staging"]
        );
        // `Project.configurations` is where the Run toolbar's build-configuration
        // picker reads its options from. It is the *only* place a declared
        // configuration surfaces now: the scan produces one run configuration
        // per project, defaulted to Debug, and the toolbar overrides it for a
        // launch. Fanning Staging out into a `App (Staging)` entry of its own is
        // exactly what was removed.
        assert_eq!(
            ws.configs.iter().filter(|c| c.kind == RunKind::App).count(),
            1
        );
        assert!(ws
            .configs
            .iter()
            .all(|c| c.build_configuration.as_deref() != Some("Staging")));
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
        assert!(
            ws.solutions.is_empty(),
            "a solution with no projects is not worth reporting"
        );
    }

    #[test]
    fn project_ids_are_relative_so_they_survive_a_move() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let ws = scan(dir.path()).unwrap();

        assert!(
            !ws.projects[0]
                .id
                .contains(&dir.path().display().to_string()),
            "id must not embed an absolute path"
        );
    }

    #[test]
    fn configurations_resolve_back_to_their_project() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            (
                "web/package.json",
                r#"{"name":"web","scripts":{"dev":"vite"}}"#,
            ),
        ]);
        let ws = scan(dir.path()).unwrap();

        for config in &ws.configs {
            assert!(
                find_project(&ws, config).is_some(),
                "config {} has no project",
                config.id
            );
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
            (
                "web/package.json",
                r#"{"name":"web","scripts":{"dev":"vite"}}"#,
            ),
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
            [
                "configs",
                "favorites",
                "name",
                "order",
                "projects",
                "root",
                "solutions"
            ]
        );
    }

    #[test]
    fn a_missing_root_is_an_error() {
        assert!(scan(Path::new("/nonexistent/workspace")).is_err());
    }

    // -- grouping configurations for the sidebar -----------------------------

    #[test]
    fn configurations_are_grouped_under_the_project_they_belong_to() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("src/App.Tests/App.Tests.csproj", XUNIT_CSPROJ),
        ]);
        let ws = scan(dir.path()).unwrap();

        let grouped = configs_by_project(&ws);

        assert!(grouped.contains_key("App"), "got: {:?}", grouped.keys());
        assert!(grouped.contains_key("App.Tests"));
        // Every configuration lands in exactly one group.
        assert_eq!(
            grouped.values().map(Vec::len).sum::<usize>(),
            ws.configs.len()
        );
    }

    /// A configuration naming no project still has to appear somewhere, or it
    /// would vanish from the sidebar rather than showing up unfiled.
    #[test]
    fn a_configuration_with_no_project_is_grouped_under_the_workspace() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let mut ws = scan(dir.path()).unwrap();
        ws.configs.push(RunConfig::new(
            "loose",
            "Loose",
            RunKind::App,
            "dotnet",
            ConfigSource::UserFile,
        ));

        let grouped = configs_by_project(&ws);

        assert_eq!(
            grouped.get("Workspace").map(|c| c.len()),
            Some(1),
            "got: {:?}",
            grouped.keys()
        );
        assert_eq!(grouped["Workspace"][0].id, "loose");
    }

    /// A configuration pointing at a project that is no longer there must not
    /// be silently dropped from the list the user is looking at.
    #[test]
    fn a_configuration_pointing_at_a_missing_project_is_still_listed() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let mut ws = scan(dir.path()).unwrap();
        let mut orphan = RunConfig::new(
            "gone",
            "Gone",
            RunKind::App,
            "dotnet",
            ConfigSource::UserFile,
        );
        orphan.project = Some(PathBuf::from("src/Deleted/Deleted.csproj"));
        ws.configs.push(orphan);

        let grouped = configs_by_project(&ws);

        assert!(grouped["Workspace"].iter().any(|c| c.id == "gone"));
    }

    #[test]
    fn a_workspace_with_no_configurations_groups_into_nothing() {
        let dir = workspace_with(&[("notes.md", "nothing to run")]);
        let ws = scan(dir.path()).unwrap();

        assert!(configs_by_project(&ws).is_empty());
    }

    // -- scan options --------------------------------------------------------

    /// The default is filesystem-only, and `scan` is defined as `scan_with` at
    /// its defaults — so the two must agree exactly.
    #[test]
    fn scanning_with_default_options_matches_the_plain_scan() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("src/App.Tests/App.Tests.csproj", XUNIT_CSPROJ),
        ]);

        assert_eq!(
            scan(dir.path()).unwrap(),
            scan_with(dir.path(), ScanOptions::default()).unwrap()
        );
        assert!(!ScanOptions::default().msbuild_evaluation);
    }

    /// MSBuild evaluation only *refines* what the XML already said. With no
    /// `dotnet` on the machine — or a project it cannot evaluate — the scan
    /// must still return the projects rather than failing or emptying out.
    #[test]
    fn asking_for_msbuild_evaluation_never_costs_the_scan_its_projects() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);

        let refined = scan_with(
            dir.path(),
            ScanOptions {
                msbuild_evaluation: true,
            },
        )
        .unwrap();

        assert_eq!(refined.projects.len(), 1);
        assert_eq!(refined.projects[0].name, "App");
        assert_eq!(refined.root, scan(dir.path()).unwrap().root);
    }

    #[test]
    fn a_missing_root_is_an_error_whichever_options_are_used() {
        assert!(scan_with(
            Path::new("/nonexistent/workspace"),
            ScanOptions {
                msbuild_evaluation: true,
            },
        )
        .is_err());
    }

    // -- launch profiles -----------------------------------------------------

    const LAUNCH_SETTINGS: &str = r#"{
      "profiles": {
        "http":  { "commandName": "Project", "applicationUrl": "http://localhost:5000" },
        "https": { "commandName": "Project", "applicationUrl": "https://localhost:5001" }
      }
    }"#;

    #[test]
    fn launch_profiles_are_read_from_beside_the_project() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("src/App/Properties/launchSettings.json", LAUNCH_SETTINGS),
        ]);

        let profiles = launch_profiles(&dir.path().join("src/App/App.csproj"));

        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["http", "https"]);
    }

    /// No launch settings is the ordinary case — `dotnet run`'s default
    /// profile applies — so it must be an empty list, never an error.
    #[test]
    fn a_project_with_no_launch_settings_has_no_profiles() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);

        assert!(launch_profiles(&dir.path().join("src/App/App.csproj")).is_empty());
    }

    #[test]
    fn unreadable_launch_settings_yield_no_profiles_rather_than_failing() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            ("src/App/Properties/launchSettings.json", "{ not json"),
        ]);

        assert!(launch_profiles(&dir.path().join("src/App/App.csproj")).is_empty());
    }

    /// Profiles live beside the project file, so a path with no parent has
    /// nowhere to look.
    #[test]
    fn a_project_path_with_no_parent_has_no_profiles() {
        assert!(launch_profiles(Path::new("App.csproj")).is_empty());
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
        assert_eq!(
            names,
            vec!["A", "M", "Z"],
            "scan order must not depend on the filesystem"
        );
    }

    // -----------------------------------------------------------------------
    // Opening a directory
    // -----------------------------------------------------------------------

    #[test]
    fn opening_a_directory_scans_it() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let ws = workspace_from_dir(dir.path()).unwrap();

        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].name, "App");
    }

    #[test]
    fn opening_a_path_that_does_not_exist_is_refused() {
        let dir = workspace_with(&[]);
        let missing = dir.path().join("nowhere");

        let err = workspace_from_dir(&missing).unwrap_err();
        assert_eq!(err, format!("{} is not a directory", missing.display()));
    }

    #[test]
    fn opening_a_file_rather_than_a_directory_is_refused() {
        let dir = workspace_with(&[("src/App/App.csproj", EXE_CSPROJ)]);
        let file = dir.path().join("src/App/App.csproj");

        let err = workspace_from_dir(&file).unwrap_err();
        assert_eq!(err, format!("{} is not a directory", file.display()));
    }

    #[test]
    fn opening_a_directory_applies_its_saved_configuration() {
        let dir = workspace_with(&[
            ("src/App/App.csproj", EXE_CSPROJ),
            (
                ".code-basics/config.json",
                r#"{"version":1,"configs":[{"id":"custom","name":"Custom","kind":"app",
                    "ecosystem":"dotnet","source":"userFile"}],"favorites":["custom"]}"#,
            ),
        ]);

        let ws = workspace_from_dir(dir.path()).unwrap();

        assert_eq!(ws.favorites, vec!["custom".to_string()]);
        assert!(
            ws.configs.iter().any(|c| c.id == "custom"),
            "the saved configuration should be merged in: {:?}",
            ws.configs.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        // Favourites sort first, which only `config::apply` arranges.
        assert_eq!(ws.configs[0].id, "custom");
    }
}
