//! Adding one `[mcp_servers.<name>]` entry to `$CODEX_HOME/config.toml`,
//! format-preservingly.
//!
//! # Why `toml_edit` and not a `toml::Value` round-trip
//!
//! A round-trip is a **whole-file rewrite of a file this app does not own**,
//! and the losses are not merely cosmetic. Measured against the real file on a
//! development machine:
//!
//! * It carries single-quoted *literal* keys holding Windows paths and colons —
//!   `[projects.'c:\users\...\stonks']`, `[hooks.state.'C:\...\hooks.json:stop:1:0']`.
//!   A serializer re-quotes those in whatever spelling it prefers, and the two
//!   readers that match on them ([`crate::intents::providers::codex::is_trusted_in`]
//!   and Codex itself) then disagree with the file.
//! * It carries bare key-values (`model`, `approval_policy`) which TOML requires
//!   to appear *before* any table; a naive rebuild can reorder them into a file
//!   that no longer parses.
//! * Codex rewrites the file while it runs, so every byte this app changes that
//!   it did not have to change is a byte to lose a race over.
//! * Comments are the user's, and a config tool that eats them is one the user
//!   stops trusting.
//!
//! [`toml_edit`] edits the parsed document in place and re-emits everything it
//! did not touch verbatim. `Cargo.lock` already carried it transitively, so
//! naming it directly costs zero packages.
//!
//! # Why `codex::is_trusted_in` keeps text-scanning, and should
//!
//! That function answers *yes or no* about a path whose separators and case
//! vary, and it must survive a file it cannot parse — a tolerant **read**. This
//! module performs a **write**, where a byte lost is a byte the user cannot get
//! back. Those are different jobs with opposite failure modes (a read should
//! guess low and carry on; a write should refuse and change nothing), so they
//! deliberately do not share an implementation. Do not "unify" them.

use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// The table Codex reads MCP servers from.
const SERVERS_TABLE: &str = "mcp_servers";

/// Compute the file's contents after adding the entry, without writing.
/// Returns the new text and whether an existing file is being merged into.
pub fn merged_text(
    path: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(String, bool)> {
    // See `super::read_existing`. An unreadable existing file must not be
    // planned as a fresh one: this module's whole argument for `toml_edit` is
    // that Codex owns this file and re-emitting a byte it did not have to
    // re-emit is a byte to lose a race over — and merging into `""` re-emits
    // every byte, because there is nothing parsed left to preserve.
    let existing = super::read_existing(path)?;
    let merges_existing = existing.is_some();
    let text = merged_into_text(existing.as_deref().unwrap_or(""), name, command, args)
        .with_context(|| format!("{} was left untouched", path.display()))?;
    Ok((text, merges_existing))
}

/// Add (or refresh) `[mcp_servers.<name>]` in a config **string**.
///
/// Bails without producing output if the text does not parse, or if
/// `mcp_servers` is present as something other than a table — either is a shape
/// this module does not understand, and rewriting it would destroy it.
pub fn merged_into_text(
    existing: &str,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<String> {
    let mut doc = parse(existing)?;
    let servers = servers_table_mut(&mut doc)?;

    let mut entry = Table::new();
    entry["command"] = value(command);
    let mut array = Array::new();
    for arg in args {
        array.push(arg.as_str());
    }
    entry["args"] = value(array);

    // Keyed on the name, so a re-install refreshes in place rather than
    // appending a second entry for the same server.
    servers.insert(name, Item::Table(entry));

    Ok(doc.to_string())
}

/// Remove our entry again. `None` when there is nothing of ours there, so the
/// caller plans **zero** writes rather than rewriting an unchanged file.
pub fn removal_text(existing: &str, name: &str) -> Result<Option<String>> {
    let mut doc = parse(existing)?;
    if !installed_in(&doc, name) {
        return Ok(None);
    }

    let servers = servers_table_mut(&mut doc)?;
    servers.remove(name);
    Ok(Some(doc.to_string()))
}

/// Remove our entry from the file at `path`. `None` when the file is absent or
/// holds no entry of ours.
/// An **unreadable** file is an error rather than `None`, for the reason given
/// on `mcp_merge::plan_removal`: `None` is rendered as *nothing to remove*, and
/// that is a claim this function cannot make about a file it could not read.
pub fn plan_removal(path: &Path, name: &str) -> Result<Option<String>> {
    let Some(text) = super::read_existing(path)? else {
        return Ok(None);
    };
    removal_text(&text, name).with_context(|| format!("{} was left untouched", path.display()))
}

/// Is `name` configured in this text? Unparseable content answers **no** rather
/// than erroring: a status read is a question, not a write.
pub fn is_installed_in(existing: &str, name: &str) -> bool {
    parse(existing).is_ok_and(|doc| installed_in(&doc, name))
}

/// Is `name` configured in the file at `path`?
///
/// `false` for absent or unparseable; see [`installed_state`] for the version an
/// installer must consult.
pub fn is_installed(path: &Path, name: &str) -> bool {
    installed_state(path, name).unwrap_or(false)
}

/// Three answers: installed, definitely not, or **`None` — unreadable, so there
/// is no answer.** The same rule as `mcp_merge::installed_state`, and it matters
/// for the same reason: a status of "not installed" invites an install, and an
/// install into a file that could not be read is what destroys it.
pub fn installed_state(path: &Path, name: &str) -> Option<bool> {
    match super::read_existing(path) {
        Err(_) => None,
        Ok(None) => Some(false),
        Ok(Some(text)) => Some(is_installed_in(&text, name)),
    }
}

fn parse(existing: &str) -> Result<DocumentMut> {
    existing.parse::<DocumentMut>().context("not valid TOML")
}

fn installed_in(doc: &DocumentMut, name: &str) -> bool {
    doc.get(SERVERS_TABLE)
        .and_then(Item::as_table_like)
        .is_some_and(|servers| servers.get(name).is_some())
}

/// The `mcp_servers` table, created (implicitly, so it renders as
/// `[mcp_servers.<name>]` with no empty parent header) when absent.
fn servers_table_mut(doc: &mut DocumentMut) -> Result<&mut Table> {
    if let Some(existing) = doc.get(SERVERS_TABLE) {
        if existing.as_table().is_none() {
            anyhow::bail!("{SERVERS_TABLE} is present but is not a table");
        }
    }

    doc.as_table_mut()
        .entry(SERVERS_TABLE)
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()
        .context("mcp_servers is not a table")
}

#[cfg(test)]
#[path = "codex_toml_tests.rs"]
mod tests;
