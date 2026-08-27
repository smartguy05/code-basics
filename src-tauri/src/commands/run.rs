//! Running applications and tests.

use cb_core::git::{ComparisonMode, Repo};
use cb_core::invocation;
use cb_core::model::{CoverageSpec, TestNode, TestRunResult};
use cb_core::process::ProcessEvent;
use cb_core::testing;
use cb_core::testing::changecov::{self, ChangeCoverage};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::state::AppState;

/// Forward supervisor events onto an IPC channel until the process ends.
///
/// Runs as its own task so output reaches the console as it is produced rather
/// than in one burst at the end.
pub(crate) fn forward(mut events: mpsc::Receiver<ProcessEvent>, channel: Channel<ProcessEvent>) {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // A closed channel means the window went away; nothing to do but
            // stop forwarding. The process itself is left to finish.
            if channel.send(event).is_err() {
                break;
            }
        }
    });
}

#[tauri::command]
pub async fn start_run(
    state: State<'_, AppState>,
    config_id: String,
    channel: Channel<ProcessEvent>,
    // Environment variables layered over the configuration's own for this
    // run only — the UI's environment picker (e.g. ASPNETCORE_ENVIRONMENT).
    env: Option<std::collections::BTreeMap<String, String>>,
) -> Result<(), String> {
    let slot = state.active_slot()?;
    let workspace = slot.workspace();
    let config = workspace
        .configs
        .iter()
        .find(|c| c.id == config_id)
        .ok_or_else(|| format!("no configuration named {config_id}"))?;

    if !config.compound.is_empty() {
        return start_compound(&slot, &workspace, config, &env, channel).await;
    }

    // Merged into the config *before* building, so invocation-time checks
    // (like the missing-launch-profile warning) see the effective environment.
    let mut config = config.clone();
    config.env.extend(env.into_iter().flatten());

    let invocation = invocation::build(&workspace, &config, None)?;

    // Surface anything questionable about the command before it runs.
    for warning in &invocation.warnings {
        let _ = channel.send(ProcessEvent::Output {
            stream: cb_core::process::Stream::Stderr,
            text: format!("[code-basics] {warning}\n"),
        });
    }

    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel);

    slot.supervisor
        .run_tracked(
            &config_id,
            &invocation,
            tx,
            cb_core::running::RunMeta {
                root: slot.root.display().to_string(),
                label: config.name.clone(),
                kind: cb_core::running::RunKind::Run,
            },
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

/// Launch every member of a compound configuration and wait for all of them.
///
/// Members run under their own config ids, so `cancel_run` on either the
/// compound or an individual member works. Output from all members is
/// interleaved onto the one channel, the way a shared console would show it.
async fn start_compound(
    slot: &std::sync::Arc<crate::state::WorkspaceSlot>,
    workspace: &cb_core::workspace::Workspace,
    config: &cb_core::model::RunConfig,
    env: &Option<std::collections::BTreeMap<String, String>>,
    channel: Channel<ProcessEvent>,
) -> Result<(), String> {
    // Resolve and build everything before starting anything, so a broken
    // member stops the whole launch rather than leaving half of it running.
    let members = invocation::plan_compound(workspace, config, env.as_ref())?;

    let mut handles = Vec::new();
    for (member, invocation) in members {
        let _ = channel.send(ProcessEvent::Output {
            stream: cb_core::process::Stream::Stderr,
            text: format!("[code-basics] starting {}\n", member.name),
        });
        for warning in &invocation.warnings {
            let _ = channel.send(ProcessEvent::Output {
                stream: cb_core::process::Stream::Stderr,
                text: format!("[code-basics] {}: {warning}\n", member.name),
            });
        }

        let (tx, rx) = mpsc::channel(512);
        forward(rx, channel.clone());

        let supervisor = slot.supervisor.clone();
        let meta = cb_core::running::RunMeta {
            root: slot.root.display().to_string(),
            label: member.name.clone(),
            kind: cb_core::running::RunKind::Run,
        };
        let member_id = member.id.clone();
        let member_name = member.name.clone();
        handles.push(tokio::spawn(async move {
            supervisor
                .run_tracked(&member_id, &invocation, tx, meta)
                .await
                .map(|_| ())
                .map_err(|e| format!("{member_name}: {e:#}"))
        }));
    }

    let mut errors = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(format!("{e}")),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Run a build-system action (build / rebuild / clean) for a configuration's
/// project, streaming compiler output to the console.
///
/// Registered with the supervisor under `<config_id>:build`, so a hung build
/// can be cancelled without touching a running app under the same config.
#[tauri::command]
pub async fn build_project(
    state: State<'_, AppState>,
    config_id: String,
    action: cb_core::adapters::dotnet::BuildAction,
    channel: Channel<ProcessEvent>,
) -> Result<(), String> {
    let slot = state.active_slot()?;
    let workspace = slot.workspace();
    let config = workspace
        .configs
        .iter()
        .find(|c| c.id == config_id)
        .ok_or_else(|| format!("no configuration named {config_id}"))?;

    if config.ecosystem != "dotnet" {
        return Err(format!(
            "build actions are only available for .NET configurations; {} is `{}`",
            config.name, config.ecosystem
        ));
    }

    let invocation =
        cb_core::adapters::dotnet::build_action_invocation(config, action, &workspace.root);

    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel);

    slot.supervisor
        .run_tracked(
            &format!("{config_id}:build"),
            &invocation,
            tx,
            cb_core::running::RunMeta {
                root: slot.root.display().to_string(),
                label: format!("{} (build)", config.name),
                kind: cb_core::running::RunKind::Build,
            },
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

/// The workspace slot a process-control command targets: the one named by `root`
/// when given (so a background tab's processes can be stopped or listed), else
/// the active one. `None` when that workspace is not open.
fn control_slot(
    state: &State<'_, AppState>,
    root: Option<String>,
) -> Option<std::sync::Arc<crate::state::WorkspaceSlot>> {
    match root {
        Some(root) => {
            let root =
                dunce::canonicalize(&root).unwrap_or_else(|_| std::path::PathBuf::from(&root));
            state.slot(&root)
        }
        None => state.active_slot().ok(),
    }
}

#[tauri::command]
pub async fn cancel_run(
    state: State<'_, AppState>,
    config_id: String,
    // The workspace whose run to cancel; defaults to the active one.
    root: Option<String>,
) -> Result<bool, String> {
    let Some(slot) = control_slot(&state, root) else {
        return Ok(false);
    };
    // Stopping a compound means stopping its members: the members are what is
    // actually registered with the supervisor.
    if let Some(config) = slot.workspace().configs.iter().find(|c| c.id == config_id) {
        if !config.compound.is_empty() {
            let mut any = false;
            for member in &config.compound {
                any |= slot.supervisor.cancel(member).await;
            }
            return Ok(any);
        }
    }
    Ok(slot.supervisor.cancel(&config_id).await)
}

#[tauri::command]
pub async fn running_ids(
    state: State<'_, AppState>,
    // The workspace whose running ids to list; defaults to the active one.
    root: Option<String>,
) -> Result<Vec<String>, String> {
    match control_slot(&state, root) {
        Some(slot) => Ok(slot.supervisor.running_ids().await),
        None => Ok(Vec::new()),
    }
}

/// The outcome of a test run, ready for the tree view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunOutcome {
    pub result: TestRunResult,
    pub tree: Vec<TestNode>,
    /// Problems worth showing that did not stop the run.
    pub warnings: Vec<String>,
    pub exit_code: Option<i32>,
}

#[tauri::command]
pub async fn run_tests(
    state: State<'_, AppState>,
    config_id: String,
    only_failed: bool,
    // Collect code coverage and map it onto the current diff. Off by default so
    // an ordinary test run's command line is unchanged.
    with_coverage: bool,
    channel: Channel<ProcessEvent>,
) -> Result<TestRunOutcome, String> {
    let slot = state.active_slot()?;
    let workspace = slot.workspace();
    let config = workspace
        .configs
        .iter()
        .find(|c| c.id == config_id)
        .ok_or_else(|| format!("no configuration named {config_id}"))?
        .clone();

    // "Re-run failed" needs the names from the previous run of this same
    // configuration.
    let previous = state.previous_test_run(&config_id);
    let filter = invocation::rerun_filter(
        only_failed,
        previous.as_ref().map(|previous| previous.cases.as_slice()),
    )?;

    let invocation = if with_coverage {
        invocation::build_coverage(&workspace, &config, filter.as_deref())?
    } else {
        invocation::build(&workspace, &config, filter.as_deref())?
    };
    let report = invocation
        .report
        .clone()
        .ok_or_else(|| format!("{} is not a test configuration", config.name))?;

    for warning in &invocation.warnings {
        let _ = channel.send(ProcessEvent::Output {
            stream: cb_core::process::Stream::Stderr,
            text: format!("[code-basics] {warning}\n"),
        });
    }

    // A stale report from a previous run would be parsed as this run's result
    // if the runner failed to produce a new one.
    let _ = std::fs::remove_file(&report.path);

    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel);

    let exit_code = slot
        .supervisor
        .run(&config_id, &invocation, tx)
        .await
        .map_err(|e| format!("{e:#}"))?;

    let result = testing::parse_file(report.format, &report.path).map_err(|e| format!("{e:#}"))?;
    let tree = testing::tree::build(&result.cases);

    let mut warnings = invocation.warnings;
    if !state.record_test_run(&workspace.root, &config_id, result.clone()) {
        warnings.push(
            "another workspace was opened while these tests were running, so this result was \
             not kept: \"re-run failed\" has nothing to filter by and will run the whole suite."
                .to_string(),
        );
    }

    // Coverage of change. A missing or unreadable coverage report is turned into
    // a warning, exactly like a missing TRX — it must never fail the test run,
    // which produced perfectly good results.
    if let Some(spec) = &invocation.coverage {
        match collect_change_coverage(&workspace.root, spec) {
            Ok(coverage) => {
                state.record_coverage(&workspace.root, &config_id, coverage);
            }
            Err(e) => warnings.push(format!(
                "tests ran, but code coverage could not be read, so coverage-of-change is \
                 unavailable: {e}"
            )),
        }
    }

    Ok(TestRunOutcome {
        result,
        tree,
        warnings,
        exit_code,
    })
}

/// Load the coverage report `spec` names and map it onto the working-tree diff.
///
/// Glue over the tested core: [`Repo::diff_all`], the report parser
/// ([`testing::coverage::load_report`]), and the mapper
/// ([`changecov::map_change_coverage`], which owns every abstain decision).
fn collect_change_coverage(
    root: &std::path::Path,
    spec: &CoverageSpec,
) -> Result<ChangeCoverage, String> {
    let coverage = testing::coverage::load_report(spec).map_err(|e| format!("{e:#}"))?;
    let repo = Repo::open(root).map_err(|e| format!("{e:#}"))?;
    let diffs = repo
        .diff_all(ComparisonMode::WorkingToHead)
        .map_err(|e| format!("{e:#}"))?;
    Ok(changecov::map_change_coverage(&diffs, &coverage))
}

/// The coverage-of-change map, resolving an absent cache to an empty result
/// carrying a warning rather than an error — nothing has been collected yet, and
/// that is a state the UI should render, not a failure.
fn resolve_coverage(cached: Option<ChangeCoverage>) -> ChangeCoverage {
    cached.unwrap_or_else(|| ChangeCoverage {
        warnings: vec![
            "no coverage has been collected yet — run a test configuration with coverage \
             enabled to see which changed lines are untested."
                .to_string(),
        ],
        ..Default::default()
    })
}

/// The last coverage-of-change map for the active workspace.
///
/// The map is computed when a coverage-enabled test run finishes (against the
/// working tree at that moment). `mode` is accepted for symmetry with the other
/// diff-reading commands; the cached map reflects the working tree as it stood
/// when coverage was collected.
#[tauri::command]
pub async fn coverage_of_change(
    state: State<'_, AppState>,
    mode: ComparisonMode,
) -> Result<ChangeCoverage, String> {
    let _ = mode;
    Ok(resolve_coverage(state.previous_coverage(None)))
}

/// The last recorded result for a configuration, so the tree survives a view
/// switch without re-running anything.
#[tauri::command]
pub async fn last_test_run(
    state: State<'_, AppState>,
    config_id: String,
) -> Result<Option<TestRunOutcome>, String> {
    Ok(state.previous_test_run(&config_id).map(|result| {
        let tree = testing::tree::build(&result.cases);
        TestRunOutcome {
            result,
            tree,
            warnings: Vec::new(),
            exit_code: None,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_coverage_returns_the_cached_map_unchanged() {
        let cached = ChangeCoverage {
            changed_lines: 4,
            covered_lines: 3,
            uncovered_lines: 1,
            ..Default::default()
        };
        let resolved = resolve_coverage(Some(cached.clone()));
        assert_eq!(resolved, cached);
        assert!(resolved.warnings.is_empty());
    }

    #[test]
    fn resolve_coverage_abstains_to_an_empty_map_with_a_warning() {
        // Nothing collected yet: an empty result the UI can render, not an error.
        let resolved = resolve_coverage(None);
        assert_eq!(resolved.changed_lines, 0);
        assert!(resolved.files.is_empty());
        assert_eq!(resolved.warnings.len(), 1);
        assert!(resolved.warnings[0].contains("no coverage has been collected"));
    }
}
