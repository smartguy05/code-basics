//! Asking this workspace's language servers something, and saying what they do.
//!
//! Thin, like the rest of this layer, and thinner than most of it. Every
//! decision — which server serves a file, whether it is up, whether it is primed
//! yet, what a location means, and what to say when there is no answer — is in
//! `cb_core::lsp`, which is tested without a windowing system and without a
//! server installed. What is left here is resolving state, cloning a handle,
//! dropping the lock, and awaiting.
//!
//! # Nothing here turns a "no" into an empty list
//!
//! That is the whole point of the subsystem and it is easiest to undo at this
//! layer, where a `Result` is inviting: a server that is still loading, one that
//! died, one that does not advertise the capability and one that genuinely found
//! nothing are four different answers, and each arrives as an
//! [`cb_core::lsp::model::Availability`] with a message rather than as an error
//! or as `[]`. So the commands below return the core's own result types
//! unchanged. The one thing they add is the *failure to ask at all* — no
//! workspace open — and that is an `Err`, exactly as it is in every other
//! command in this layer.
//!
//! [`lsp_status`] is the deliberate exception: it never errors, because "no
//! server, and here is why" is an answer and the status surface is where the
//! user is sent to read it.

use std::path::Path;

use cb_core::lsp::model::{AnchorResult, DefinitionResult, LspStatus, UsageResult};
use cb_core::lsp::session::{self, LspHandle};
use tauri::State;

use crate::state::AppState;

/// What to say when the workspace went away underneath a request.
const GONE: &str = "the workspace was closed or replaced while its language servers were starting";

/// This workspace's session, starting it if this is the first request.
///
/// # Why starting is lazy, and idempotent
///
/// Lazy because it costs nothing to defer and something to get wrong: a session
/// resolves four languages on the calling thread, and every entry point that
/// opens a workspace would otherwise have to remember to start one. Idempotent
/// because there are several such entry points and a request can arrive before
/// any of them ran — `code-basics .` opens a workspace before any command is
/// invoked, and a workspace swap tears the previous session down without putting
/// anything in its place. A session that could only be started by one command
/// would be a permanent dead end whenever that command did not run.
///
/// Starting the session is **not** starting a server. `session::start` resolves
/// where each language's server is and starts none of them; the first document
/// or request for a language is what spawns a process. So this is a handful of
/// directory reads, and its product is the status surface — "Python is not
/// installed, here is how to get it" — available before anybody opens a file.
///
/// # The `lsp` block of `.code-basics/config.json` is read here
///
/// Which means it is read once per session, and a session survives a rescan (see
/// [`AppState::set_workspace`]: only a change of *root* tears one down). Editing
/// the `lsp` block therefore takes effect when the workspace is next opened,
/// not on the save. That is the deliberate side of the same trade: a rescan runs
/// on every configuration save, and restarting Roslyn on each one would cost
/// tens of seconds of "still loading" over a solution that did not change.
///
/// # A refusal is retried, because the loser of a race has not lost anything
///
/// Two callers can start on a cold session by design — `open_workspace` spawns
/// one, and any `lsp_*` command calls this itself — and the older claim is refused
/// on the generation check. Reading the slot once at that point is wrong: the
/// refusal happens while the winner may still be inside `session::start`, so the
/// slot is empty and the caller would be told [`GONE`] — *"the workspace was
/// closed or replaced"* — at the moment the user opened it. Nothing was closed,
/// a healthy session appears a moment later, and the message sends them looking
/// for a problem that does not exist, which is the class of confident wrong
/// reason this subsystem exists to refuse.
///
/// So a refusal loops instead. The retry re-reads the slot first (serving the
/// winner if it has published) and otherwise claims a fresh generation and starts
/// again, which supersedes whatever else was in flight — bounded, because an
/// unbounded loop against a workspace that really is being closed and re-opened
/// would spin. [`GONE`] is then only reported when there is genuinely no session
/// and no workspace to start one for.
pub fn ensure_session(state: &AppState) -> Result<LspHandle, String> {
    ensure_session_interleaved(state, || {})
}

/// How many times a refused start is re-attempted before [`GONE`] is the answer.
///
/// Three, not one: the first refusal is the ordinary two-callers-on-a-cold-session
/// race, and a second would need a *third* caller to arrive inside the same
/// window. Not unbounded, because "the workspace keeps being replaced" is a real
/// state and spinning through it would be worse than saying so.
const START_ATTEMPTS: usize = 3;

/// [`ensure_session`], with a seam at the instant between claiming a generation
/// and publishing the session — the window another caller's claim lands in.
///
/// The seam exists rather than a test that spawns two threads and hopes, for the
/// same reason `AppState`'s interleaved methods do: the window is a few directory
/// reads wide and a test that lost the race would pass.
fn ensure_session_interleaved(
    state: &AppState,
    mut interleave: impl FnMut(),
) -> Result<LspHandle, String> {
    for _ in 0..START_ATTEMPTS {
        if let Some(handle) = state.lsp() {
            return Ok(handle);
        }

        let root = state.workspace_root()?;
        // A config that will not parse cannot reach here — `open_workspace` fails
        // on it first — so the fallback is for the ordinary case of no file at
        // all, and `None` means "every built-in default", never "nothing
        // configured".
        let config = cb_core::config::load(&root)
            .ok()
            .and_then(|saved| saved.lsp);

        // Claimed before the start, so a second caller racing this one supersedes
        // it rather than publishing a second server tree behind its back.
        let generation = state.begin_lsp_session();
        interleave();
        let handle = session::start(root, config, generation);

        match state.record_lsp_session(handle) {
            // Read back rather than returned, so a swap between the publish and
            // this read is reported as the swap it is.
            Ok(()) => return state.lsp().ok_or_else(|| GONE.to_string()),
            Err(rejected) => {
                // Ours was refused: the workspace moved on, or somebody else's
                // start got there first. Either way this one is a live actor that
                // nothing else holds, and tearing it down is the obligation the
                // handback exists to create.
                rejected.request_teardown();
            }
        }
    }
    // Every attempt was refused. Serve whatever is there — the incumbent, if one
    // of the winners has published by now — and only then call it gone.
    state.lsp().ok_or_else(|| GONE.to_string())
}

/// Start this workspace's session without waiting for anything.
///
/// For the entry points that open a workspace: they want the status surface
/// populated and Roslyn's project load begun before the user asks a question,
/// and they want none of that on the path between choosing a folder and seeing
/// what is in it. A failure is dropped, because there is nobody to tell and
/// nothing has been lost — the next request calls [`ensure_session`] itself.
pub fn spawn_session(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        if let Err(reason) = ensure_session(&app.state::<AppState>()) {
            eprintln!("code-basics: no language-server session was started: {reason}");
        }
    });
}

/// What every configured server is doing.
///
/// Never an error. A workspace with no session yet, or none open at all, has
/// nothing to report about servers, and an empty list says exactly that — the
/// alternative would put an error toast in front of a user whose only mistake
/// was opening the tab. A language the session could not resolve is *not* this
/// case: it appears as a row, with everywhere that was searched and a hint.
#[tauri::command]
pub async fn lsp_status(state: State<'_, AppState>) -> Result<LspStatus, String> {
    Ok(status_for(ensure_session(&state)))
}

/// [`lsp_status`]'s one decision, outside the command body so it can be tested.
///
/// A `#[tauri::command]` takes [`State`], which cannot be built without a
/// `tauri::App`, so nothing inside one is reachable from a unit test — see the
/// `src-tauri` section of `CLAUDE.md`. That is tolerable exactly while the
/// bodies decide nothing, and this one does decide something: it turns a failure
/// into a successful empty list, which is the shape this whole subsystem exists
/// to refuse everywhere else. Leaving it inline would have left the one line
/// worth checking as the one line nothing could check.
fn status_for(session: Result<LspHandle, String>) -> LspStatus {
    match session {
        Ok(session) => session.status(),
        // Deliberately not the error: see this function's own doc and the
        // command's. "No workspace open" is not a server problem, and there are
        // no servers to report.
        Err(_) => LspStatus {
            servers: Vec::new(),
        },
    }
}

/// Tear down this workspace's language-server session and start a fresh one.
///
/// The Restart action behind the status dropdown. It helps the one failure mode
/// a restart can help — a server that ran and then crashed (a second death
/// inside the restart window latches [`cb_core::lsp::model::Availability::Failed`])
/// — by resetting the session so the next request spawns the process again and
/// replays the open documents. It cannot fix a server that never started (a
/// missing binary, a failed handshake) or a `.code-basics/config.json`
/// misconfiguration; those re-run identically, which the dropdown says.
///
/// The slot is emptied before the restart because [`AppState::record_lsp_session`]
/// refuses to publish over a live session; the old handle is torn down rather
/// than dropped, because a dropped handle would orphan its server process tree.
/// Never an error, for the same reason [`lsp_status`] is not: the status surface
/// is where the result is read.
#[tauri::command]
pub async fn lsp_restart(state: State<'_, AppState>) -> Result<LspStatus, String> {
    if let Ok(slot) = state.active_slot() {
        if let Some(handle) = slot.take_lsp() {
            handle.request_teardown();
        }
    }
    Ok(status_for(ensure_session(&state)))
}

/// The editor opened a document, or replaced its contents wholesale.
///
/// Enqueued and not confirmed: a `didOpen` has no reply to wait for, and the
/// ordering that matters — every edit reaching the server ahead of the request
/// that depends on it — is the actor's, not this call's.
#[tauri::command]
pub async fn lsp_open_document(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<(), String> {
    let session = ensure_session(&state)?;
    session.open_document(Path::new(&path), &text).await;
    Ok(())
}

/// The document's contents changed.
#[tauri::command]
pub async fn lsp_change_document(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> Result<(), String> {
    let session = ensure_session(&state)?;
    session.change_document(Path::new(&path), &text).await;
    Ok(())
}

/// The editor closed the document, so the servers go back to reading disk.
#[tauri::command]
pub async fn lsp_close_document(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let session = ensure_session(&state)?;
    session.close_document(Path::new(&path)).await;
    Ok(())
}

/// Every use site of the symbol at `line`/`character`.
///
/// `line` is **1-based**, matching the editor gutter and every other line number
/// crossing this boundary; `character` is **0-based UTF-16 code units**, which is
/// what CodeMirror hands the caller. The asymmetry is the IPC contract, it holds
/// in both directions, and `cb_core::lsp::positions` is the only thing that
/// converts either of them.
#[tauri::command]
pub async fn lsp_find_usages(
    state: State<'_, AppState>,
    path: String,
    line: u32,
    character: u32,
) -> Result<UsageResult, String> {
    let session = ensure_session(&state)?;
    Ok(session.find_usages(Path::new(&path), line, character).await)
}

/// Where the symbol at `line`/`character` is declared, implemented and typed.
///
/// Same convention as [`lsp_find_usages`]: 1-based `line`, 0-based UTF-16
/// `character`, in both directions — the [`cb_core::lsp::model::Target`]s coming
/// back are spelled the same way.
#[tauri::command]
pub async fn lsp_goto_definition(
    state: State<'_, AppState>,
    path: String,
    line: u32,
    character: u32,
) -> Result<DefinitionResult, String> {
    let session = ensure_session(&state)?;
    Ok(session
        .goto_definition(Path::new(&path), line, character)
        .await)
}

/// Which declarations in a file deserve an inline "N usages" row, and where to
/// aim the request that fills one in.
///
/// Each anchor carries a **1-based** `line` (where the row is drawn), a
/// **1-based** `selectionLine` and a **0-based UTF-16** `character` — the
/// identifier's own position, which is what [`lsp_find_usages`] must be given
/// rather than the start of the declaration.
#[tauri::command]
pub async fn lsp_declaration_anchors(
    state: State<'_, AppState>,
    path: String,
) -> Result<AnchorResult, String> {
    let session = ensure_session(&state)?;
    Ok(session.declaration_anchors(Path::new(&path)).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use cb_core::lsp::registry::Probe;
    use cb_core::workspace::Workspace;

    fn workspace_at(root: &str) -> Workspace {
        Workspace {
            root: PathBuf::from(root),
            name: "w".into(),
            projects: Vec::new(),
            configs: Vec::new(),
            solutions: Vec::new(),
            favorites: Vec::new(),
            order: Vec::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_loser_of_a_start_race_is_served_a_session_rather_than_told_the_workspace_closed() {
        // Two callers on a cold session is the designed case: `open_workspace`
        // spawns one and any `lsp_*` command starts one itself. The loser is
        // refused on the generation check *while the winner is still resolving*,
        // so the slot it would read back is empty — and reporting that as "the
        // workspace was closed or replaced" is a confident wrong reason handed to
        // a user who has just opened a workspace and whose session comes up a
        // moment later.
        //
        // The interleave stands in for the other caller, and claims a generation
        // exactly where one would land: after ours, before our publish.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let mut raced = false;
        let handle = ensure_session_interleaved(&state, || {
            // Only once: a competitor that claimed on every attempt would be a
            // test of the bound rather than of the race.
            if !raced {
                raced = true;
                // Ignored deliberately: this stands in for another caller's
                // claim, and the session it would start is not this test's
                // business — only the bump it leaves behind is.
                let _ = state.begin_lsp_session();
            }
        })
        .expect("the loser of a same-root race was told the workspace was closed");

        assert!(raced, "the race was never arranged, so nothing was tested");
        assert_eq!(handle.root(), PathBuf::from("/a"));
        assert_eq!(
            state.lsp().map(|published| published.generation()),
            Some(handle.generation()),
            "the caller was served a session other than the published one"
        );
        handle.request_teardown();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_is_started_once_and_then_handed_out() {
        // Idempotence, which is what lets every entry point call this: a second
        // call must serve the incumbent rather than start a second server tree.
        let state = AppState::default();
        state.set_workspace(workspace_at("/a")).unwrap();

        let first = ensure_session(&state).expect("a session");
        let second = ensure_session(&state).expect("a session");

        assert_eq!(
            first.generation(),
            second.generation(),
            "a second call started a second session"
        );
        first.request_teardown();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn no_workspace_is_an_empty_server_list_and_a_session_is_its_own_rows() {
        // The status surface is where a user is *sent* to find out why there is
        // no language server, so it must not be the one place that puts an error
        // toast in front of them. An empty list is the honest answer to "which
        // servers does this workspace have" when there is no workspace.
        assert!(status_for(Err("anything".to_string())).servers.is_empty());

        // And the other half, which is what stops "always empty" from passing.
        //
        // Deliberately over a probe that finds nothing rather than over the real
        // one: a session lists a language the moment it is started, *or*
        // immediately and permanently if it could not be resolved at all — so on
        // a machine with every server installed a fresh session has no rows, and
        // an assertion about a non-empty list would pass here and fail on the
        // next developer's box. With nothing installed all four languages are
        // unresolvable, which is the same everywhere.
        let session = session::start_with_probe(
            PathBuf::from("/a"),
            None,
            1,
            std::sync::Arc::new(NothingInstalled),
        );
        for _ in 0..200 {
            if !session.status().servers.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let rows = status_for(Ok(session.clone()));
        assert_eq!(
            rows.servers.len(),
            4,
            "four languages, none of them resolvable, all of them reported"
        );
        assert_eq!(
            rows.servers,
            session.status().servers,
            "the rows are the session's own, unmodified"
        );
        session.request_teardown();
    }

    /// A machine with nothing installed, so the row count is a fact about the
    /// registry rather than about whoever is running the suite.
    struct NothingInstalled;

    impl Probe for NothingInstalled {
        fn on_path(&self, _name: &str) -> Option<PathBuf> {
            None
        }
        fn is_file(&self, _path: &Path) -> bool {
            false
        }
        fn is_dir(&self, _path: &Path) -> bool {
            false
        }
        fn read_dir(&self, _path: &Path) -> Vec<PathBuf> {
            Vec::new()
        }
        fn home(&self) -> Option<PathBuf> {
            None
        }
        fn env(&self, _key: &str) -> Option<String> {
            None
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_no_workspace_open_there_is_nothing_to_start_and_that_is_an_error() {
        // The one failure this layer adds: not "the servers said no" but "there
        // was nothing to ask about". `lsp_status` turns it into an empty list.
        let state = AppState::default();
        assert!(ensure_session(&state).is_err());
    }
}
