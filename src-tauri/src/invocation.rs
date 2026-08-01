//! Turning a run configuration into a command line.
//!
//! The only place that knows which adapter owns which ecosystem. Everything it
//! calls lives in `cb-core` and is tested there; this is dispatch and path
//! resolution only.

use std::path::Path;

use cb_core::adapters::{dotnet, manifest, node};
use cb_core::config;
use cb_core::model::{Invocation, RunConfig, RunKind, TestRunner};
use cb_core::workspace::{self, Workspace};

/// Build the command for a configuration.
///
/// `filter` names the tests to restrict a run to, for "re-run failed".
pub fn build(
    workspace: &Workspace,
    config: &RunConfig,
    filter: Option<&[String]>,
) -> Result<Invocation, String> {
    let root = workspace.root.as_path();
    let results = config::results_dir(root);

    // Reports are written here by the runner, so the directory has to exist
    // before it runs rather than after.
    std::fs::create_dir_all(&results)
        .map_err(|e| format!("failed to create {}: {e}", results.display()))?;

    match config.ecosystem.as_str() {
        "dotnet" => Ok(build_dotnet(workspace, config, &results, filter)),
        "node" => build_node(workspace, config, &results, filter),
        // Anything else must come from a declarative manifest.
        other => build_from_manifest(workspace, config, &results, filter, other),
    }
}

fn build_dotnet(
    workspace: &Workspace,
    config: &RunConfig,
    results: &Path,
    filter: Option<&[String]>,
) -> Invocation {
    let root = workspace.root.as_path();

    // Read the project fresh rather than trusting the scan, so switching a
    // project between VSTest and MTP takes effect without reopening.
    let (runner, trx_available) = match &config.project {
        Some(project) => workspace::dotnet_test_context(root, &root.join(project)),
        None => (None, false),
    };

    let ctx = dotnet::BuildContext {
        workspace_root: root,
        results_dir: results,
        runner: runner.or(Some(TestRunner::VsTest)),
        trx_extension_available: trx_available,
        filter: filter.map(<[String]>::to_vec),
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
                root,
                &project_dir,
                manager,
                runner,
                results,
                filter,
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

    let found = manifests.iter().find(|m| m.id == ecosystem).ok_or_else(|| {
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
        RunKind::Test => found.test.as_ref().ok_or_else(|| {
            format!("the `{ecosystem}` adapter does not define a test command")
        })?,
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
