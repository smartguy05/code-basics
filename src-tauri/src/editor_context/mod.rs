//! The application's side of the editor-context MCP server: the process-global
//! control pipe, the instance registry, and the dispatch that answers a tool call
//! from the live editor state the React frontend pushed into [`crate::state`].
//!
//! Every decision lives in [`cb_core::editor_context`]; this is the I/O and the
//! wiring. Layout mirrors [`crate::roslyn`]'s pipe host — its twin — with one
//! difference that shapes the whole module: where the Roslyn host reads a warm
//! [`cb_core::lsp::session::LspHandle`], this host reads
//! [`crate::state::AppState::editor_context_for`], which the frontend fills.
//!
//! | Module | Role |
//! |---|---|
//! | [`pipe`] | The named-pipe listener and its dispatch (Windows only). |
//! | [`registry`] | Publishing this application into the editor instance registry. |
//! | [`agent`] | Mapping a tool call onto the pushed state and a renderer. |
//!
//! Like the Roslyn pipe (and unlike the browser pipe, which is tied to an open
//! panel), the editor pipe is **process-global**: [`start_listener`] opens it once
//! at startup, and each call resolves its own `--workspace` afresh.

pub mod agent;
#[cfg(windows)]
pub mod pipe;
pub mod registry;

use crate::state::AppState;
use tauri::AppHandle;

/// Open the process-global editor-context control pipe and publish this
/// application in the editor instance registry. Called once, at startup, from
/// inside the Tokio runtime.
///
/// A failure to start the pipe costs agent access to the editor state and nothing
/// else, so it is reported to stderr and the application carries on with no
/// registry entry — an absent entry reads as
/// [`cb_core::editor_context::instances::InstanceError::NoneRunning`], which is the
/// truth.
#[cfg(windows)]
pub fn start_listener(state: &AppState, app: &AppHandle) {
    match pipe::start(app.clone(), registry::mint_token()) {
        Ok(listener) => {
            state.set_editor_pipe(Some(listener));
            registry::republish(state);
        }
        Err(error) => eprintln!("code-basics: {error}"),
    }
}

#[cfg(not(windows))]
pub fn start_listener(_state: &AppState, _app: &AppHandle) {}
