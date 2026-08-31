//! The record of one process the app has spawned, and the on-disk file format.
//!
//! Pure data + serde, no I/O. The camelCase keys are pinned by a test so a rename
//! cannot silently break the hand-written TS mirror in `src/ipc/types.ts`, the
//! same contract `model.rs` follows.

use serde::{Deserialize, Serialize};
use specta::Type;

/// What kind of process a record describes — drives the UI icon/label and how a
/// kill is routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    /// A configuration run (`dotnet run`, `npm run dev`, …).
    Run,
    /// A build/rebuild/clean action.
    Build,
    /// An interactive floating terminal.
    Terminal,
    /// The adversarial review agent.
    Review,
    /// A behavioral before/after run.
    Behavioral,
    /// A command the user launched themselves from the app launcher — an app
    /// that belongs to no project, so it is neither a configuration `Run` nor a
    /// `Terminal`. Its own variant because the panel labels it differently and a
    /// kill routes to the global supervisor, not a codebase's.
    External,
}

/// One process the app started, as tracked for the Running panel and persisted
/// for crash-orphan detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunningRecord {
    /// OS process id.
    pub pid: u32,
    /// What kind of process this is.
    pub kind: RunKind,
    /// Human label: the configuration name, or the terminal title.
    pub label: String,
    /// The workspace root this process belongs to (display grouping + kill
    /// routing). A string because it crosses IPC and is only compared, never
    /// walked.
    pub root: String,
    /// The handle the app addresses this process by while it is alive: the
    /// supervisor config id (run/build) or the PTY session id (terminal). Routes a
    /// kill to the right subsystem and finds a terminal's record on rename. Unique
    /// within a `root`, which is why the store keys on `(root, key)`.
    pub key: String,
    /// The OS image name at spawn (e.g. `dotnet.exe`), captured for the PID-reuse
    /// identity guard — never a bare pid alone decides a kill.
    pub program: String,
    /// When it started, ms since the Unix epoch: the other half of the identity
    /// guard, and shown as an age.
    pub started_at_ms: u64,
}

/// What `list_running` hands the panel: the live set, the orphan candidates, and
/// any classification notes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunningReport {
    /// Processes running now, across every open codebase.
    pub live: Vec<RunningRecord>,
    /// Processes from a previous session still alive on the machine.
    pub orphans: Vec<RunningRecord>,
    /// Notes about records dropped during orphan classification (a reused pid).
    pub warnings: Vec<String>,
}

/// The persisted file: a schema version and the flat record list. Live and orphan
/// records are written together (their in-memory split is not persisted) because
/// on the next load every record is a candidate orphan to be re-probed anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunningFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub records: Vec<RunningRecord>,
}

fn default_version() -> u32 {
    1
}

impl Default for RunningFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            records: Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
