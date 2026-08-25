//! Notes / scratchpad commands.
//!
//! The bridge and nothing else: the store — where the file lives, how a missing
//! or corrupt one is tolerated — is all in [`cb_core::notes`]. Notes are
//! user-global, not per-workspace, so these commands never touch `AppState`
//! (exactly like [`crate::commands::enhancements::list_prompts`]).

use cb_core::notes::{self, NotesFile};

/// Read the global notes file. A missing or unreadable file is an empty set of
/// notes, not an error.
#[tauri::command]
pub async fn read_notes() -> Result<NotesFile, String> {
    Ok(notes::load(&notes::notes_path()))
}

/// Write the global notes file, creating its directory if absent.
#[tauri::command]
pub async fn write_notes(file: NotesFile) -> Result<(), String> {
    notes::save(&notes::notes_path(), &file).map_err(|e| format!("{e:#}"))
}
