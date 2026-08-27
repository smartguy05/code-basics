//! The Running panel's bridge: list every process the app has running (across
//! all open codebases) plus the crash-orphan candidates, and kill one.
//!
//! The list is read straight from the shared `RunningStore`, which the
//! supervisors and PTY manager keep current. A kill is *routed* by kind so a
//! live process stops through its own handle (a run reports as a cancel, a
//! terminal closes cleanly); only an orphan — which has no live handle — is
//! killed by raw pid, and only after re-checking its identity so a reused pid is
//! never mistaken for the original.

use std::path::PathBuf;

use cb_core::running::{RunKind, RunningReport};
use tauri::State;

use crate::state::AppState;

/// Everything running now, plus orphan candidates from a previous session.
#[tauri::command]
pub async fn list_running(state: State<'_, AppState>) -> Result<RunningReport, String> {
    Ok(RunningReport {
        live: state.running.live(),
        orphans: state.running.orphans(),
        warnings: state.running.warnings(),
    })
}

/// Kill one process from the panel.
///
/// `orphan` distinguishes a leftover from a previous session (killed by pid,
/// after an identity re-check) from a live process (stopped through its owning
/// subsystem). `kind`, `root` and `key` identify a live process; `pid` is what an
/// orphan is addressed by. Returns whether something was actually terminated.
#[tauri::command]
pub async fn kill_running(
    state: State<'_, AppState>,
    pid: u32,
    kind: RunKind,
    root: String,
    key: String,
    orphan: bool,
) -> Result<bool, String> {
    if orphan {
        return kill_orphan(&state, pid).await;
    }
    match kind {
        // A configuration run/build lives in its codebase's supervisor; cancel it
        // there so the exit is reported as a cancellation, not a failure.
        RunKind::Run | RunKind::Build => match state.slot(&PathBuf::from(&root)) {
            Some(slot) => Ok(slot.supervisor.cancel(&key).await),
            None => Ok(false),
        },
        // A terminal closes through the PTY manager (tree-kills its shell's
        // children — a `claude`/`node` under it — the same as the close button).
        RunKind::Terminal => Ok(state.pty.close(&key).await),
        // Review and behavioral runs live in the global supervisor.
        RunKind::Review | RunKind::Behavioral => Ok(state.supervisor.cancel(&key).await),
    }
}

/// Kill an orphan by pid, but only after confirming the live pid is still the
/// process that was recorded — never kill a pid a different process has since
/// reused. Either way the orphan is resolved out of the registry.
async fn kill_orphan(state: &State<'_, AppState>, pid: u32) -> Result<bool, String> {
    let recorded = state.running.orphans().into_iter().find(|r| r.pid == pid);
    let Some(record) = recorded else {
        // Already resolved or never present — nothing to do.
        return Ok(false);
    };

    match cb_core::running::probe::probe(pid) {
        Some(info) if cb_core::running::classify::identity_matches(&record, &info) => {
            let killed = cb_core::process::kill_tree_async(pid).await;
            state.running.resolve_orphan(pid);
            Ok(killed)
        }
        Some(_) => {
            // The pid is alive but is now a different process. Drop the stale
            // record and refuse — killing it would kill a stranger.
            state.running.resolve_orphan(pid);
            Err(format!(
                "pid {pid} is no longer `{}` — it has been reused by another process, so it was left alone",
                record.label
            ))
        }
        None => {
            // Gone on its own between listing and now.
            state.running.resolve_orphan(pid);
            Ok(false)
        }
    }
}
