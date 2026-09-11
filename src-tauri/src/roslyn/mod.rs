//! The application's side of the Roslyn MCP server: the process-global control
//! pipe, the instance registry, and the dispatch that answers a tool call from
//! the warm per-workspace [`cb_core::lsp::session::LspHandle`].
//!
//! Every decision lives in [`cb_core::roslyn`]; this is the I/O and the wiring.
//! Layout mirrors [`crate::browser`]'s pipe host:
//!
//! | Module | Role |
//! |---|---|
//! | [`pipe`] | The named-pipe listener and its dispatch (Windows only). |
//! | [`registry`] | Publishing this application into the instance registry. |
//! | [`agent`] | Mapping a tool call onto a handle method and a renderer. |
//!
//! Unlike the browser pipe (tied to an open panel), the Roslyn pipe is
//! **process-global**: [`start_listener`] opens it once at startup, and each call
//! resolves its own `--workspace` afresh through
//! [`crate::state::AppState::lsp_for_root`].

pub mod agent;
#[cfg(windows)]
pub mod pipe;
pub mod registry;

use crate::state::AppState;
use tauri::AppHandle;

/// Open the process-global Roslyn control pipe and publish this application in the
/// instance registry. Called once, at startup, from inside the Tokio runtime.
///
/// A failure to start the pipe costs agent access to the language server and
/// nothing else, so it is reported to stderr and the application carries on with
/// no registry entry — an absent entry reads as
/// [`cb_core::roslyn::instances::InstanceError::NoneRunning`], which is the truth.
#[cfg(windows)]
pub fn start_listener(state: &AppState, app: &AppHandle) {
    match pipe::start(app.clone(), registry::mint_token()) {
        Ok(listener) => {
            state.set_roslyn_pipe(Some(listener));
            registry::republish(state);
        }
        Err(error) => eprintln!("code-basics: {error}"),
    }
}

#[cfg(not(windows))]
pub fn start_listener(_state: &AppState, _app: &AppHandle) {}
