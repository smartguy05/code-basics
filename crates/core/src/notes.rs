//! The global notes / scratchpad store, `code-basics/notes.json`.
//!
//! Unlike almost everything else this crate persists, notes are **user-global,
//! not per-workspace**: the file lives beside the enhancements library under the
//! user's config directory (`%APPDATA%` on Windows, `$XDG_CONFIG_HOME` / `~/.config`
//! elsewhere), so the same scratchpad is available in every project. It is never
//! part of any repository, so there is no gitignore entry to keep in step.
//!
//! Pure filesystem/serde work with no Tauri dependency, tested headlessly. The
//! path is resolved once by [`notes_path`] and passed into [`load`]/[`save`], so
//! tests drive the store against a temp file without touching the real config
//! directory (and without racing on a shared environment variable).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One note in the scratchpad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Stable id: the React key and the target of rename/delete/update.
    pub id: String,
    /// The tab label.
    pub title: String,
    /// The note text.
    pub body: String,
    /// When the note was created, milliseconds since the Unix epoch. A number so
    /// the frontend can format it however it likes; the clock is the caller's.
    pub created_at_ms: u64,
    /// When the note was last edited, milliseconds since the Unix epoch.
    pub updated_at_ms: u64,
}

/// The whole notes file: a schema version and the ordered notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotesFile {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    /// The notes, in the order the panel shows their tabs.
    #[serde(default)]
    pub notes: Vec<Note>,
}

fn default_version() -> u32 {
    1
}

impl Default for NotesFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            notes: Vec::new(),
        }
    }
}

/// The file name inside `code-basics/`.
pub const NOTES_FILE: &str = "notes.json";

/// Where the notes file lives: `<config>/code-basics/notes.json`.
///
/// `CB_NOTES_PATH` overrides the whole path (used under `pnpm tauri dev`, and by
/// anything that wants notes elsewhere), matching how `CB_PROMPTS_PATH` overrides
/// the prompts directory. Otherwise the base is `%APPDATA%`, then
/// `$XDG_CONFIG_HOME`, then `~/.config`, then the current directory — the same
/// resolution order as [`crate::enhancements`]'s library directory.
pub fn notes_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_NOTES_PATH") {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(NOTES_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Read the notes at `path`. A missing or unparseable file yields an empty
/// [`NotesFile`] rather than an error — the store is a convenience, and a corrupt
/// one must not prevent the panel from opening.
pub fn load(path: &Path) -> NotesFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return NotesFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Write the notes to `path`, creating the parent directory if absent.
pub fn save(path: &Path, notes: &NotesFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(notes).context("failed to serialise notes")?;
    std::fs::write(path, format!("{json}\n")).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
