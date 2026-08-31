//! Turning a run configuration into a command line.
//!
//! The only place that knows which adapter owns which ecosystem. Everything it
//! calls lives in the adapter modules and is tested there; this is dispatch and
//! path resolution only.

use std::collections::BTreeMap;
use std::path::Path;

use crate::adapters::{dotnet, manifest, node};
use crate::config;
use crate::inspect::session::{self, ArmedDumps};
use crate::model::{Invocation, RunConfig, RunKind, TestCase, TestRunner};
use crate::testing;
use crate::workspace::{self, Workspace};

/// Build the command for a configuration.
///
/// `filter` names the tests to restrict a run to, for "re-run failed".
pub fn build(
    workspace: &Workspace,
    config: &RunConfig,
    filter: Option<&[String]>,
) -> Result<Invocation, String> {
    build_with(workspace, config, filter, false)
}

/// Build the command for a **coverage** test run.
///
/// Identical to [`build`] except that the ecosystem adapters that support it
/// (dotnet, node) add their coverage flags and set [`Invocation::coverage`].
/// A separate entry point rather than a parameter on [`build`] so the existing
/// callers — application launches, compound members, the run tab — are
/// untouched and keep producing byte-identical commands. Adapters that do not
/// understand coverage (declarative manifests) simply ignore the request.
pub fn build_coverage(
    workspace: &Workspace,
    config: &RunConfig,
    filter: Option<&[String]>,
) -> Result<Invocation, String> {
    build_with(workspace, config, filter, true)
}

fn build_with(
    workspace: &Workspace,
    config: &RunConfig,
    filter: Option<&[String]>,
    coverage: bool,
) -> Result<Invocation, String> {
    let root = workspace.root.as_path();
    let results = config::results_dir(root);

    // Reports are written here by the runner, so the directory has to exist
    // before it runs rather than after.
    std::fs::create_dir_all(&results)
        .map_err(|e| format!("failed to create {}: {e}", results.display()))?;

    match config.ecosystem.as_str() {
        // Compounds have no command of their own; commands/run.rs launches
        // their members individually and must never get here.
        "compound" => Err(format!(
            "{} is a compound configuration with no command of its own",
            config.name
        )),
        "dotnet" => {
            let dumps = session::arm_dumps(root);
            Ok(build_dotnet(
                workspace,
                config,
                &results,
                filter,
                dumps.as_ref(),
                coverage,
            ))
        }
        "node" => build_node(workspace, config, &results, filter, coverage),
        // Anything else must come from a declarative manifest.
        other => build_from_manifest(workspace, config, &results, filter, other),
    }
}

/// The test-name filter a run should use, given whether the user asked to
/// re-run only the failures and what the previous run of that same
/// configuration produced.
///
/// Asking to re-run failures when there are none is an error rather than a
/// silent full run: running the whole suite would look like the request was
/// ignored.
pub fn rerun_filter(
    only_failed: bool,
    previous: Option<&[TestCase]>,
) -> Result<Option<Vec<String>>, String> {
    if !only_failed {
        return Ok(None);
    }

    let names = previous
        .map(testing::tree::failed_names)
        .filter(|names| !names.is_empty())
        .ok_or_else(|| {
            "there are no failed tests from a previous run of this configuration to re-run"
                .to_string()
        })?;

    Ok(Some(names))
}

/// Resolve every member of a compound configuration into the command that will
/// run it.
///
/// Everything is resolved before anything is started, so a broken member stops
/// the whole launch rather than leaving half of it running — and every problem
/// is reported at once, because fixing them one error at a time is a poor way
/// to learn that three members are missing.
///
/// `env` is the environment for this run only (the UI's environment picker),
/// layered over each member's own.
pub fn plan_compound(
    workspace: &Workspace,
    config: &RunConfig,
    env: Option<&BTreeMap<String, String>>,
) -> Result<Vec<(RunConfig, Invocation)>, String> {
    let mut members = Vec::new();
    let mut errors = Vec::new();

    for member_id in &config.compound {
        let Some(member) = workspace.configs.iter().find(|c| c.id == *member_id) else {
            errors.push(format!("compound member `{member_id}` no longer exists"));
            continue;
        };
        if !member.compound.is_empty() {
            errors.push(format!(
                "`{}` is itself a compound configuration; nesting is not supported",
                member.name
            ));
            continue;
        }

        let mut member = member.clone();
        // The run's environment wins over the configuration's own.
        member.env.extend(
            env.into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), v.clone())),
        );

        match build(workspace, &member, None) {
            Ok(invocation) => members.push((member, invocation)),
            Err(e) => errors.push(e),
        }
    }

    if errors.is_empty() {
        Ok(members)
    } else {
        Err(errors.join("; "))
    }
}

/// One project of a solution build: a label and the command that builds it.
pub struct SolutionBuildStep {
    pub name: String,
    pub invocation: Invocation,
}

/// Plan a "build solution" as a per-project sequence.
///
/// The user builds **each** project rather than handing the `.sln` to MSBuild,
/// so this resolves the solution's members onto the projects the scan actually
/// found — a member that resolves to nothing, or to a non-.NET project, is
/// reported in the warnings and never invented — and emits one
/// `dotnet build`/`clean` per resolved project. Resolution mirrors
/// `architecture::graph::solution_containment`: a member's root-relative path,
/// joined onto the workspace root, is looked up against each project's manifest
/// path. The steps are returned in solution order for the caller to run in
/// sequence.
pub fn plan_solution_build(
    workspace: &Workspace,
    solution: &crate::adapters::solution::Solution,
    action: dotnet::BuildAction,
) -> (Vec<SolutionBuildStep>, Vec<String>) {
    let by_manifest: BTreeMap<&std::path::PathBuf, &crate::model::Project> = workspace
        .projects
        .iter()
        .map(|p| (&p.manifest_path, p))
        .collect();

    let mut steps = Vec::new();
    let mut warnings = Vec::new();

    for member in &solution.projects {
        let absolute = workspace.root.join(&member.path);
        let Some(project) = by_manifest.get(&absolute) else {
            warnings.push(format!(
                "{}: solution member '{}' matches no project the scan found, so it was not built",
                solution.name,
                member.path.display()
            ));
            continue;
        };
        if project.ecosystem != "dotnet" {
            warnings.push(format!(
                "{}: solution member '{}' is not a .NET project ({}), so it was not built",
                solution.name,
                member.path.display(),
                project.ecosystem
            ));
            continue;
        }

        // A minimal configuration is all `build_action_invocation` reads: it
        // takes only the project path (plus optional configuration/framework,
        // left unset so MSBuild's defaults apply).
        let mut config = RunConfig::new(
            format!("solution-build:{}", member.name),
            member.name.clone(),
            RunKind::App,
            "dotnet",
            crate::model::ConfigSource::Detected,
        );
        config.project = Some(member.path.clone());

        steps.push(SolutionBuildStep {
            name: member.name.clone(),
            invocation: dotnet::build_action_invocation(&config, action, &workspace.root),
        });
    }

    (steps, warnings)
}

fn build_dotnet(
    workspace: &Workspace,
    config: &RunConfig,
    results: &Path,
    filter: Option<&[String]>,
    dumps: Option<&ArmedDumps>,
    coverage: bool,
) -> Invocation {
    let root = workspace.root.as_path();

    // Read the project fresh rather than trusting the scan, so switching a
    // project between VSTest and MTP takes effect without reopening.
    let (runner, trx_available) = match &config.project {
        Some(project) => workspace::dotnet_test_context(root, &root.join(project)),
        None => (None, false),
    };

    let has_launch_settings = config
        .project
        .as_ref()
        .and_then(|p| {
            root.join(p)
                .parent()
                .map(|d| d.join("Properties").join("launchSettings.json"))
        })
        .is_some_and(|p| p.exists());

    let ctx = dotnet::BuildContext {
        workspace_root: root,
        results_dir: results,
        runner: runner.or(Some(TestRunner::VsTest)),
        trx_extension_available: trx_available,
        has_launch_settings,
        filter: filter.map(<[String]>::to_vec),
        coverage,
        dumps_dir: dumps.map(|d| d.dir.as_path()),
        dump_env: dumps.map(|d| &d.env),
    };

    match config.kind {
        RunKind::Test => dotnet::test_invocation(config, &ctx),
        RunKind::App => dotnet::run_invocation(config, &ctx),
    }
}

fn build_node(
    workspace: &Workspace,
    config: &RunConfig,
    results: &Path,
    filter: Option<&[String]>,
    coverage: bool,
) -> Result<Invocation, String> {
    let root = workspace.root.as_path();
    let project_dir = node::project_dir(root, config.project.as_ref());
    let manager = node::detect_package_manager(&project_dir);

    match config.kind {
        RunKind::Test => {
            let runner = workspace::find_project(workspace, config)
                .and_then(|p| p.test_runner)
                .ok_or_else(|| {
                    format!(
                        "no test runner was detected for {}. Add vitest or jest to its \
                         package.json, then rescan.",
                        config.name
                    )
                })?;

            Ok(node::test_invocation(
                config,
                node::TestInvocationContext {
                    workspace_root: root,
                    project_dir: &project_dir,
                    manager,
                    runner,
                    results_dir: results,
                },
                filter,
                coverage,
            ))
        }
        RunKind::App => Ok(node::script_invocation(config, root, &project_dir, manager)),
    }
}

fn build_from_manifest(
    workspace: &Workspace,
    config: &RunConfig,
    results: &Path,
    filter: Option<&[String]>,
    ecosystem: &str,
) -> Result<Invocation, String> {
    let root = workspace.root.as_path();
    let (manifests, errors) = manifest::load_dir(&manifest::manifest_dir(root));

    let found = manifests
        .iter()
        .find(|m| m.id == ecosystem)
        .ok_or_else(|| {
            if errors.is_empty() {
                format!("no adapter named `{ecosystem}` is defined in .code-basics/adapters")
            } else {
                // A manifest that failed to load is the likeliest reason its
                // adapter appears to be missing, so say so rather than just
                // reporting it as unknown.
                format!(
                    "no adapter named `{ecosystem}` could be loaded. \
                 One or more manifests failed to parse: {}",
                    errors.join("; ")
                )
            }
        })?;

    let template = match config.kind {
        RunKind::Test => found
            .test
            .as_ref()
            .ok_or_else(|| format!("the `{ecosystem}` adapter does not define a test command"))?,
        RunKind::App => {
            let name = config.script.as_deref().unwrap_or_default();
            found.run.get(name).ok_or_else(|| {
                format!("the `{ecosystem}` adapter does not define a run command named `{name}`")
            })?
        }
    };

    let project_dir = match &config.project {
        Some(project) => root.join(project),
        None => root.to_path_buf(),
    };

    Ok(manifest::build_invocation(
        template,
        config,
        root,
        &project_dir,
        results,
        filter,
    ))
}
