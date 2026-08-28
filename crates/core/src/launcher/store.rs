//! The launchers file on disk: `<config>/code-basics/launchers.json`.
//!
//! A copy of [`crate::notes`]'s shape on purpose — same user-global location,
//! same tolerant read, same atomic crash-safe write — because it is the same
//! kind of store: small, user-owned, and never part of a repository. The path is
//! resolved once by [`launchers_path`] and passed into [`load`]/[`save`], so
//! tests drive the store against a temp file without racing on an environment
//! variable.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::model::LauncherFile;

/// The file name inside `code-basics/`.
pub const LAUNCHERS_FILE: &str = "launchers.json";

/// Where the launchers file lives: `<config>/code-basics/launchers.json`.
///
/// `CB_LAUNCHERS_PATH` overrides the whole path (used under `pnpm tauri dev`);
/// otherwise the base is `%APPDATA%`, then `$XDG_CONFIG_HOME`, then `~/.config`,
/// then the current directory — the same ladder as [`crate::notes::notes_path`].
pub fn launchers_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_LAUNCHERS_PATH") {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(LAUNCHERS_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Read the launchers at `path`. A missing or unparseable file yields an empty
/// store rather than an error — a corrupt file must not stop the picker opening.
pub fn load(path: &Path) -> LauncherFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return LauncherFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// A sibling of `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write the launchers to `path`, creating the parent directory if absent.
///
/// Atomic (temp + rename) for the same reason as [`crate::notes::save`]: the
/// tolerant [`load`] would read a truncated file as an empty store and then
/// clobber it on the next save. Replacing a non-empty store with an empty one
/// first copies the previous content to a sibling `.bak`.
pub fn save(path: &Path, file: &LauncherFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    if file.entries.is_empty() && !load(path).entries.is_empty() {
        // Best effort: a failed backup must not block the save itself.
        let _ = std::fs::copy(path, sibling(path, ".bak"));
    }

    let json = serde_json::to_string_pretty(file).context("failed to serialise launchers")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
