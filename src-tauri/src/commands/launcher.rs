//! The app launcher's bridge: run an arbitrary command line, and manage the
//! remembered ones.
//!
//! Everything that decides anything is in [`cb_core::launcher`] — how a command
//! line splits, what a re-run remembers, how the picker groups. What is left
//! here is the wiring, plus the three defaulting decisions a launch needs, each
//! extracted to a free function below and tested: where it runs, what it is
//! called, and how it is addressed while alive.
//!
//! Two choices worth knowing:
//!
//! * A launched app runs in the **global** supervisor, not the active codebase's,
//!   and is recorded with `root` set to its own cwd. An app the user started is
//!   not owned by whichever tab happened to be in front, so closing that codebase
//!   must not take it down — the same reason `Review` and `Behavioral` runs live
//!   there.
//! * The store commands take **no `AppState`**: launchers are user-global, like
//!   [`crate::commands::notes`].

use std::path::PathBuf;

use cb_core::launcher::{self, LauncherFile, LauncherGroups};
use cb_core::model::Invocation;
use cb_core::process::ProcessEvent;
use serde::Serialize;
use specta::Type;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::commands::run::forward;
use crate::state::AppState;

/// What a launch hands back: the handle to stop it by, and the entry it was
/// recorded as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchedApp {
    /// The supervisor key, for `stop_command` and the Running panel's Kill.
    pub key: String,
    /// The recents entry's id, for pin/rename/delete.
    pub id: String,
    /// The label the output tab and the Running panel row show.
    pub label: String,
    /// The resolved working directory, so the panel can say where it ran.
    pub cwd: PathBuf,
}

/// How long a label may be before it is elided. The Running panel row and the
/// output tab are both narrow, and a pasted command line can be hundreds of
/// characters; the full command is still shown in the picker and by the
/// console's Copy diagnostics.
const MAX_LABEL: usize = 60;

/// Where a launch runs: the caller's choice, else the open workspace root, else
/// the user's home, else the current directory.
///
/// A launcher deliberately does **not** require an open workspace — half the
/// point is running things that belong to no repository — so an absent root
/// falls back rather than erroring, exactly as `terminal::spec_cwd` does.
pub(crate) fn launch_cwd(requested: Option<String>, workspace_root: Option<PathBuf>) -> PathBuf {
    match requested {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path.trim()),
        _ => workspace_root
            .or_else(home_dir)
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// What to call a launched app: the user's rename, else the command line itself,
/// elided to something a narrow row can show. Never empty — a blank row would
/// leave the user with a pid and no idea what it is.
pub(crate) fn launch_label(label: Option<String>, command: &str) -> String {
    let chosen = label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| command.trim());
    elide(chosen, MAX_LABEL)
}

fn elide(text: &str, limit: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= limit {
        return text.to_string();
    }
    // `limit` counts the ellipsis, so the result never exceeds it.
    let kept: String = chars[..limit.saturating_sub(1)].iter().collect();
    format!("{}…", kept.trim_end())
}

/// The supervisor key for one launch.
///
/// A fresh id **per launch**, not per remembered command: reusing a key would
/// make the supervisor treat the second launch as a restart and kill the first
/// (see `Supervisor::run_inner`), so running the same command twice on purpose —
/// two workers, two ports — would silently give one process.
///
/// The caller may supply the key, and the frontend does: output events start
/// arriving the moment the process spawns, which is *before* this command's
/// promise resolves, so a key minted here would leave the first lines of output
/// with nowhere to go. A blank or absent request still gets one minted here, so
/// any other caller is safe.
pub(crate) fn launch_key(requested: Option<String>) -> String {
    match requested {
        Some(key) if !key.trim().is_empty() => key.trim().to_string(),
        _ => format!("ext:{}", Uuid::new_v4()),
    }
}

/// Build the invocation for a command line, or explain why it cannot be one.
pub(crate) fn launch_invocation(
    command: &str,
    cwd: PathBuf,
    shell: bool,
    env: std::collections::BTreeMap<String, String>,
) -> Result<Invocation, String> {
    let (program, args) = launcher::program_and_args(command, shell)?;
    Ok(Invocation {
        program,
        args,
        cwd,
        env,
        report: None,
        coverage: None,
        warnings: Vec::new(),
    })
}

/// The remembered commands, grouped for the picker: the open codebase's first.
#[tauri::command]
pub async fn list_launchables(state: State<'_, AppState>) -> Result<LauncherGroups, String> {
    let file = launcher::load(&launcher::launchers_path());
    let root = state.workspace_root().ok();
    Ok(launcher::group(&file.entries, root.as_deref()))
}

/// Run a command line, streaming its output to `channel`.
///
/// Resolved and recorded before it is spawned, so a command line that cannot be
/// split is reported as an error and never enters the recents — the store is a
/// list of things that ran, not of things that were typed.
#[tauri::command]
pub async fn launch_command(
    state: State<'_, AppState>,
    command: String,
    cwd: Option<String>,
    shell: bool,
    label: Option<String>,
    // The key to address this launch by. Supplied by the frontend so its output
    // channel has a destination before this command returns; minted here when
    // absent.
    key: Option<String>,
    channel: Channel<ProcessEvent>,
) -> Result<LaunchedApp, String> {
    let cwd = launch_cwd(cwd, state.workspace_root().ok());
    let invocation = launch_invocation(&command, cwd.clone(), shell, Default::default())?;

    let path = launcher::launchers_path();
    let mut file = launcher::load(&path);
    let id = launcher::record_run(&mut file, &command, &cwd, shell, now_ms());
    if let Err(error) = launcher::save(&path, &file) {
        // The store is a convenience; failing to remember a command must not
        // stop it running. Say so on the console rather than silently.
        let _ = channel.send(ProcessEvent::Output {
            stream: cb_core::process::Stream::Stderr,
            text: format!("[code-basics] could not save the launcher list: {error:#}\n"),
        });
    }

    let stored_label = launcher::recents::find(&file.entries, &id).and_then(|e| e.label.clone());
    let label = launch_label(label.or(stored_label), &command);
    let key = launch_key(key);

    let (tx, rx) = mpsc::channel(512);
    forward(rx, channel);

    let supervisor = state.supervisor.clone();
    let meta = cb_core::running::RunMeta {
        root: cwd.display().to_string(),
        label: label.clone(),
        kind: cb_core::running::RunKind::External,
    };
    let spawn_key = key.clone();
    // Detached like `start_review`: the command returns as soon as the process is
    // running so the picker closes, and the output panel follows the channel.
    tokio::spawn(async move {
        let _ = supervisor
            .run_tracked(&spawn_key, &invocation, tx, meta)
            .await;
    });

    Ok(LaunchedApp {
        key,
        id,
        label,
        cwd,
    })
}

/// Stop a launched app by the key its launch returned.
#[tauri::command]
pub async fn stop_command(state: State<'_, AppState>, key: String) -> Result<bool, String> {
    Ok(state.supervisor.cancel(&key).await)
}

/// Pin/unpin and rename a remembered command. Both in one command because both
/// are a read-modify-write of the same file and the panel offers them together.
#[tauri::command]
pub async fn save_launchable(
    id: String,
    label: Option<String>,
    pinned: Option<bool>,
) -> Result<LauncherFile, String> {
    let path = launcher::launchers_path();
    let mut file = launcher::load(&path);
    if label.is_some() {
        launcher::rename(&mut file, &id, label.as_deref());
    }
    if let Some(pinned) = pinned {
        launcher::set_pinned(&mut file, &id, pinned);
    }
    launcher::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(file)
}

/// Forget a remembered command.
#[tauri::command]
pub async fn delete_launchable(id: String) -> Result<LauncherFile, String> {
    let path = launcher::launchers_path();
    let mut file = launcher::load(&path);
    launcher::remove(&mut file, &id);
    launcher::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(file)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requested_cwd_wins() {
        assert_eq!(
            launch_cwd(Some("  /work/repo  ".into()), Some(PathBuf::from("/ws"))),
            PathBuf::from("/work/repo")
        );
    }

    #[test]
    fn a_blank_request_falls_back_to_the_workspace_root() {
        assert_eq!(
            launch_cwd(Some("   ".into()), Some(PathBuf::from("/ws"))),
            PathBuf::from("/ws")
        );
        assert_eq!(
            launch_cwd(None, Some(PathBuf::from("/ws"))),
            PathBuf::from("/ws")
        );
    }

    #[test]
    fn with_no_workspace_open_a_launch_still_has_somewhere_to_run() {
        // Half the point of the launcher is running things that belong to no
        // repository, so an absent root must not be an error.
        let resolved = launch_cwd(None, None);
        assert!(!resolved.as_os_str().is_empty());
    }

    #[test]
    fn the_label_falls_back_to_the_command_line() {
        assert_eq!(launch_label(None, "docker compose up"), "docker compose up");
        assert_eq!(
            launch_label(Some("  ".into()), "  redis-server  "),
            "redis-server"
        );
    }

    #[test]
    fn a_given_label_wins_over_the_command() {
        assert_eq!(
            launch_label(Some(" Redis ".into()), "redis-server"),
            "Redis"
        );
    }

    #[test]
    fn a_very_long_command_is_elided_to_fit_a_narrow_row() {
        let long = "node ".to_string() + &"a".repeat(200);
        let label = launch_label(None, &long);
        assert_eq!(label.chars().count(), MAX_LABEL);
        assert!(label.ends_with('\u{2026}'));
    }

    #[test]
    fn every_launch_gets_its_own_key() {
        // Reusing a key would make the supervisor restart the first process
        // instead of starting a second one.
        assert_ne!(launch_key(None), launch_key(None));
        assert!(launch_key(None).starts_with("ext:"));
    }

    #[test]
    fn a_supplied_key_is_used_as_given() {
        // The frontend mints it so its output channel has a destination before
        // this command returns.
        assert_eq!(launch_key(Some("  ext:abc  ".into())), "ext:abc");
        assert!(launch_key(Some("   ".into())).starts_with("ext:"));
    }

    #[test]
    fn an_invocation_carries_the_split_command_and_the_cwd() {
        let invocation = launch_invocation(
            "node -e 1",
            PathBuf::from("/repo"),
            false,
            Default::default(),
        )
        .unwrap();
        assert_eq!(invocation.program, "node");
        assert_eq!(invocation.args, vec!["-e", "1"]);
        assert_eq!(invocation.cwd, PathBuf::from("/repo"));
        assert!(invocation.report.is_none());
        assert!(invocation.coverage.is_none());
    }

    #[test]
    fn a_command_line_that_needs_a_shell_is_refused_rather_than_mangled() {
        let error = launch_invocation(
            "echo hi | findstr hi",
            PathBuf::from("/repo"),
            false,
            Default::default(),
        )
        .unwrap_err();
        assert!(error.contains('|'), "{error}");
        // And is accepted once the caller says to use a shell.
        assert!(launch_invocation(
            "echo hi | findstr hi",
            PathBuf::from("/repo"),
            true,
            Default::default()
        )
        .is_ok());
    }

    #[test]
    fn an_empty_command_line_is_an_error() {
        assert!(
            launch_invocation("   ", PathBuf::from("/repo"), false, Default::default()).is_err()
        );
    }
}
