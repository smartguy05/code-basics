//! Generic marker-based merging of hook entries into a `settings.json`-shaped
//! file, shared by the intent recorder ([`super::hooks_json`]) and the quality
//! gate ([`crate::qgate::install`]).
//!
//! The rule is the whole point and is spelled out at length in `hooks_json`:
//! **never rewrite the file.** Parse what is there, replace only our own marked
//! entries in the events we manage, and leave every other key, event and
//! handler exactly as found — including unknown ones from a future version of
//! either agent. Our entries are recognised by a marker substring in the
//! command rather than by position, so installing twice is a no-op.
//!
//! Each caller supplies its own `marker` and event list, which is what lets two
//! independent hooks (the intent recorder and the quality gate) coexist in the
//! same `Stop` array without either removing the other's entry.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

/// Does an entry carry `marker` in one of its command strings?
pub fn contains_marker(entry: &Value, marker: &str) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            handlers.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(marker))
            })
        })
}

/// Is a marked entry present in **every** one of `events`?
pub fn is_installed(path: &Path, events: &[&str], marker: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return false;
    };

    events.iter().all(|event| {
        value
            .get("hooks")
            .and_then(|h| h.get(event))
            .and_then(Value::as_array)
            .is_some_and(|entries| entries.iter().any(|e| contains_marker(e, marker)))
    })
}

/// Compute the file's contents after merging `ours` (an events→entries object)
/// in, without writing anything.
///
/// Returns the new text and whether an existing file is being merged into.
/// Bails without writing if the file exists but does not hold a JSON object, so
/// a format we do not understand is never destroyed.
pub fn merged_text(path: &Path, ours: &Value, marker: &str) -> Result<(String, bool)> {
    let existing = std::fs::read_to_string(path).ok();
    let merges_existing = existing.is_some();
    let text = merged_into_text(existing.as_deref().unwrap_or(""), ours, marker)
        .with_context(|| format!("{} was left untouched", path.display()))?;
    Ok((text, merges_existing))
}

/// Merge `ours` into a settings.json **string** (rather than a file on disk),
/// returning the new text. `existing` may be empty for a fresh file. Bails
/// without producing output if `existing` is non-empty but not a JSON object,
/// so a format we do not understand is never destroyed.
///
/// This is the seam the combined first-open setup plan uses to chain two merges
/// (the intent recorder's, then the quality gate's) into one settings.json.
pub fn merged_into_text(existing: &str, ours: &Value, marker: &str) -> Result<String> {
    let mut root_value: Value = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(existing).context("not valid JSON")?
    };

    if !root_value.is_object() {
        anyhow::bail!("does not contain a JSON object");
    }

    merge_into(&mut root_value, ours, marker);

    let mut text = serde_json::to_string_pretty(&root_value)
        .context("failed to serialise the hook configuration")?;
    text.push('\n');

    Ok(text)
}

/// Add `ours` to whatever is already there, replacing any earlier marked entry.
fn merge_into(root_value: &mut Value, ours: &Value, marker: &str) {
    let object = root_value
        .as_object_mut()
        .expect("checked to be an object by the caller");

    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));

    // A `hooks` key holding something other than an object belongs to a format
    // we do not understand; replacing it would destroy it.
    let Some(hooks) = hooks.as_object_mut() else {
        return;
    };

    for (event, entries) in ours.as_object().into_iter().flatten() {
        let slot = hooks
            .entry(event.clone())
            .or_insert_with(|| Value::Array(Vec::new()));

        let Some(existing) = slot.as_array_mut() else {
            continue;
        };

        // Ours may already be here from an earlier install.
        existing.retain(|entry| !contains_marker(entry, marker));

        if let Some(new_entries) = entries.as_array() {
            existing.extend(new_entries.iter().cloned());
        }
    }
}

/// Remove our marked entries again, leaving everything else untouched. `None`
/// when the file is absent.
pub fn plan_removal(path: &Path, events: &[&str], marker: &str) -> Result<Option<String>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };

    let mut value: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;

    let Some(hooks) = value
        .as_object_mut()
        .and_then(|o| o.get_mut("hooks"))
        .and_then(Value::as_object_mut)
    else {
        return Ok(None);
    };

    for event in events {
        if let Some(entries) = hooks.get_mut(*event).and_then(Value::as_array_mut) {
            entries.retain(|entry| !contains_marker(entry, marker));
        }
    }
    hooks.retain(|_, v| !v.as_array().is_some_and(|a| a.is_empty()));

    let mut out = serde_json::to_string_pretty(&value)
        .context("failed to serialise the hook configuration")?;
    out.push('\n');

    Ok(Some(out))
}
