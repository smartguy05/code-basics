//! Running applications and tests.

use cb_core::invocation;
use cb_core::model::{TestNode, TestRunResult};
use cb_core::process::ProcessEvent;
use cb_core::testing;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::state::AppState;

/// Forward supervisor events onto an IPC channel until the process ends.
///
/// Runs as its own task so output reaches the console as it is produced rather
/// than in one burst at the end.
fn forward(mut events: mpsc::Receiver<ProcessEvent>, channel: Channel<ProcessEvent>) {
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
    let workspace = state.workspace()?;
    let config = workspace
        .configs
        .iter()
        .find(|c| c.id == config_id)
        .ok_or_else(|| format!("no configuration named {config_id}"))?;

    if !config.compound.is_empty() {
        return start_compound(&state, &workspace, config, &env, channel).await;
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

    state
        .supervisor
        .run(&config_id, &invocation, tx)
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
    state: &State<'_, AppState>,
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

        let supervisor = state.supervisor.clone();
        handles.push(tokio::spawn(async move {
            supervisor
                .run(&member.id, &invocation, tx)
                .await
                .map(|_| ())
                .map_err(|e| format!("{}: {e:#}", member.name))
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
    let workspace = state.workspace()?;
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

    state
        .supervisor
        .run(&format!("{config_id}:build"), &invocation, tx)
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn cancel_run(state: State<'_, AppState>, config_id: String) -> Result<bool, String> {
    // Stopping a compound means stopping its members: the members are what is
    // actually registered with the supervisor.
    if let Ok(workspace) = state.workspace() {
        if let Some(config) = workspace.configs.iter().find(|c| c.id == config_id) {
            if !config.compound.is_empty() {
                let mut any = false;
                for member in &config.compound {
                    any |= state.supervisor.cancel(member).await;
                }
                return Ok(any);
            }
        }
    }
    Ok(state.supervisor.cancel(&config_id).await)
}

#[tauri::command]
pub async fn running_ids(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.supervisor.running_ids().await)
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
    channel: Channel<ProcessEvent>,
) -> Result<TestRunOutcome, String> {
    let workspace = state.workspace()?;
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

    let invocation = invocation::build(&workspace, &config, filter.as_deref())?;
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

    let exit_code = state
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

    Ok(TestRunOutcome {
        result,
        tree,
        warnings,
        exit_code,
    })
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
