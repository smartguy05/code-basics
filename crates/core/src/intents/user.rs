//! User-authored intents: a note the user writes on a card when no agent hook
//! recorded one — a manual edit, or an agent that did not follow the hooks.
//!
//! Rather than a parallel pipeline, a user note reuses the whole recorded-intent
//! machinery. It is stored as the **content** of the card's changed lines plus a
//! declared label, and at load it is converted into ordinary [`IntentRecord`]s
//! and an [`IntentLabel`] and merged into [`Intents`]. Attribution then rebinds
//! it to the diff *by content* (so it survives the lines moving) and grouping
//! titles the card with it — exactly as for an agent intent.
//!
//! Two properties make it behave the way the user expects:
//!
//! * **True overwrite.** [`merge_into`] rebases every user record's sequence
//!   number above every agent record's, and attribution's "later edit wins"
//!   rule then hands any contested line to the user note. So a note wins even
//!   over an agent's stated intent on the same lines.
//! * **Replace, not pile up.** Re-annotating the same change replaces the
//!   previous note rather than stacking a second: [`upsert`] drops any existing
//!   note whose geometry [`overlaps`](UserIntent::overlaps) the new one.
//!
//! Stored in its own `user-intents.json`, separate from the hook-owned JSONL
//! files, so it is never clobbered by a hook append and is not wiped by
//! [`super::clear`] (which forgets *agent* history). Removed per-card instead.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{
    intents_dir, normalise_path, IntentEdit, IntentLabel, IntentRecord, Intents, LabelSource,
    ProviderId,
};

/// File under `.code-basics/intents/` holding user-authored notes.
pub const USER_FILE: &str = "user-intents.json";

/// One file's share of an annotation's geometry, as line *content*.
///
/// Positions are deliberately not stored — the same discipline
/// [`crate::git::attribution`] follows — so the note rebinds by content when the
/// lines shift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserEdit {
    pub path: String,
    #[serde(default)]
    pub old_lines: Vec<String>,
    #[serde(default)]
    pub new_lines: Vec<String>,
}

impl UserEdit {
    fn is_empty(&self) -> bool {
        self.old_lines.is_empty() && self.new_lines.is_empty()
    }

    fn lines(&self) -> impl Iterator<Item = &String> {
        self.old_lines.iter().chain(self.new_lines.iter())
    }
}

/// A user's note on the specific changes of one card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIntent {
    /// Stable, colon-free id, so it survives intact inside a grouping key
    /// (`intent:usernote:{id}:{label}`) and can be recognised there.
    pub id: String,
    /// Orders notes among themselves; [`merge_into`] rebases these above every
    /// agent sequence number at load, which is what makes a note win a tie.
    #[serde(default)]
    pub seq: u64,
    pub label: String,
    pub edits: Vec<UserEdit>,
}

impl UserIntent {
    /// The synthetic turn id. The `usernote:` prefix (not a bare `user:`) is
    /// distinctive enough that no agent turn collides with it, so a grouping key
    /// beginning `intent:usernote:` unambiguously marks a user-authored card.
    pub fn turn_id(&self) -> String {
        format!("usernote:{}", self.id)
    }

    /// Any real geometry to bind? A note with only empty edits would match
    /// nothing and is not worth storing.
    pub fn is_empty(&self) -> bool {
        self.edits.iter().all(UserEdit::is_empty)
    }

    /// Do two notes touch the same change? They overlap when they share a file
    /// and any one changed line — which is what lets re-annotating a card
    /// replace the earlier note rather than stack a second one on the same code.
    pub fn overlaps(&self, other: &UserIntent) -> bool {
        for a in &self.edits {
            let mine: HashSet<&String> = a.lines().collect();
            if mine.is_empty() {
                continue;
            }
            for b in &other.edits {
                if normalise_path(&a.path) != normalise_path(&b.path) {
                    continue;
                }
                if b.lines().any(|l| mine.contains(l)) {
                    return true;
                }
            }
        }
        false
    }
}

/// Insert a note, first dropping any with the same id or overlapping geometry.
///
/// The overlap drop is what makes editing a card's note an *overwrite*: the new
/// note covers the same lines, so the old one is removed rather than left to
/// compete with it.
pub fn upsert(list: &mut Vec<UserIntent>, new: UserIntent) {
    list.retain(|u| u.id != new.id && !u.overlaps(&new));
    list.push(new);
}

/// Drop every note overlapping `geom`, returning whether anything was removed.
/// The clear-a-card action: the caller passes the card's current geometry.
pub fn remove_overlapping(list: &mut Vec<UserIntent>, geom: &UserIntent) -> bool {
    let before = list.len();
    list.retain(|u| !u.overlaps(geom));
    before != list.len()
}

/// The next unused id/sequence for a note list. Ids are `u{n}` — colon-free, so
/// they survive inside a grouping key.
pub fn next_id(list: &[UserIntent]) -> (String, u64) {
    let seq = list.iter().map(|u| u.seq).max().map_or(0, |m| m + 1);
    (format!("u{seq}"), seq)
}

/// Convert notes to the records and labels the rest of the pipeline consumes.
///
/// Each edit becomes one [`IntentRecord`]; each note contributes one declared
/// [`IntentLabel`] scoped to the files it touches. Sequence numbers here are the
/// notes' own; [`merge_into`] rebases them for overwrite.
pub fn to_intents(list: &[UserIntent]) -> (Vec<IntentRecord>, Vec<IntentLabel>) {
    let mut records = Vec::new();
    let mut labels = Vec::new();

    for note in list {
        if note.is_empty() {
            continue;
        }
        let turn_id = note.turn_id();
        let mut paths = Vec::new();

        for (i, edit) in note.edits.iter().enumerate() {
            if edit.is_empty() {
                continue;
            }
            let path = normalise_path(&edit.path);
            if !paths.contains(&path) {
                paths.push(path.clone());
            }
            records.push(IntentRecord {
                provider: ProviderId::User,
                turn_id: turn_id.clone(),
                tool_use_id: format!("{turn_id}:{i}"),
                seq: note.seq,
                path,
                edit: IntentEdit {
                    old_lines: edit.old_lines.clone(),
                    new_lines: edit.new_lines.clone(),
                    whole_file: false,
                },
                branch: None,
            });
        }

        labels.push(IntentLabel {
            provider: ProviderId::User,
            turn_id,
            label: note.label.clone(),
            paths,
            anchor: None,
            source: LabelSource::Declared,
        });
    }

    (records, labels)
}

/// Merge stored user notes into a loaded [`Intents`], as the highest-priority
/// records so they win any contested line (true overwrite).
pub fn merge_into(root: &Path, intents: &mut Intents) -> Result<()> {
    let list = load(root)?;
    if list.is_empty() {
        return Ok(());
    }

    let (mut records, labels) = to_intents(&list);

    // Rebase every user record above every agent record, so attribution's
    // "later edit wins" hands a contested line to the user's note.
    let base = intents
        .records
        .iter()
        .map(|r| r.seq)
        .max()
        .map_or(0, |m| m + 1);
    for (i, record) in records.iter_mut().enumerate() {
        record.seq = base + i as u64;
    }

    intents.records.extend(records);
    intents.labels.extend(labels);
    Ok(())
}

pub fn user_intents_path(root: &Path) -> PathBuf {
    intents_dir(root).join(USER_FILE)
}

/// Read the stored notes. Missing is empty; a corrupt file is treated as empty
/// rather than failing the whole intent load (consistent with the JSONL reader).
pub fn load(root: &Path) -> Result<Vec<UserIntent>> {
    let path = user_intents_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

/// Write the notes, creating the directory and its gitignore entry if needed.
pub fn save(root: &Path, list: &[UserIntent]) -> Result<()> {
    let dir = intents_dir(root);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    crate::config::ensure_gitignore(&crate::config::config_dir(root))?;

    let json = serde_json::to_string_pretty(list).context("failed to serialise user intents")?;
    let path = user_intents_path(root);
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;
