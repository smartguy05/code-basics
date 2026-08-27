//! Behavioral before/after testing — running the same configuration against
//! git HEAD and the working tree and diffing the observable outcomes.
//!
//! The command body orchestrates only: it opens a baseline worktree, runs the
//! configured tests on both sides, captures each run's console, replays any
//! `.http` scenarios, and hands the raw signals to
//! [`cb_core::behavioral::assemble_report`] — the pure seam that decides how
//! deltas become the wire [`BehavioralReport`]. Every decision worth testing
//! lives in `cb-core`; nothing here does.
//!
//! # Abstain, never 500
//!
//! A baseline that will not check out, a config missing on the HEAD side, a
//! server that never comes up — none of these are failures of the *tool*, and
//! none should sink the whole report. Each becomes a warning on an otherwise
//! honest report, mirroring the whole intent stack's rule.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cb_core::behavioral::{
    self, BaselineWorktree, BehavioralReport, ConsoleNormalization, HttpDelta, HttpRequestSpec,
    LaunchChoice, Readiness, SideResult, WorktreeOptions,
};
use cb_core::git::attribution::{self, Options};
use cb_core::git::grouping;
use cb_core::git::{ComparisonMode, Repo};
use cb_core::intents::{self, LoadOptions};
use cb_core::model::{RunKind, TestRunResult};
use cb_core::process::{ProcessEvent, Stream};
use cb_core::{invocation, testing};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::state::AppState;

/// An empty report carrying only a reason it could not run — the abstain shape.
fn abstained(warning: String) -> BehavioralReport {
    behavioral::assemble_report(None, None, Vec::new(), &[], vec![warning])
}

/// Forward supervisor events onto the IPC channel *and* capture stdout text
/// into a shared buffer, so the console diff has something to compare.
///
/// The returned `Arc<Mutex<String>>` holds everything written to stdout once
/// the run finishes; the spawned task ends when the supervisor closes its
/// sender.
fn tee(
    mut events: mpsc::Receiver<ProcessEvent>,
    channel: Channel<ProcessEvent>,
) -> (Arc<Mutex<String>>, tokio::task::JoinHandle<()>) {
    let captured = Arc::new(Mutex::new(String::new()));
    let sink = captured.clone();
    let handle = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if let ProcessEvent::Output {
                stream: Stream::Stdout,
                text,
            } = &event
            {
                if let Ok(mut buf) = sink.lock() {
                    buf.push_str(text);
                }
            }
            // A closed channel means the window went away; keep draining so the
            // capture completes, but stop forwarding.
            let _ = channel.send(event);
        }
    });
    (captured, handle)
}

/// Read a captured buffer out of its `Arc<Mutex<..>>` once the tee task is done.
fn drain_capture(captured: Arc<Mutex<String>>) -> String {
    captured.lock().map(|b| b.clone()).unwrap_or_default()
}

/// Run one side of the comparison as a test run, returning its parsed result
/// and captured stdout. `id` disambiguates the two sides on the supervisor
/// (`Supervisor::run` replaces a repeated id).
async fn run_side(
    state: &State<'_, AppState>,
    workspace: &cb_core::workspace::Workspace,
    config: &cb_core::model::RunConfig,
    id: &str,
    channel: &Channel<ProcessEvent>,
) -> Result<(TestRunResult, String), String> {
    let invocation = invocation::build(workspace, config, None)?;
    let report = invocation
        .report
        .clone()
        .ok_or_else(|| format!("{} is not a test configuration", config.name))?;

    // A stale report would be parsed as this run's result if the runner failed
    // to produce a fresh one.
    let _ = std::fs::remove_file(&report.path);

    let (tx, rx) = mpsc::channel(512);
    let (captured, tee_handle) = tee(rx, channel.clone());

    state
        .supervisor
        .run(id, &invocation, tx)
        .await
        .map_err(|e| format!("{e:#}"))?;

    // The supervisor has dropped its sender; wait for the tee to flush.
    let _ = tee_handle.await;

    let result = testing::parse_file(report.format, &report.path).map_err(|e| format!("{e:#}"))?;
    Ok((result, drain_capture(captured)))
}

/// Pump supervisor events onto the IPC channel, so a launched server's logs
/// stream to the console during HTTP replay. Nothing is captured — unlike
/// [`tee`], the HTTP side diffs responses, not console text.
fn forward(mut events: mpsc::Receiver<ProcessEvent>, channel: Channel<ProcessEvent>) {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if channel.send(event).is_err() {
                break;
            }
        }
    });
}

/// Bring one side's server up, poll readiness, replay every request, then tear
/// it down — the untestable I/O half of HTTP replay. Every *decision* it feeds
/// (readiness gating, pairing, diffing) lives in `cb_core::behavioral::scenario`.
///
/// A server never exits, so [`Supervisor::run`] is spawned as a detached task
/// and **never awaited before replaying** — awaiting it would hang forever. The
/// blocking reqwest calls run on `spawn_blocking` so they never touch the async
/// executor. The server is cancelled and its task awaited before returning, so
/// the caller can safely launch the next side on the same port.
async fn run_http_side(
    state: &State<'_, AppState>,
    workspace: &cb_core::workspace::Workspace,
    config: &cb_core::model::RunConfig,
    id: String,
    channel: &Channel<ProcessEvent>,
    readiness: &Readiness,
    requests: &[(String, HttpRequestSpec)],
) -> SideResult {
    // A build failure abstains this side rather than sinking the command.
    let invocation = match invocation::build(workspace, config, None) {
        Ok(inv) => inv,
        Err(e) => return SideResult::unready(format!("could not build launch command: {e}")),
    };

    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel.clone());

    // Detach the server: run() only returns when the process exits, and a
    // server does not exit until we cancel it.
    let run_handle = {
        let sup = state.supervisor.clone();
        let run_id = id.clone();
        // Tracked so a behavioral server that is left running (or orphaned by a
        // crash mid-replay) shows in the Running panel and can be killed there.
        let meta = cb_core::running::RunMeta {
            root: invocation.cwd.display().to_string(),
            label: format!("Behavioral: {}", config.name),
            kind: cb_core::running::RunKind::Behavioral,
        };
        tokio::spawn(async move { sup.run_tracked(&run_id, &invocation, tx, meta).await })
    };

    // reqwest::blocking must not run on the async executor.
    let ready = {
        let r = readiness.clone();
        tokio::task::spawn_blocking(move || cb_core::behavioral::await_ready(&r))
            .await
            .unwrap_or_else(|e| Err(format!("readiness task panicked: {e}")))
    };

    let mut responses = BTreeMap::new();
    if ready.is_ok() {
        for (key, req) in requests {
            let req = req.clone();
            let resp = tokio::task::spawn_blocking(move || cb_core::behavioral::send(&req))
                .await
                .unwrap_or_else(|e| Err(format!("send task panicked: {e}")));
            responses.insert(key.clone(), resp);
        }
    }

    // Always tear the server down, even if it never became ready, and wait for
    // the port to free before the next side launches.
    //
    // A cancel issued before the detached run() has registered the process (it
    // registers only *after* spawning the child) returns false and kills
    // nothing — reachable when readiness fails fast, e.g. a tiny
    // `@readiness timeout=`. An unconditional `run_handle.await` on a server
    // that was never killed then blocks forever, hanging the whole command and
    // leaking the process. So retry the cancel until it takes or the task ends,
    // then await under a timeout so a stuck spawn can never hang us.
    for _ in 0..100 {
        if state.supervisor.cancel(&id).await || run_handle.is_finished() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), run_handle).await;

    SideResult { ready, responses }
}

/// Compute the intent groups for the current working tree, exactly like
/// `commands::intents::intent_groups` does.
fn working_tree_groups(root: &std::path::Path, repo: &Repo) -> Vec<grouping::IntentGroup> {
    let Ok(diffs) = repo.diff_all(ComparisonMode::WorkingToHead) else {
        return Vec::new();
    };
    let branch = repo.status().ok().and_then(|s| s.branch);
    let Ok(intents) = intents::load(root, &LoadOptions { branch }) else {
        return Vec::new();
    };
    let attributions = attribution::attribute(&diffs, &intents, Options::default());
    grouping::group(&diffs, &attributions, &intents)
}

/// Drive the whole serverful HTTP replay, returning the computed deltas and
/// pushing every abstain into `warnings`.
///
/// The orchestration (spawn/poll/send/cancel) is here because it is untestable;
/// every *decision* is delegated to the pure, tested seams in
/// `cb_core::behavioral::scenario` — `plan_replay`, `choose_launch_config`,
/// `pair_and_diff`. The base side is launched, replayed, cancelled **and
/// awaited** before the work side starts, so the two never hold the same port.
#[allow(clippy::too_many_arguments)]
async fn run_http_replay(
    state: &State<'_, AppState>,
    workspace: &cb_core::workspace::Workspace,
    base_ws: &cb_core::workspace::Workspace,
    work_config: &cb_core::model::RunConfig,
    config_id: &str,
    root: &std::path::Path,
    http_files: &Option<Vec<String>>,
    channel: &Channel<ProcessEvent>,
    warnings: &mut Vec<String>,
) -> Vec<HttpDelta> {
    // a. Decide the scenarios: explicit files if any were passed, else discover.
    let files: Vec<std::path::PathBuf> = match http_files {
        Some(fs) if !fs.is_empty() => fs.iter().map(|f| root.join(f)).collect(),
        _ => behavioral::discover_http_files(root),
    };
    // Nothing requested and nothing found — HTTP was simply not in scope.
    if files.is_empty() {
        return Vec::new();
    }

    let mut scenarios = Vec::new();
    for path in &files {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut scenario = behavioral::parse_http_file(&text);
                scenario.path = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                warnings.extend(scenario.warnings.clone());
                scenarios.push(scenario);
            }
            Err(e) => warnings.push(format!("could not read .http file {}: {e}", path.display())),
        }
    }

    let plan = behavioral::plan_replay(&scenarios);

    // A plan we cannot safely gate on, or with nothing to send, abstains.
    let Some(readiness) = plan.readiness else {
        warnings.push(
            "HTTP replay skipped: no @readiness declared in any .http file; cannot safely determine \
             when the server is up to replay"
                .to_string(),
        );
        return Vec::new();
    };
    if plan.requests.is_empty() {
        return Vec::new();
    }

    // b. Decide the launch config.
    let launch_id = match behavioral::choose_launch_config(work_config, &workspace.configs) {
        LaunchChoice::Use(id) => id,
        LaunchChoice::Abstain(reason) => {
            warnings.push(reason);
            return Vec::new();
        }
    };
    let Some(work_launch) = behavioral::find_config(workspace, &launch_id).cloned() else {
        warnings.push(format!(
            "launch configuration {launch_id} vanished from the working tree; HTTP replay skipped"
        ));
        return Vec::new();
    };
    // The baseline may lack the config (newly added); that side abstains.
    let base_launch = behavioral::find_config(base_ws, &launch_id).cloned();

    // c. Run BASE side first, fully torn down, then WORK — same port, in turn.
    let base_side = match &base_launch {
        Some(base_launch) => {
            run_http_side(
                state,
                base_ws,
                base_launch,
                format!("{config_id}:http-base"),
                channel,
                &readiness,
                &plan.requests,
            )
            .await
        }
        None => SideResult::unready(format!(
            "launch configuration {launch_id} does not exist at HEAD, so the baseline server \
             cannot be started"
        )),
    };

    let work_side = run_http_side(
        state,
        workspace,
        &work_launch,
        format!("{config_id}:http-work"),
        channel,
        &readiness,
        &plan.requests,
    )
    .await;

    // d. Pair and diff. `diff_http` already ignores VOLATILE_HEADERS, so no
    // extra ignore list is needed here.
    let (deltas, http_warnings) =
        behavioral::pair_and_diff(&plan.keys, &base_side, &work_side, &[]);
    warnings.extend(http_warnings);
    deltas
}

#[tauri::command]
pub async fn behavioral_diff(
    state: State<'_, AppState>,
    config_id: String,
    http_files: Option<Vec<String>>,
    channel: Channel<ProcessEvent>,
) -> Result<BehavioralReport, String> {
    let workspace = state.workspace()?;
    let root = workspace.root.clone();

    // Groups are derived from the working tree so the deltas can be attributed
    // to the cards the user is looking at. Opening the repo also gives us HEAD.
    let repo = Repo::open(&root).map_err(|e| format!("{e:#}"))?;
    let head_oid = repo.head_oid().map_err(|e| format!("{e:#}"))?;
    let groups = working_tree_groups(&root, &repo);

    // A baseline that will not materialise is an abstain, not a 500.
    let mut wt = match BaselineWorktree::create(&root, &head_oid, &WorktreeOptions::default()) {
        Ok(wt) => wt,
        Err(e) => {
            return Ok(abstained(format!(
                "could not check out the baseline at {head_oid}: {e:#} — nothing to compare against"
            )));
        }
    };

    let mut warnings: Vec<String> = Vec::new();

    // The working-tree config must exist; the baseline one may not (the config
    // could be newly added), in which case the test/console sides are skipped.
    let work_config = match behavioral::find_config(&workspace, &config_id) {
        Some(c) => c.clone(),
        None => {
            warnings.extend(wt.finish());
            return Ok(abstained(format!(
                "no configuration named {config_id} in the working tree"
            )));
        }
    };

    let base_ws = match behavioral::scan_baseline(wt.path()) {
        Ok(ws) => ws,
        Err(e) => {
            warnings.extend(wt.finish());
            return Ok(abstained(format!(
                "could not scan the baseline checkout: {e} — nothing to compare against"
            )));
        }
    };
    let base_config = behavioral::find_config(&base_ws, &config_id).cloned();

    // ---- Tests + console, both sides ------------------------------------
    let mut tests = None;
    let mut console = None;

    if let Some(base_config) = &base_config {
        if matches!(work_config.kind, RunKind::Test) {
            let base_run = run_side(
                &state,
                &base_ws,
                base_config,
                &format!("{config_id}:base"),
                &channel,
            )
            .await;
            let work_run = run_side(
                &state,
                &workspace,
                &work_config,
                &format!("{config_id}:work"),
                &channel,
            )
            .await;

            match (base_run, work_run) {
                (Ok((base_result, base_out)), Ok((work_result, work_out))) => {
                    tests = Some(behavioral::diff_tests(&base_result, &work_result));
                    let norm = ConsoleNormalization {
                        roots: vec![
                            wt.path().to_string_lossy().to_string(),
                            root.to_string_lossy().to_string(),
                        ],
                        ..Default::default()
                    };
                    console = Some(behavioral::diff_console(&base_out, &work_out, &norm));
                }
                (base_run, work_run) => {
                    if let Err(e) = base_run {
                        warnings.push(format!("baseline test run failed: {e}"));
                    }
                    if let Err(e) = work_run {
                        warnings.push(format!("working-tree test run failed: {e}"));
                    }
                }
            }
        } else {
            warnings.push(format!(
                "{} is not a test configuration, so no test/console comparison was run",
                work_config.name
            ));
        }
    } else {
        warnings.push(format!(
            "configuration {config_id} does not exist at HEAD, so there is no baseline test run to \
             compare against"
        ));
    }

    // ---- HTTP replay -----------------------------------------------------
    //
    // Bring the launch config up on each side in turn — strictly sequential, so
    // the two servers never contend for the same port — poll readiness, replay
    // every request, tear the server down, then diff the two sides' responses.
    // Every threshold abstains into `warnings`: no @readiness, no/ambiguous
    // launch config, a side that never came up, a request that errored.
    let http = run_http_replay(
        &state,
        &workspace,
        &base_ws,
        &work_config,
        &config_id,
        &root,
        &http_files,
        &channel,
        &mut warnings,
    )
    .await;

    // Keep the checkout for the next run at this HEAD, then drain any teardown
    // residue (there is none when kept, but finish() is where warnings live).
    wt.keep_for_reuse();
    warnings.extend(wt.finish());

    Ok(behavioral::assemble_report(
        tests, console, http, &groups, warnings,
    ))
}

/// Clear every cached baseline checkout for this workspace.
#[tauri::command]
pub async fn behavioral_clear(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let root = state.workspace_root()?;
    Ok(cb_core::behavioral::worktree::clear_all(&root))
}
