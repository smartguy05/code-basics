//! Marker-free merging of one named server into a JSON config file's server map.
//!
//! A **new sibling** to [`crate::intents::providers::settings_merge`], sharing
//! none of its code — deliberately, because the two shapes have nothing in
//! common beyond "JSON". `settings_merge::merge_into` is hard-wired to
//! `hooks[event] = [entries deduped by a marker substring]`: an event layer,
//! an array, and a substring search over each entry's command. `mcpServers[name]
//! = {object}` has no event layer, no array, and needs no marker at all — the
//! **key is the identity**, so re-installing replaces the value in place and a
//! moved executable refreshes rather than duplicating.
//!
//! The one parameter that keeps this from needing a second copy per file shape
//! is `pointer`: the JSON nesting path holding the server map. `["mcpServers"]`
//! serves both `<root>/.mcp.json` and `~/.claude.json`; a hypothetical local
//! scope would pass `["projects", "<root>", "mcpServers"]` and need no new code.
//!
//! The rules are copied verbatim from `settings_merge` because they are the
//! reason that module is trusted, not because they are convenient:
//!
//! * **Never rewrite the file.** Parse what is there, set exactly one key, and
//!   leave every other key — including unknown ones from a future version of
//!   the agent — exactly as found.
//! * **Bail rather than destroy.** Content that is not a JSON object, or a
//!   pointer segment holding something that is not an object, yields an error
//!   and no output at all.
//! * **Pretty-print with one trailing newline.** Note the deliberate asymmetry
//!   with [`crate::mcp::ndjson`], which is compact because the MCP wire forbids
//!   embedded newlines: a config file is read and diffed by a human, a frame is
//!   not. Both sides are pinned by tests so neither drifts into the other.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

/// Compute the file's contents after merging `entry` in under `pointer`,
/// without writing anything.
///
/// Returns the new text and whether an existing file is being merged into —
/// the second is what the preview shows the user, since changing a file they
/// already have is the case that needs their attention.
pub fn merged_text(
    path: &Path,
    pointer: &[&str],
    name: &str,
    entry: &Value,
) -> Result<(String, bool)> {
    let existing = super::read_existing(path)?;
    let merges_existing = existing.is_some();
    let text = merged_into_text(existing.as_deref().unwrap_or(""), pointer, name, entry)
        .with_context(|| format!("{} was left untouched", path.display()))?;
    Ok((text, merges_existing))
}

/// Merge `entry` into a JSON config **string**, returning the new text.
/// `existing` may be empty for a fresh file.
pub fn merged_into_text(
    existing: &str,
    pointer: &[&str],
    name: &str,
    entry: &Value,
) -> Result<String> {
    let mut root: Value = parse_object(existing)?;
    let map = descend_mut(&mut root, pointer)?;
    map.insert(name.to_string(), entry.clone());
    render(&root)
}

/// Remove our entry again, leaving everything else untouched. `None` when there
/// is nothing of ours there, so the caller can plan **zero** writes rather than
/// rewrite an unchanged file.
pub fn removal_text(existing: &str, pointer: &[&str], name: &str) -> Result<Option<String>> {
    let mut root: Value = parse_object(existing)?;

    // Descend read-only first: a file that does not have our entry must not
    // gain empty parent objects as a side effect of being asked about.
    if !is_installed_in_value(&root, pointer, name) {
        return Ok(None);
    }

    let map = descend_mut(&mut root, pointer)?;
    // `remove`, not `shift_remove`: the latter exists only under serde_json's
    // `preserve_order` feature, which this tree does not enable — the map is a
    // BTreeMap and key order is therefore unaffected by a removal.
    map.remove(name);
    render(&root).map(Some)
}

/// Remove our entry from the file at `path`. `None` when the file is absent or
/// holds no entry of ours.
///
/// An **unreadable** file is an error rather than `None`. `None` here means "we
/// looked and there is nothing of ours to take out", which an uninstall renders
/// as *nothing to remove* — a claim this function has not earned when it could
/// not read the file at all.
pub fn plan_removal(path: &Path, pointer: &[&str], name: &str) -> Result<Option<String>> {
    let Some(text) = super::read_existing(path)? else {
        return Ok(None);
    };
    removal_text(&text, pointer, name)
        .with_context(|| format!("{} was left untouched", path.display()))
}

/// Is `name` present under `pointer` in this text? Unparseable content answers
/// **no** rather than erroring: a status read is a question, not a write.
pub fn is_installed_in(existing: &str, pointer: &[&str], name: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(existing) else {
        return false;
    };
    is_installed_in_value(&value, pointer, name)
}

/// Is `name` present under `pointer` in the file at `path`?
///
/// `false` for a file that is absent or unparseable, which is why this is not
/// the function an installer should consult — see [`installed_state`]. Kept
/// because a status read that only ever asks "is it there?" is a fair question
/// with a fair answer.
pub fn is_installed(path: &Path, pointer: &[&str], name: &str) -> bool {
    installed_state(path, pointer, name).unwrap_or(false)
}

/// Three answers, not two: `Some(true)` installed, `Some(false)` definitely not,
/// and **`None` — this file could not be read, so there is no answer.**
///
/// The distinction is what stops a one-click catastrophe. The old code folded a
/// read error into "not installed", so a briefly unreadable `~/.claude.json`
/// showed the user an install button, and taking it planned a *fresh* config —
/// four lines to replace 122 KB, with the `.bak` suppressed because
/// `merges_existing` was false too. A running Claude Code rewrites that file
/// continuously, so "briefly unreadable" is its normal state, not a rare one.
pub fn installed_state(path: &Path, pointer: &[&str], name: &str) -> Option<bool> {
    match super::read_existing(path) {
        // Unreadable. Not an answer, and must not be dressed up as one.
        Err(_) => None,
        // Absent, which is a definite answer: our server is certainly not in a
        // file that does not exist.
        Ok(None) => Some(false),
        Ok(Some(text)) => Some(is_installed_in(&text, pointer, name)),
    }
}

fn is_installed_in_value(root: &Value, pointer: &[&str], name: &str) -> bool {
    let mut current = root;
    for segment in pointer {
        match current.get(*segment) {
            Some(next) => current = next,
            None => return false,
        }
    }
    current.get(name).is_some()
}

fn parse_object(existing: &str) -> Result<Value> {
    let value: Value = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(existing).context("not valid JSON")?
    };
    if !value.is_object() {
        anyhow::bail!("does not contain a JSON object");
    }
    Ok(value)
}

/// Walk (creating as needed) to the object `pointer` names.
///
/// A segment that exists but holds something other than an object belongs to a
/// shape we do not understand, so this errors rather than replacing it.
fn descend_mut<'a>(root: &'a mut Value, pointer: &[&str]) -> Result<&'a mut Map<String, Value>> {
    let mut current = root
        .as_object_mut()
        .expect("checked to be an object by parse_object");

    for (depth, segment) in pointer.iter().enumerate() {
        let slot = current
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            anyhow::bail!(
                "{} does not hold a JSON object",
                pointer[..=depth].join(".")
            );
        }
        current = slot
            .as_object_mut()
            .expect("checked to be an object one line above");
    }

    Ok(current)
}

fn render(root: &Value) -> Result<String> {
    let mut text =
        serde_json::to_string_pretty(root).context("failed to serialise the MCP configuration")?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
#[path = "mcp_merge_tests.rs"]
mod tests;
