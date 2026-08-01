//! Running applications and tests.

use cb_core::model::{TestNode, TestRunResult};
use cb_core::process::ProcessEvent;
use cb_core::testing;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::invocation;
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
) -> Result<(), String> {
    let workspace = state.workspace()?;
    let config = workspace
        .configs
        .iter()
        .find(|c| c.id == config_id)
        .ok_or_else(|| format!("no configuration named {config_id}"))?;

    let invocation = invocation::build(&workspace, config, None)?;

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

#[tauri::command]
pub async fn cancel_run(state: State<'_, AppState>, config_id: String) -> Result<bool, String> {
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
    let filter = only_failed
        .then(|| state.previous_test_run(&config_id))
        .flatten()
        .map(|previous| testing::tree::failed_names(&previous.cases))
        .filter(|names| !names.is_empty());

    if only_failed && filter.is_none() {
        return Err(
            "there are no failed tests from a previous run of this configuration to re-run"
                .to_string(),
        );
    }

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

    state.record_test_run(&config_id, result.clone());

    Ok(TestRunOutcome {
        result,
        tree,
        warnings: invocation.warnings,
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
