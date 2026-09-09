//! Publishing what an MCP client needs to find this application.
//!
//! The registry itself — its shape, its tolerance of a corrupt file, and the
//! five reasons there may be no instance to talk to — is
//! [`cb_core::browser::instances`]. This module is the application's side of it:
//! composing the entry, keeping it current, and taking it away.
//!
//! # Every state change republishes, because the file is the only channel
//!
//! A client cannot ask a question before it knows where to ask it, so the file
//! has to answer *whether* to ask at all. That means it is rewritten on four
//! events and no others: startup, the browser panel opening (which is what
//! creates the pipe), the panel closing, and the plugin being switched on or
//! off. Anything else — a navigation, a consent grant — is live state that a
//! client learns by asking, and putting it in the file would guarantee a stale
//! answer.
//!
//! # The plugin being off is published, not omitted
//!
//! An absent entry means *no application is running*; an entry with
//! `browserFeature: false` means *the user switched the browser off*. Those are
//! one setting apart and a client must be able to tell them apart **without
//! connecting** — which is the whole reason the entry exists in the off state,
//! and why [`instance_for`] refuses to record a listener alongside it.

use cb_core::browser::instances::{self, BrowserInstance, Listener, PROTOCOL_VERSION};

/// A fresh per-launch token: 32 bytes of randomness as 64 hex characters.
///
/// Two v4 UUIDs rather than a new dependency — `uuid` is already here, and its
/// v4 is a CSPRNG draw. Deliberately **not** derived from the pid, the time or
/// the path: those are all things a local prober can compute.
///
/// It is a speed bump and is documented as one everywhere it appears; see
/// [`cb_core::browser::wire`]. It sits in a file the same user can read, so it
/// reduces "any local program" to "any program that can read this user's
/// `%APPDATA%`" and nothing more. It grants nothing at all.
pub fn mint_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Compose this application's entry.
///
/// **A listener is dropped when the plugin is off, structurally rather than by
/// convention.** The two facts arrive from different places (a feature setting
/// and a running pipe) and a caller could pass a stale pair; an entry that
/// advertised a pipe while claiming the browser was switched off would send a
/// client to connect to something that is not there and, worse, would make the
/// two states indistinguishable — which is the one thing this file exists to
/// prevent.
pub fn instance_for(
    pid: u32,
    exe: String,
    browser_feature: bool,
    listener: Option<Listener>,
    workspaces: Vec<String>,
) -> BrowserInstance {
    BrowserInstance {
        pid,
        exe,
        protocol: PROTOCOL_VERSION,
        browser_feature,
        listener: if browser_feature { listener } else { None },
        workspaces,
    }
}

/// This executable's path, or an empty string when it cannot be read.
///
/// Empty rather than a guess: [`cb_core::browser::liveness::same_executable`]
/// matches nothing against an empty recorded path, so an entry that could not
/// identify itself is never selected — which is the safe direction, because the
/// alternative is a client driving a process nobody identified.
pub fn own_exe() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

/// Write (or replace) this application's entry.
///
/// Read-modify-write rather than a whole-file write: other applications have
/// their own entries in the same file and they are not this process's to
/// discard.
pub fn publish(instance: BrowserInstance) -> Result<(), String> {
    let path = instances::instances_path();
    let mut file = instances::load(&path);
    instances::upsert(&mut file, instance);
    instances::save(&path, &file).map_err(|error| format!("{error:#}"))
}

/// Publish this application at startup, before any panel exists.
///
/// The state this records is *running, with the plugin in whatever state the
/// user left it, and no browser panel open* - which is the answer a client
/// needs in order not to report "no application is running" at somebody whose
/// application is right in front of them.
///
/// Failures are reported to stderr and nothing else. A registry that cannot be
/// written costs agent access to the browser and nothing else, and a modal at
/// startup about an MCP server the user may not use would be worse than the
/// thing it reports.
pub fn announce_startup() {
    let enabled = cb_core::features::load(&cb_core::features::features_path())
        .is_enabled(cb_core::features::FeatureId::WebBrowser);
    // No workspaces yet at this point in `setup` in the common case, and that
    // is fine: the list only labels an ambiguity refusal, and the entry is
    // rewritten on every panel open.
    let entry = instance_for(std::process::id(), own_exe(), enabled, None, Vec::new());
    if let Err(error) = publish(entry) {
        eprintln!("code-basics: the browser instance registry could not be written: {error}");
    }
}

/// Remove this application's entry — on shutdown, and after a failed publish.
///
/// A leftover entry is not fatal (liveness is re-probed, so a dead pid is
/// `NoneRunning`) but it is worth clearing: it makes a second application's
/// entry look like an ambiguity.
pub fn withdraw(pid: u32) -> Result<(), String> {
    let path = instances::instances_path();
    let mut file = instances::load(&path);
    instances::remove(&mut file, pid);
    instances::save(&path, &file).map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
