//! Persistence for the per-workspace task store: path resolution plus the
//! tolerant [`load`] and atomic, crash-safe [`save`], copied verbatim from
//! [`crate::notes`] so the two stores behave identically on disk.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::TasksFile;

/// The file name inside `.code-basics/`.
pub const TASKS_FILE: &str = "tasks.json";

/// Where the tasks file lives: `<root>/.code-basics/tasks.json`.
///
/// `CB_TASKS_PATH` overrides the whole path (used by tests, and under
/// `pnpm tauri dev`), matching how `CB_NOTES_PATH` overrides the notes path.
/// Otherwise the base is the workspace's own `.code-basics/` directory, reusing
/// [`crate::config::config_dir`], so the store cannot drift from the rest of the
/// per-workspace state.
pub fn tasks_path(root: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("CB_TASKS_PATH") {
        return PathBuf::from(path);
    }
    crate::config::config_dir(root).join(TASKS_FILE)
}

/// Read the tasks at `path`. A missing or unparseable file yields an empty
/// [`TasksFile`] rather than an error — the store is a convenience, and a corrupt
/// one must not prevent the panel from opening.
pub fn load(path: &Path) -> TasksFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return TasksFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// A sibling of `path` with `suffix` appended to its file name
/// (`tasks.json` + `.tmp` → `tasks.json.tmp`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write the tasks to `path`, creating the parent directory if absent.
///
/// The write is **atomic and crash-safe**: the JSON is written to a sibling
/// `.tmp` file and then renamed over the target, so a crash mid-write can never
/// leave a truncated file — which, given the tolerant [`load`], would otherwise
/// read back as an empty list and then be clobbered on the next save.
/// `fs::rename` replaces the destination atomically on both Windows and POSIX.
///
/// As a second guard against that same cascade, replacing a **non-empty** file
/// with an **empty** one first copies the previous content to a sibling `.bak`.
/// A deliberate "delete all" still applies, but the old tasks stay recoverable.
pub fn save(path: &Path, tasks: &TasksFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    if tasks.tasks.is_empty() && !load(path).tasks.is_empty() {
        // Best effort: a failed backup must not block the save itself.
        let _ = std::fs::copy(path, sibling(path, ".bak"));
    }

    let json = serde_json::to_string_pretty(tasks).context("failed to serialise tasks")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}
