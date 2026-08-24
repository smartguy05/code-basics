//! Interactive floating terminals.
//!
//! The bridge for `cb_core::pty`: output streams to the webview over a
//! `Channel<TerminalEvent>` (the one-way path, like `run::start_run`), and
//! keystrokes, resizes and close come back as ordinary commands (the path
//! `run` never needed, because a supervised process has no stdin). The
//! `PtyManager` in `AppState` keys every session by the id minted here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cb_core::pty::{PtySpec, TerminalEvent};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::AppState;

/// Where a new terminal should start: the caller's choice, or the open
/// workspace root when they express none.
///
/// Extracted from the command body because it is a defaulting decision, which
/// a `#[tauri::command]` cannot be tested through — see the module rules in
/// `CLAUDE.md`.
pub(crate) fn spec_cwd(requested: Option<String>, workspace_root: PathBuf) -> PathBuf {
    match requested {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => workspace_root,
    }
}

/// Forward terminal events onto the IPC channel until the session ends.
fn forward(mut events: mpsc::Receiver<TerminalEvent>, channel: Channel<TerminalEvent>) {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // A closed channel means the panel (or window) went away; stop
            // forwarding. The `close` command tears the PTY down separately.
            if channel.send(event).is_err() {
                break;
            }
        }
    });
}

/// Open a terminal and stream its output to `channel`. Returns the session id
/// used by the write/resize/close commands.
#[tauri::command]
pub async fn terminal_open(
    state: State<'_, AppState>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    channel: Channel<TerminalEvent>,
) -> Result<String, String> {
    // A terminal does not require an open workspace (a bare shell is useful on
    // the welcome screen), so fall back to the user's home rather than erroring
    // when none is open.
    let root = state
        .workspace_root()
        .ok()
        .or_else(dirs_home)
        .unwrap_or_else(|| PathBuf::from("."));
    let spec = PtySpec {
        shell: cb_core::pty::default_shell(),
        args: Vec::new(),
        cwd: spec_cwd(cwd, root),
        cols,
        rows,
        env: BTreeMap::new(),
    };

    let id = Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel);

    state
        .pty
        .open(&id, spec, tx)
        .map_err(|e| format!("{e:#}"))?;
    Ok(id)
}

/// Send keystrokes to a terminal.
#[tauri::command]
pub async fn terminal_write(
    state: State<'_, AppState>,
    id: String,
    data: String,
) -> Result<(), String> {
    state.pty.write(&id, &data).map_err(|e| format!("{e:#}"))
}

/// Tell a terminal its viewport changed size.
#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .pty
        .resize(&id, cols, rows)
        .map_err(|e| format!("{e:#}"))
}

/// Close a terminal, killing its process tree. Returns whether a session was
/// actually open.
#[tauri::command]
pub async fn terminal_close(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    Ok(state.pty.close(&id).await)
}

/// The ids of every open terminal.
#[tauri::command]
pub async fn terminal_list(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.pty.list())
}

/// The user's home directory, for a terminal opened with no workspace.
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_cwd_prefers_the_requested_directory() {
        assert_eq!(
            spec_cwd(Some("/work/repo".into()), PathBuf::from("/ws")),
            PathBuf::from("/work/repo")
        );
    }

    #[test]
    fn spec_cwd_falls_back_to_the_workspace_root_when_absent() {
        assert_eq!(spec_cwd(None, PathBuf::from("/ws")), PathBuf::from("/ws"));
    }

    #[test]
    fn spec_cwd_treats_a_blank_request_as_absent() {
        // The frontend may send "" for "wherever the workspace is"; a blank
        // path would otherwise spawn in the process's own cwd, which is not the
        // repository the user is looking at.
        assert_eq!(
            spec_cwd(Some("   ".into()), PathBuf::from("/ws")),
            PathBuf::from("/ws")
        );
    }
}
