//! Object-inspection commands.
//!
//! The bridge and nothing else. Every decision — which architecture to try,
//! whether a failure is worth retrying, what the workspace's limits are, what
//! the user should be warned about — lives in [`cb_core::inspect`]. The one
//! job that genuinely cannot: resolving the bundled sidecar directory, which
//! only the Tauri app handle knows.
//!
//! The sidecar runs through the same [`cb_core::process::Supervisor`] as a test
//! run, under the id `inspect:<session>`, so cancelling and process-tree kill
//! work without anything here knowing how either is done.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cb_core::inspect::sidecar::{self, ProcessList};
use cb_core::inspect::{
    session, AttachableList, ElidedReason, InspectGraph, InspectStatus, InspectTarget, RootSpec,
    RunDump,
};
use cb_core::model::Invocation;
use cb_core::process::{ProcessEvent, Stream};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;

use crate::state::AppState;

/// Where the bundled sidecar builds live, when they were bundled at all.
///
/// `None` is an ordinary answer: the sidecar is a separate .NET publish that
/// `cargo build` does not produce, so a fresh checkout legitimately has no
/// resource directory. `cb_core`'s resolver takes it from here, including the
/// `CB_INSPECTOR_PATH` development override.
fn bundled_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve("inspector", tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|dir| dir.is_dir())
}

/// Forward supervisor events onto an IPC channel, as `run.rs` does.
fn forward(mut events: mpsc::Receiver<ProcessEvent>, channel: Channel<ProcessEvent>) {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if channel.send(event).is_err() {
                break;
            }
        }
    });
}

fn note(channel: &Channel<ProcessEvent>, text: String) {
    let _ = channel.send(ProcessEvent::Output {
        stream: Stream::Stderr,
        text: format!("[code-basics] {text}\n"),
    });
}

/// Whether the inspector can serve this workspace, and what to warn about.
#[tauri::command]
pub async fn inspect_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<InspectStatus, String> {
    let root = state.workspace_root()?;
    Ok(session::status(&root, bundled_dir(&app).as_deref()))
}

/// Enumerate the machine's .NET processes.
///
/// The one thing this must never do is answer "there is nothing there" when
/// what happened is "I could not look". Those are different statements, and the
/// second one is used as a safety gate: `inspect_capture` refuses a live target
/// that is not on the list, telling the user their process has exited and its
/// pid may have been reused. An unwritable temp directory, a sidecar an
/// anti-virus has locked, or one rebuilt at a different schema version would all
/// have produced that sentence about a process the user can watch running in the
/// console.
///
/// So a failure is an `Err` carrying what actually went wrong, and every caller
/// decides what to do with it. A degraded-but-real answer — the enumerator ran
/// and could not read parent pids, say — comes back as warnings on the list
/// instead.
///
/// The listing runs through the supervisor under an `inspect:` id so that it is
/// cancellable and process-tree killed like anything else — and so that
/// [`cb_core::inspect::session::attribute`] can recognise and drop it, since the
/// enumerator is itself a .NET process and will find itself.
async fn enumerate(
    app: &AppHandle,
    state: &State<'_, AppState>,
    cwd: &Path,
) -> Result<ProcessList, String> {
    let bitness = session::first_bitness(bundled_dir(app).as_deref())
        .ok_or_else(session::missing_sidecar_reason)?;
    let program = sidecar::resolve(bundled_dir(app).as_deref(), bitness)
        .ok_or_else(session::missing_sidecar_reason)?;

    let session_id = session::new_session_id();
    let result_path = sidecar::process_list_path(&session_id);

    let invocation = Invocation {
        program: program.display().to_string(),
        args: sidecar::list_command_args(&result_path),
        cwd: cwd.to_path_buf(),
        env: BTreeMap::new(),
        report: None,
        warnings: Vec::new(),
    };

    // The enumeration says nothing worth showing in a console, but the
    // supervisor streams unconditionally, so the events are drained rather than
    // dropped mid-send.
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let ran = state
        .supervisor
        .run(&format!("inspect:list-{session_id}"), &invocation, tx)
        .await;

    // Ran-or-not and read-or-not are kept apart by `session::enumeration_outcome`,
    // which does not even open the file when the run failed: a listing left by
    // an earlier poll would otherwise answer this one as though it were fresh.
    let listed =
        session::enumeration_outcome(ran.map(|_| ()).map_err(|e| format!("{e:#}")), || {
            sidecar::parse_process_list_file(&result_path).map_err(|e| format!("{e:#}"))
        });

    // Read once and gone: it is a poll's answer, not state.
    let _ = std::fs::remove_file(&result_path);

    listed
}

/// The .NET processes that can be attached to, and how each was linked to a run
/// configuration.
///
/// An empty list is a normal answer, not an error — nothing is running. A list
/// that could not be read is an error, because the view says something different
/// about each and must not show the second as the first.
///
/// Anything that degraded the answer without preventing it rides along as
/// `warnings`: the enumerator being unable to read parent pids leaves every
/// process unattributable, which on its own looks exactly like a machine running
/// nothing of the user's.
///
/// The attribution itself is [`cb_core::inspect::session::attribute`]'s; this
/// supplies the three observations it reasons over: what is on the machine, what
/// the supervisor launched, and what the workspace's configurations are.
#[tauri::command]
pub async fn inspect_attachable(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AttachableList, String> {
    // No workspace means no cwd to run the enumerator in and no configurations
    // to attribute against — an empty answer rather than a failure, because
    // nothing is wrong: no repository is open.
    let (Ok(root), Ok(workspace)) = (state.workspace_root(), state.workspace()) else {
        return Ok(AttachableList::default());
    };
    let listed = enumerate(&app, &state, &root).await?;
    let running = state.supervisor.running().await;
    Ok(AttachableList {
        processes: session::attribute(&listed.processes, &running, &workspace.configs),
        warnings: listed.warnings,
    })
}

/// The dump a finished run may have written, and whether it is certainly that
/// run's.
///
/// The attribution rule is [`cb_core::inspect::session::dump_for_run`]'s, in the
/// core rather than in the view, because "which dump belongs to this run" is
/// exactly the kind of claim a UI must not invent: with two configurations up,
/// the newest dump since a run started belongs to whichever of them crashed
/// last. `pid` is what the run reported, when it reported one.
#[tauri::command]
pub async fn inspect_run_dump(
    state: State<'_, AppState>,
    pid: Option<u32>,
    started_at: u64,
) -> Result<Option<RunDump>, String> {
    let root = state.workspace_root()?;
    let dumps = cb_core::inspect::dumps::list(&cb_core::inspect::dumps::dumps_dir(&root));
    Ok(session::dump_for_run(&dumps, pid, started_at))
}

/// Take one capture, streaming the sidecar's output to the console.
///
/// There is deliberately no separate "expand" command: expanding past a cap is
/// this same call with [`RootSpec::Address`], plus `widen` naming the cap that
/// stopped the last read of that branch. Without `widen` the re-read runs under
/// the limits that truncated it and returns the identical truncation, so an
/// expand that could not expand would still report a fresh read.
#[tauri::command]
pub async fn inspect_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    target: InspectTarget,
    root: RootSpec,
    widen: Option<ElidedReason>,
    channel: Channel<ProcessEvent>,
) -> Result<InspectGraph, String> {
    let workspace_root = state.workspace_root()?;
    let bundled = bundled_dir(&app);

    // Checked before anything is spawned: a live capture clones the target's
    // process image, and paying that only to learn the request could never
    // have worked charges the user's application for it.
    if let Some(reason) = session::unsupported_reason(&target, &root) {
        return Err(reason);
    }

    // Re-checked here, not just when the picker was filled. The list the pid
    // came from is refreshed on demand, so between choosing a process and
    // pressing Capture it can have exited and the operating system can have
    // reused its number — and the replacement is very often another managed
    // process, which would attach happily and be rendered under the user's
    // configuration name.
    //
    // An enumeration that *failed* is refused with its own reason rather than
    // being read as an empty machine: "this pid is no longer running" is a
    // statement of fact about the user's process, and it must never stand in for
    // "the list could not be read". The capture could not have run either way —
    // it is the same sidecar — so nothing is lost by saying which happened.
    if let InspectTarget::Live { .. } = target {
        let workspace = state.workspace()?;
        let listed = enumerate(&app, &state, &workspace_root)
            .await
            .map_err(|e| {
                format!(
                    "The .NET processes on this machine could not be listed, so the process you \
                 picked could not be confirmed to still be running and nothing was read from it. \
                 Nothing is known about whether it is still there. {e}"
                )
            })?;
        let running = state.supervisor.running().await;
        let attachable = session::attribute(&listed.processes, &running, &workspace.configs);
        if let Some(reason) = session::live_target_reason(&target, &attachable) {
            return Err(reason);
        }
    }

    let mut bitness =
        session::first_bitness(bundled.as_deref()).ok_or_else(session::missing_sidecar_reason)?;

    let session_id = session::new_session_id();
    let request = session::request_for(&workspace_root, target, root, widen);
    let request_path = sidecar::write_request(&workspace_root, &session_id, &request)
        .map_err(|e| format!("{e:#}"))?;
    let result_path = sidecar::result_path(&workspace_root, &session_id);

    // A result left by an earlier attempt would be read as this one's answer if
    // the sidecar died before writing — the same guard `run_tests` puts on a
    // stale test report.
    let _ = std::fs::remove_file(&result_path);

    loop {
        let program = sidecar::resolve(bundled.as_deref(), bitness)
            .ok_or_else(session::missing_sidecar_reason)?;

        let invocation = Invocation {
            program: program.display().to_string(),
            args: sidecar::command_args(&request_path, &result_path),
            cwd: workspace_root.clone(),
            env: BTreeMap::new(),
            report: None,
            warnings: Vec::new(),
        };

        let (tx, rx) = mpsc::channel(512);
        forward(rx, channel.clone());

        state
            .supervisor
            .run(&format!("inspect:{session_id}"), &invocation, tx)
            .await
            .map_err(|e| format!("{e:#}"))?;

        // The loop and the spawning are this layer's; whether there is another
        // attempt worth making is `session::retry_bitness`'s, including the
        // "only when the other build is actually on disk" half of it — which is
        // why the resolve happens here, where the bundled directory is known.
        let outcome = session::attempt_outcome(&result_path);
        let other = session::other_bitness(bitness);
        let Some(next) = session::retry_bitness(
            &outcome,
            bitness,
            sidecar::resolve(bundled.as_deref(), other).is_some(),
        ) else {
            break;
        };

        note(
            &channel,
            format!(
                "the target is a different architecture; retrying with {}",
                sidecar::sidecar_file_name(next)
            ),
        );
        bitness = next;
    }

    let graph = cb_core::inspect::parse_result_file(&session_id, &result_path)
        .map_err(|e| format!("{e:#}"))?;

    for problem in session::prune(&workspace_root) {
        note(&channel, problem);
    }

    state.record_inspect(graph.clone());
    Ok(graph)
}

/// The last capture, so the tree survives a view switch without re-reading a
/// process that may since have moved on.
#[tauri::command]
pub async fn inspect_last(state: State<'_, AppState>) -> Result<Option<InspectGraph>, String> {
    Ok(state.previous_inspect())
}

/// Forget the held capture. A graph is a copy of somebody's data; "close it"
/// has to actually drop it.
#[tauri::command]
pub async fn inspect_clear(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_inspect();
    Ok(())
}
