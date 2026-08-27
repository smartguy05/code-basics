//! The running-process registry behind the Running panel.
//!
//! Records every process the app spawns — configuration runs, builds, the
//! interactive terminals, the review and behavioral runs — so the user can see
//! what is running (now that everything spawns headless) and kill anything stray,
//! and so a process that outlived a crash can be found again on the next launch.
//!
//! Layering mirrors the rest of the crate: `record` is pure data + serde,
//! `classify` is the pure orphan decision, `store` is the fs-backed handle, and
//! `probe` is the one platform seam (sysinfo). The abstain rule is sharpened here
//! because the action gated is a *kill*: an orphan is only ever surfaced, and only
//! ever killed, when the live pid's identity matches what was recorded.

pub mod classify;
pub mod probe;
pub mod record;
pub mod store;

use std::time::{SystemTime, UNIX_EPOCH};

pub use classify::{classify_orphans, OrphanClassification, ProcInfo};
pub use record::{RunKind, RunningFile, RunningRecord, RunningReport};
pub use store::{running_path, RunningStore};

/// What a spawn site tells the registry about the process it just started, beyond
/// the pid and OS identity the registry captures itself.
#[derive(Debug, Clone)]
pub struct RunMeta {
    /// The workspace root the process belongs to (display + kill routing).
    pub root: String,
    /// Human label: configuration name or terminal title.
    pub label: String,
    /// What kind of process it is.
    pub kind: RunKind,
}

/// Build a record for a just-spawned pid, capturing the OS's view of it (image
/// name + start time) for the later PID-reuse guard. Falls back to
/// `fallback_program` and the current wall clock if the OS cannot see the pid yet
/// (a race the identity guard then treats leniently — same-second, same-name).
pub fn observe(pid: u32, key: &str, meta: RunMeta, fallback_program: &str) -> RunningRecord {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let (program, started_at_ms) = match probe::probe(pid) {
        Some(info) => (info.name, info.started_at_ms),
        None => (fallback_program.to_string(), now_ms),
    };
    RunningRecord {
        pid,
        kind: meta.kind,
        label: meta.label,
        root: meta.root,
        key: key.to_string(),
        program,
        started_at_ms,
    }
}
