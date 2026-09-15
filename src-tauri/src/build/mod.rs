//! The application's side of the Build & Diagnostics MCP server: the
//! process-global control pipe, the instance registry, and the dispatch that runs
//! a build through the workspace's [`cb_core::process::Supervisor`] and answers a
//! tool call from the cached [`cb_core::build::BuildReport`].
//!
//! Every decision lives in [`cb_core::build`] and [`cb_core::build::mcp`]; this is
//! the I/O and the wiring. Layout mirrors [`crate::roslyn`] and
//! [`crate::editor_context`]:
//!
//! | Module | Role |
//! |---|---|
//! | [`pipe`] | The named-pipe listener and its dispatch (Windows only). |
//! | [`registry`] | Publishing this application into the build instance registry. |
//! | [`agent`] | Running a build, and answering a tool call from the cache. |
//!
//! Like the Roslyn and editor pipes (and unlike the browser pipe, tied to an open
//! panel), the build pipe is **process-global**: [`start_listener`] opens it once
//! at startup, and each call resolves its own `--workspace` afresh through
//! [`crate::state::AppState::slot_for_root`].
//!
//! # It runs the build, it does not just read it
//!
//! The one structural difference from the Roslyn and editor hosts: `get_errors`,
//! `get_warnings` and `get_build_status` read a cached report, but `build_solution`
//! actually *runs* `dotnet build` through the slot's supervisor, parses the
//! MSBuild file-logger artifacts and caches the result — see [`agent`].

pub mod agent;
#[cfg(windows)]
pub mod pipe;
pub mod registry;

use crate::state::AppState;
use tauri::AppHandle;

/// Open the process-global Build control pipe and publish this application in the
/// build instance registry. Called once, at startup, from inside the Tokio
/// runtime.
///
/// A failure to start the pipe costs agent access to the build and nothing else,
/// so it is reported to stderr and the application carries on with no registry
/// entry — an absent entry reads as
/// [`cb_core::build::mcp::instances::InstanceError::NoneRunning`], which is the
/// truth.
#[cfg(windows)]
pub fn start_listener(state: &AppState, app: &AppHandle) {
    match pipe::start(app.clone(), registry::mint_token()) {
        Ok(listener) => {
            state.set_build_pipe(Some(listener));
            registry::republish(state);
        }
        Err(error) => eprintln!("code-basics: {error}"),
    }
}

#[cfg(not(windows))]
pub fn start_listener(_state: &AppState, _app: &AppHandle) {}
