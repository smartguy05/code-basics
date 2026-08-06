//! Reading Codex's patch format.
//!
//! Where Claude Code reports an edit as the string it replaced and the string
//! it wrote, Codex reports a *patch*: one envelope describing any number of
//! files at once.
//!
//! ```text
//! *** Begin Patch
//! *** Update File: src/lib.rs
//! @@ fn main
//!  unchanged context
//! -removed line
//! +added line
//! *** Add File: src/new.rs
//! +every line of a new file is an addition
//! *** End Patch
//! ```
//!
//! Three things make this harder than it looks, and all three are observed
//! behaviour rather than hypothetical:
//!
//! 1. **The payload is not always JSON.** Codex advertises `apply_patch` as a
//!    "freeform" tool for some models, in which case the hook reports the raw
//!    patch text instead of an object with fields.
//! 2. **`apply_patch` is not always its own tool.** It is also invoked through
//!    the shell (`apply_patch 'PATCH'`), so a shell tool call has to be
//!    examined rather than skipped.
//! 3. **A context line can be empty.** A blank context line is written as an
//!    empty line, not `" "`, so the leading-character rule has to treat "no
//!    character at all" as context. Getting this wrong silently shifts every
//!    subsequent line into the wrong bucket.
//!
//! Parsing is deliberately lenient: this is evidence for labelling a diff, not
//! something being applied. A section that cannot be understood is skipped and
//! the rest of the envelope is still used.

use super::IntentEdit;

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const UPDATE: &str = "*** Update File:";
const ADD: &str = "*** Add File:";
const DELETE: &str = "*** Delete File:";
const MOVE: &str = "*** Move to:";

/// One file's worth of change, lifted out of an envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchedFile {
    /// The path the patch names, as written.
    pub path: String,
    /// Where the file was moved to, when the section said so.
    pub moved_to: Option<String>,
    pub edit: IntentEdit,
}

/// Pull a patch envelope out of whatever the hook recorded.
///
/// Accepts the raw text itself, or text with surrounding noise (a shell
/// command line, a JSON string that has already been unescaped). Returns an
/// empty vector when there is no envelope, which is the common case for the
/// many tool calls that are not edits.
pub fn parse_envelope(text: &str) -> Vec<PatchedFile> {
    let Some(body) = envelope_body(text) else {
        return Vec::new();
    };

    let mut files: Vec<PatchedFile> = Vec::new();
    let mut current: Option<PatchedFile> = None;

    for line in body.lines() {
        let trimmed = line.trim_end_matches('\r');

        if let Some(path) = section_header(trimmed, UPDATE) {
            push(&mut files, current.take());
            current = Some(new_file(path, false));
            continue;
        }
        if let Some(path) = section_header(trimmed, ADD) {
            push(&mut files, current.take());
            let mut file = new_file(path, true);
            file.edit.whole_file = true;
            current = Some(file);
            continue;
        }
        if let Some(path) = section_header(trimmed, DELETE) {
            push(&mut files, current.take());
            let mut file = new_file(path, false);
            file.edit.whole_file = true;
            current = Some(file);
            continue;
        }
        if let Some(target) = section_header(trimmed, MOVE) {
            if let Some(file) = current.as_mut() {
                file.moved_to = Some(target.to_string());
            }
            continue;
        }

        let Some(file) = current.as_mut() else {
            // Content before any section header belongs to no file.
            continue;
        };

        // `@@` hunk markers carry a symbol hint, not content.
        if trimmed.starts_with("@@") {
            continue;
        }

        match trimmed.chars().next() {
            Some('+') => file.edit.new_lines.push(trimmed[1..].to_string()),
            Some('-') => file.edit.old_lines.push(trimmed[1..].to_string()),
            // A context line, which is evidence of neither side. An entirely
            // empty line is context too — see the module note.
            _ => {}
        }
    }

    push(&mut files, current.take());
    files
}

fn new_file(path: &str, _is_add: bool) -> PatchedFile {
    PatchedFile {
        path: path.to_string(),
        moved_to: None,
        edit: IntentEdit {
            old_lines: Vec::new(),
            new_lines: Vec::new(),
            whole_file: false,
        },
    }
}

fn push(files: &mut Vec<PatchedFile>, file: Option<PatchedFile>) {
    if let Some(file) = file {
        // A section naming a file but changing nothing is not evidence.
        if !file.edit.is_empty() {
            files.push(file);
        }
    }
}

/// The text between the envelope markers.
///
/// A missing `*** End Patch` is tolerated — a truncated payload still tells us
/// about the files it did name.
fn envelope_body(text: &str) -> Option<&str> {
    let start = text.find(BEGIN)? + BEGIN.len();
    let rest = &text[start..];

    Some(match rest.find(END) {
        Some(end) => &rest[..end],
        None => rest,
    })
}

fn section_header<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(marker)?;
    let path = rest.trim();
    (!path.is_empty()).then_some(path)
}

/// Split a plain unified diff into the lines it removed and added.
///
/// Codex's session files record an update as a `unified_diff` string rather
/// than as an envelope section, so this is the second shape the same
/// information arrives in. Only the leading marker matters here — hunk
/// headers, file headers and the `\ No newline` marker are all skipped, and
/// no attempt is made to track line numbers, which the caller discards anyway.
pub fn parse_unified_diff(diff: &str) -> IntentEdit {
    let mut edit = IntentEdit {
        old_lines: Vec::new(),
        new_lines: Vec::new(),
        whole_file: false,
    };

    for line in diff.lines() {
        let line = line.trim_end_matches('\r');

        // File headers look like additions and deletions but are neither.
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
            continue;
        }
        if line.starts_with('\\') {
            continue;
        }

        match line.chars().next() {
            Some('+') => edit.new_lines.push(line[1..].to_string()),
            Some('-') => edit.old_lines.push(line[1..].to_string()),
            _ => {}
        }
    }

    edit
}

/// Find a patch envelope inside a `tool_input` value of unknown shape.
///
/// Codex hands this over in at least three ways depending on the model and on
/// whether `apply_patch` ran as a tool or through the shell, so rather than
/// guessing which one applies, every string in the value is examined and the
/// first containing an envelope wins.
pub fn envelope_from_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => text.contains(BEGIN).then(|| text.clone()),
        serde_json::Value::Array(items) => items.iter().find_map(envelope_from_value),
        serde_json::Value::Object(fields) => {
            // Prefer the conventional carriers, so a payload that happens to
            // quote a patch elsewhere cannot outrank the real one.
            for key in ["input", "patch", "content", "command", "argv"] {
                if let Some(found) = fields.get(key).and_then(envelope_from_value) {
                    return Some(found);
                }
            }
            fields.values().find_map(envelope_from_value)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "patchfmt_tests.rs"]
mod tests;
