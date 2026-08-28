//! What a launchable is, and the shape of the file that stores them.
//!
//! Pure data + serde, no I/O. The camelCase keys are pinned by a test so a
//! rename cannot silently break the hand-written TS mirror in
//! `src/ipc/types.ts` — the same contract `model.rs` and `running/record.rs`
//! follow. There is deliberately **no `skip_serializing_if`**: an entry with no
//! user-given label must cross as an explicit `null`, so "never renamed" and
//! "renamed to nothing" cannot collapse into the same absent key.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

/// One command the user has run, or pinned to run again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Launchable {
    /// Stable id: the React key and the target of pin/rename/delete.
    pub id: String,
    /// The command line exactly as the user typed it. Kept verbatim rather than
    /// pre-split so the picker can show what they wrote and re-parse it under
    /// whatever `shell` says at run time.
    pub command: String,
    /// Where it runs. Part of the identity of a recent: the same command in two
    /// checkouts is two entries, which is also what makes the "this codebase"
    /// grouping possible without a per-repository store.
    pub cwd: PathBuf,
    /// Environment applied over the inherited one. Empty for anything the picker
    /// creates today; present so a hand-edited file can carry it.
    pub env: BTreeMap<String, String>,
    /// The user's rename, or `None` to show the command itself.
    pub label: Option<String>,
    /// Run the command line through the default shell instead of splitting it
    /// into argv. Required for anything using `|`, `>` or `&&`.
    pub shell: bool,
    /// Pinned entries sort first and are never evicted by the recents cap.
    pub pinned: bool,
    /// When it last ran, ms since the Unix epoch. The clock is the caller's, so
    /// the recents policy stays a pure function.
    pub last_run_ms: u64,
    /// How many times it has been run. Shown as a hint, never used to rank.
    pub run_count: u32,
}

/// The whole launchers file: a schema version and the entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LauncherFile {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    /// The entries, in the order they were recorded. Display order is decided by
    /// [`crate::launcher::recents::group`], not by this list.
    #[serde(default)]
    pub entries: Vec<Launchable>,
}

fn default_version() -> u32 {
    1
}

impl Default for LauncherFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            entries: Vec::new(),
        }
    }
}

/// What the picker renders: the open codebase's commands, then everything else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LauncherGroups {
    /// Entries whose `cwd` is inside the active workspace root.
    pub this_codebase: Vec<Launchable>,
    /// Everything else the user has run, from any directory.
    pub global: Vec<Launchable>,
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
