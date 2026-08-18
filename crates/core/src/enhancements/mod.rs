//! A library of reusable instruction snippets for `CLAUDE.md` / `AGENTS.md`.
//!
//! The intent-recording block (`intents::providers::instructions`) proved the
//! shape: a marker-bounded section, appended idempotently, that a re-install can
//! recognise and refresh. This module generalises it so the sections are not
//! hard-coded Rust constants but plain `.md` files in a user-owned directory —
//! drop a file in, and it appears in the menu with no recompile.
//!
//! Each template declares in its front matter *where* it belongs, so a snippet
//! can sit at the top of the file, just under the first heading, at the end, or
//! anchored to a named comment. The write rules are the same ones the rest of
//! this codebase already trusts: mark our own span, rewrite only between the
//! markers, never touch the file when the section is already current, and back
//! the original up before merging (via [`providers::apply_writes`]).
//!
//! Everything here is pure string and filesystem work with no Tauri dependency,
//! so the whole feature is tested headlessly in `enhancements_tests.rs`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::intents::providers::{self, PlannedWrite};

pub mod runs;

/// Where a template's section is spliced into the target file.
///
/// An anchor that cannot be located never guesses — it falls back to [`End`],
/// the one position that is always valid and always visible.
///
/// [`End`]: Placement::End
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// The very top of the file, before everything.
    Top,
    /// Immediately after the first `#` heading. No heading ⇒ [`Top`].
    ///
    /// [`Top`]: Placement::Top
    AfterFirstHeading,
    /// Appended after everything else.
    End,
    /// Just before the line containing the given anchor text. Absent ⇒ [`End`].
    ///
    /// [`End`]: Placement::End
    BeforeMarker(String),
    /// Just after the line containing the given anchor text. Absent ⇒ [`End`].
    ///
    /// [`End`]: Placement::End
    AfterMarker(String),
}

/// One parsed instruction template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Stable slug: the menu key and the source of the section markers.
    pub id: String,
    /// Human label for the menu.
    pub title: String,
    pub placement: Placement,
    /// Run-once intent (prompts only): declared with `once: true` in front
    /// matter. An instruction ignores it.
    pub once: bool,
    /// The markdown spliced into the target file (front matter stripped).
    pub body: String,
}

/// What the menu shows for one template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnhancementInfo {
    pub id: String,
    pub title: String,
    /// Present in `CLAUDE.md` or `AGENTS.md` in the current workspace.
    pub installed: bool,
}

/// One prompt in the Run Agent submenu.
///
/// Unlike an instruction, a prompt is never written to a file — its `body` is
/// run as an agent — so the whole thing travels to the frontend at once rather
/// than behind an add/remove round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptInfo {
    pub id: String,
    pub title: String,
    /// Declared run-once (`once: true`): the menu records a successful run for
    /// the workspace and confirms before re-running it.
    pub once: bool,
    /// The prompt text to run (front matter stripped).
    pub body: String,
}

/// Opens the section belonging to `id`.
pub fn begin_marker(id: &str) -> String {
    format!("<!-- code-basics: enhancement:{id} -->")
}

/// Closes the section belonging to `id`, bounding what a refresh may rewrite.
pub fn end_marker(id: &str) -> String {
    format!("<!-- /code-basics: enhancement:{id} -->")
}

/// Is `id`'s section already in this text?
pub fn is_present(text: &str, id: &str) -> bool {
    text.contains(&begin_marker(id))
}

/// Parse a template file. `default_id` (normally the file stem) fills in for a
/// missing or absent `id:` key so a bare markdown file is still usable.
pub fn parse_template(text: &str, default_id: &str) -> Template {
    let mut id = default_id.to_string();
    let mut title = None;
    let mut placement_word = None;
    let mut anchor = None;
    let mut once = false;

    let body = if let Some((front, rest)) = split_front_matter(text) {
        for line in front.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "id" if !value.is_empty() => id = value.to_string(),
                "title" => title = Some(value.to_string()),
                "placement" => placement_word = Some(value.to_string()),
                "anchor" => anchor = Some(value.to_string()),
                // Only an explicit truthy value opts in; anything else is not
                // run-once.
                "once" => once = matches!(value.to_ascii_lowercase().as_str(), "true" | "yes"),
                _ => {}
            }
        }
        rest
    } else {
        text.to_string()
    };

    let placement = match placement_word.as_deref() {
        Some("top") => Placement::Top,
        Some("after-first-heading") => Placement::AfterFirstHeading,
        Some("before-marker") => match anchor.filter(|a| !a.is_empty()) {
            Some(a) => Placement::BeforeMarker(a),
            None => Placement::End,
        },
        Some("after-marker") => match anchor.filter(|a| !a.is_empty()) {
            Some(a) => Placement::AfterMarker(a),
            None => Placement::End,
        },
        // "end", "append", anything unrecognised, or no front matter at all.
        _ => Placement::End,
    };

    Template {
        title: title
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.clone()),
        id,
        placement,
        once,
        body,
    }
}

/// Split leading `---`-fenced front matter from the body, if present.
///
/// Returns `(front, body)`. A file that does not open with a `---` fence has no
/// front matter and the whole text is the body.
fn split_front_matter(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim_start_matches(['\u{feff}']);
    let mut lines = trimmed.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }

    let mut front = String::new();
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            let body: String = lines.collect();
            return Some((front, body.trim_start_matches('\n').to_string()));
        }
        front.push_str(line);
    }
    // Opened a fence that never closed: treat the file as bodyless front matter
    // rather than dropping content.
    None
}

/// The template text spliced into `file_text`, marker-wrapped.
///
/// Idempotent: an already-present section is rewritten in place (so a changed
/// template refreshes) and an unchanged one leaves the file byte-identical.
pub fn insert(file_text: &str, template: &Template) -> String {
    let section = section_block(&template.id, &template.body);

    if is_present(file_text, &template.id) {
        return refresh(file_text, &template.id, &section);
    }
    splice(file_text, &section, &template.placement)
}

/// `id`'s marked section removed, blank lines around the gap normalised. `None`
/// when the section was not there (nothing to do) or its end marker is missing
/// (unclosed span — left alone rather than guessed at).
pub fn remove(file_text: &str, id: &str) -> Option<String> {
    let (nl, had_trailing, lines) = split_lines(file_text);
    let begin = begin_marker(id);
    let end = end_marker(id);

    let b = lines.iter().position(|l| l.contains(&begin))?;
    let e = lines[b..]
        .iter()
        .position(|l| l.contains(&end))
        .map(|i| b + i)?;

    let mut out: Vec<String> = lines[..b].to_vec();
    let prefix_blank = out.last().is_none_or(|l| l.trim().is_empty());
    let mut si = e + 1;
    if prefix_blank {
        while si < lines.len() && lines[si].trim().is_empty() {
            si += 1;
        }
    }
    out.extend_from_slice(&lines[si..]);
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }

    Some(join_lines(&out, nl, had_trailing))
}

/// The wrapped section: begin marker, trimmed body, end marker.
fn section_block(id: &str, body: &str) -> String {
    format!("{}\n{}\n{}", begin_marker(id), body.trim(), end_marker(id))
}

/// Replace `id`'s existing span with `section`. A missing end marker bounds
/// nothing, so the file is returned untouched.
fn refresh(file_text: &str, id: &str, section: &str) -> String {
    let (nl, had_trailing, lines) = split_lines(file_text);
    let begin = begin_marker(id);
    let end = end_marker(id);

    let Some(b) = lines.iter().position(|l| l.contains(&begin)) else {
        return file_text.to_string();
    };
    let Some(e) = lines[b..]
        .iter()
        .position(|l| l.contains(&end))
        .map(|i| b + i)
    else {
        return file_text.to_string();
    };

    let mut out: Vec<String> = lines[..b].to_vec();
    out.extend(section.split('\n').map(str::to_string));
    out.extend_from_slice(&lines[e + 1..]);
    join_lines(&out, nl, had_trailing)
}

/// Insert `section` at the resolved anchor with one blank line either side.
fn splice(file_text: &str, section: &str, placement: &Placement) -> String {
    let (nl, had_trailing, lines) = split_lines(file_text);
    let idx = anchor_index(&lines, placement);

    let mut prefix: Vec<String> = lines[..idx].to_vec();
    while prefix.last().is_some_and(|l| l.trim().is_empty()) {
        prefix.pop();
    }
    let mut start = idx;
    while start < lines.len() && lines[start].trim().is_empty() {
        start += 1;
    }
    let suffix = &lines[start..];

    let mut out = prefix;
    if !out.is_empty() {
        out.push(String::new());
    }
    out.extend(section.split('\n').map(str::to_string));
    if !suffix.is_empty() {
        out.push(String::new());
        out.extend_from_slice(suffix);
    }

    join_lines(&out, nl, had_trailing)
}

/// The line index the section is inserted at, per placement (with fallbacks).
fn anchor_index(lines: &[String], placement: &Placement) -> usize {
    match placement {
        Placement::Top => 0,
        Placement::End => lines.len(),
        Placement::AfterFirstHeading => lines
            .iter()
            .position(|l| l.trim_start().starts_with('#'))
            .map_or(0, |i| i + 1),
        Placement::BeforeMarker(anchor) => lines
            .iter()
            .position(|l| l.contains(anchor.as_str()))
            .unwrap_or(lines.len()),
        Placement::AfterMarker(anchor) => lines
            .iter()
            .position(|l| l.contains(anchor.as_str()))
            .map_or(lines.len(), |i| i + 1),
    }
}

/// Split into logical lines, remembering the newline style and whether the file
/// ended with one, so [`join_lines`] can reproduce both.
fn split_lines(text: &str) -> (&'static str, bool, Vec<String>) {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let had_trailing = text.ends_with('\n');
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    // `split('\n')` on a newline-terminated string yields a trailing empty
    // element; `had_trailing` remembers it, so drop it here.
    if had_trailing {
        lines.pop();
    }
    (nl, had_trailing, lines)
}

fn join_lines(lines: &[String], nl: &str, had_trailing: bool) -> String {
    let mut result = lines.join(nl);
    if had_trailing && !result.is_empty() {
        result.push_str(nl);
    }
    result
}

/// The two agent instruction files, in the workspace root.
fn agent_files(root: &Path) -> [PathBuf; 2] {
    [root.join("CLAUDE.md"), root.join("AGENTS.md")]
}

/// Parse every `*.md` in `dir`, skipping ones that will not read, sorted by
/// title (case-insensitive) and de-duplicated by id.
pub fn discover(dir: &Path) -> Vec<Template> {
    let mut templates = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return templates;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("instruction");
        if let Ok(text) = std::fs::read_to_string(&path) {
            templates.push(parse_template(&text, stem));
        }
    }

    templates.sort_by_key(|t| t.title.to_lowercase());
    templates.dedup_by(|a, b| a.id == b.id);
    templates
}

/// Ensure `dir` exists and holds a copy of every bundled default that is not
/// already there. Existing files — which the user may have edited — are never
/// overwritten.
pub fn seed(dir: &Path, bundled: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let Some(bundled) = bundled.filter(|b| b.is_dir()) else {
        return Ok(());
    };

    for entry in std::fs::read_dir(bundled)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(name) = path.file_name() {
            let target = dir.join(name);
            if !target.exists() {
                std::fs::copy(&path, &target)?;
            }
        }
    }
    Ok(())
}

/// The menu listing: every template, flagged with whether it is installed in
/// this workspace.
pub fn list(dir: &Path, root: &Path) -> Vec<EnhancementInfo> {
    let files: Vec<String> = agent_files(root)
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();

    discover(dir)
        .into_iter()
        .map(|t| EnhancementInfo {
            installed: files.iter().any(|text| is_present(text, &t.id)),
            id: t.id,
            title: t.title,
        })
        .collect()
}

/// Every prompt found on disk, sorted by title, ready to run.
///
/// A prompt reuses the same template file format as an instruction — the
/// placement and marker machinery is simply ignored; all that is kept is the
/// title, the body, and the run-once flag.
pub fn list_prompts(dir: &Path) -> Vec<PromptInfo> {
    discover(dir)
        .into_iter()
        .map(|t| PromptInfo {
            id: t.id,
            title: t.title,
            once: t.once,
            body: t.body,
        })
        .collect()
}

/// Add the template `id` to both agent files, creating them if absent.
pub fn add(root: &Path, dir: &Path, id: &str) -> Result<()> {
    let template = discover(dir)
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| anyhow::anyhow!("no instruction template named {id}"))?;

    let mut writes = Vec::new();
    for path in agent_files(root) {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let content = insert(&existing, &template);
        let merges_existing = path.exists();
        writes.push(PlannedWrite {
            path,
            content,
            merges_existing,
        });
    }

    providers::apply_writes(&writes)
}

/// Remove the template `id` from both agent files. Returns how many files
/// actually changed, so a caller can tell nothing-there from done.
pub fn remove_from_agents(root: &Path, id: &str) -> Result<usize> {
    let mut writes = Vec::new();
    for path in agent_files(root) {
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(content) = remove(&existing, id) {
            writes.push(PlannedWrite {
                path,
                content,
                merges_existing: true,
            });
        }
    }

    let changed = writes.len();
    providers::apply_writes(&writes)?;
    Ok(changed)
}

/// The user-owned instruction-templates directory.
///
/// `CB_INSTRUCTIONS_PATH` overrides it wholesale (used under `pnpm tauri dev`,
/// where the bundled resource directory does not resolve). Otherwise it is
/// `<config>/code-basics/instructions`, where `<config>` is `%APPDATA%` on
/// Windows and `$XDG_CONFIG_HOME` / `~/.config` elsewhere.
pub fn templates_dir() -> PathBuf {
    library_dir("CB_INSTRUCTIONS_PATH", "instructions")
}

/// The user-owned prompts directory, a sibling of [`templates_dir`].
///
/// `CB_PROMPTS_PATH` overrides it wholesale for development.
pub fn prompts_dir() -> PathBuf {
    library_dir("CB_PROMPTS_PATH", "prompts")
}

/// A `code-basics/<name>` directory under the user's config location, with an
/// env override for development.
fn library_dir(env_override: &str, name: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(env_override) {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(name)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

#[cfg(test)]
#[path = "enhancements_tests.rs"]
mod tests;
