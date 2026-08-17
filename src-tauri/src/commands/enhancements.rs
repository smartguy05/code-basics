//! Instruction-template commands.
//!
//! The bridge and nothing else: every decision — parsing a template, where its
//! section belongs, whether a file already carries it — lives in
//! [`cb_core::enhancements`]. The one thing only the Tauri layer knows is where
//! the bundled default templates were installed, resolved exactly as the
//! inspector sidecar resolves its own bundled directory.

use std::path::PathBuf;

use cb_core::enhancements::{self, EnhancementInfo, PromptInfo};
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// Where a bundled resource directory lives, when it was bundled at all.
///
/// `None` is an ordinary answer: a `cargo`-only build produces no resource
/// directory, and under `pnpm tauri dev` the `CB_*_PATH` overrides point the
/// libraries straight at the source folders instead.
fn bundled_dir(app: &AppHandle, name: &str) -> Option<PathBuf> {
    app.path()
        .resolve(name, tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|dir| dir.is_dir())
}

/// Seed a user library directory from its bundled defaults, then return the
/// directory. Seeding never overwrites files the user has edited.
fn seeded(dir: PathBuf, app: &AppHandle, bundled_name: &str) -> PathBuf {
    let _ = enhancements::seed(&dir, bundled_dir(app, bundled_name).as_deref());
    dir
}

/// The seeded instruction-templates directory.
fn seeded_dir(app: &AppHandle) -> PathBuf {
    seeded(enhancements::templates_dir(), app, "instructions")
}

/// Every instruction template found on disk, flagged with whether it is
/// installed in the current workspace.
#[tauri::command]
pub async fn list_enhancements(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<EnhancementInfo>, String> {
    let root = state.workspace_root()?;
    let dir = seeded_dir(&app);
    Ok(enhancements::list(&dir, &root))
}

/// Add one template's section to `CLAUDE.md` and `AGENTS.md`, then return the
/// refreshed listing.
#[tauri::command]
pub async fn add_enhancement(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<EnhancementInfo>, String> {
    let root = state.workspace_root()?;
    let dir = seeded_dir(&app);
    enhancements::add(&root, &dir, &id).map_err(|e| format!("{e:#}"))?;
    Ok(enhancements::list(&dir, &root))
}

/// Remove one template's section from both agent files, then return the
/// refreshed listing.
#[tauri::command]
pub async fn remove_enhancement(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<EnhancementInfo>, String> {
    let root = state.workspace_root()?;
    let dir = seeded_dir(&app);
    enhancements::remove_from_agents(&root, &id).map_err(|e| format!("{e:#}"))?;
    Ok(enhancements::list(&dir, &root))
}

/// Every prompt found on disk, each carrying the body to copy to the clipboard.
///
/// Prompts need no workspace — they are copied, not written — so this does not
/// touch `AppState`.
#[tauri::command]
pub async fn list_prompts(app: AppHandle) -> Result<Vec<PromptInfo>, String> {
    let dir = seeded(enhancements::prompts_dir(), &app, "prompts");
    Ok(enhancements::list_prompts(&dir))
}
