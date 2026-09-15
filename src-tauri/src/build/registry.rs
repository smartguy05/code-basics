//! Publishing what an `mcp-build` client needs to find this application.
//!
//! The **twin** of [`crate::roslyn::registry`] and [`crate::editor_context::registry`].
//! The registry itself — its shape, its tolerance of a corrupt file, and the five
//! reasons there may be no instance to talk to — is
//! [`cb_core::build::mcp::instances`]. This module is the application's side of it:
//! composing the entry, keeping its `workspaces` list current, and taking it away.
//!
//! # The pipe is process-global, so the entry always carries a listener
//!
//! Like the Roslyn and editor pipes (and unlike the browser registry, whose
//! listener comes and goes with a panel), the build pipe is opened once at startup
//! and lives for the process, so a running application's entry always names a
//! [`Listener`]. There is no "off" state to publish and no `Option` around the
//! listener.
//!
//! # Only two facts ever change: whether the app is running, and what it has open
//!
//! The entry is (re)written on exactly four events: startup (when the pipe is
//! created), and a workspace opening, becoming active, or closing (each of which
//! changes the `workspaces` list [`cb_core::build::mcp::instances::choose_instance`]
//! narrows on). Everything else — whether a build has run, what it found — is live
//! state a client learns by connecting, and putting it in the file would guarantee
//! a stale answer.

use cb_core::build::mcp::instances::{self, BuildInstance, Listener, PROTOCOL_VERSION};

use crate::state::AppState;

/// A fresh per-launch token: 32 bytes of randomness as 64 hex characters.
///
/// Two v4 UUIDs, matching [`crate::roslyn::registry::mint_token`] — a CSPRNG draw,
/// deliberately not derived from the pid, the time or the path. It is a speed bump
/// and is documented as one wherever it appears
/// ([`cb_core::build::mcp::instances::Listener::token`]); it grants nothing,
/// because the boundary here is the `--workspace` scope, not a token.
pub fn mint_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// This executable's path, or an empty string when it cannot be read.
///
/// Empty rather than a guess: [`cb_core::mcp::liveness`] matches nothing against an
/// empty recorded path, so an entry that could not identify itself is never
/// selected — the safe direction, because the alternative is a client driving a
/// process nobody identified.
pub fn own_exe() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

/// Compose this application's entry.
pub fn instance_for(
    pid: u32,
    exe: String,
    listener: Listener,
    workspaces: Vec<String>,
) -> BuildInstance {
    BuildInstance {
        pid,
        exe,
        protocol: PROTOCOL_VERSION,
        listener,
        workspaces,
    }
}

/// Write (or replace) this application's entry.
///
/// Read-modify-write rather than a whole-file write: other applications have their
/// own entries in the same file and they are not this process's to discard.
pub fn publish(instance: BuildInstance) -> Result<(), String> {
    let path = instances::instances_path();
    let mut file = instances::load(&path);
    instances::upsert(&mut file, instance);
    instances::save(&path, &file).map_err(|error| format!("{error:#}"))
}

/// The workspace roots this application currently has open, in the display form
/// the registry compares against.
fn open_roots(state: &AppState) -> Vec<String> {
    state
        .open_workspaces()
        .into_iter()
        .map(|workspace| workspace.root.display().to_string())
        .collect()
}

/// (Re)publish this application's entry, carrying the pipe it already opened and
/// the workspaces it now has open.
///
/// A no-op when there is no listener — off Windows, or when the pipe failed to
/// start: there is then nothing an agent could reach, so nothing to advertise.
/// Failures go to stderr and nowhere else; a user did not ask for an MCP server
/// and a modal about one they may not use would be worse than the thing it
/// reports.
#[cfg(windows)]
pub fn republish(state: &AppState) {
    let Some(listener) = state.build_pipe_published() else {
        return;
    };
    let entry = instance_for(std::process::id(), own_exe(), listener, open_roots(state));
    if let Err(error) = publish(entry) {
        eprintln!("code-basics: the build instance registry could not be written: {error}");
    }
}

#[cfg(not(windows))]
pub fn republish(_state: &AppState) {}

/// Remove this application's entry — on shutdown.
///
/// A leftover entry is not fatal (liveness is re-probed, so a dead pid is
/// [`cb_core::build::mcp::instances::InstanceError::NoneRunning`]) but it is worth
/// clearing: it makes a second application's entry look like an ambiguity.
pub fn withdraw(pid: u32) -> Result<(), String> {
    let path = instances::instances_path();
    let mut file = instances::load(&path);
    instances::remove(&mut file, pid);
    instances::save(&path, &file).map_err(|error| format!("{error:#}"))
}
