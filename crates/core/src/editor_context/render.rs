//! Turning the pushed editor state into the words an agent reads.
//!
//! The **twin** of [`crate::roslyn::render`]. The application holds a
//! [`crate::model::EditorContext`] pushed from the frontend and hands it here;
//! this module — pure, in the core crate — turns it into a
//! [`super::wire::ToolAnswer`]. The runner and the app-side dispatch decide
//! nothing about the prose.
//!
//! # The rule that shapes every function here
//!
//! **An answer must never let an absence pass for a fact about the editor, and
//! must never fabricate a path or a position.** So:
//!
//! * No active file is [`super::answer::EditorRefusal::NoActiveFile`] — a
//!   refusal with its own code — for the two tools that report the focused file.
//! * An empty selection, no open tabs and no recent files are **genuine data**:
//!   stated plainly as the complete answer, not as a refusal, the way a
//!   `Ready`-but-empty Roslyn result is data.
//! * A cursor or viewport that was not reported is stated as *not reported*
//!   rather than invented.
//!
//! The three pre-render refusals (no workspace, feature off, nothing pushed yet)
//! are produced by the application layer before an [`crate::model::EditorContext`]
//! exists to hand here; they live in [`super::answer`].

use crate::model::{EditorContext, EditorOpenFile, EditorRecentFile, EditorSelection};

use super::answer::EditorRefusal;
use super::wire::ToolAnswer;

/// The one-line reminder appended wherever a position is reported.
const POSITION_NOTE: &str = "(Positions are 1-based line, 0-based UTF-16 character.)";

/// The `get_active_file` answer.
pub fn get_active_file(context: &EditorContext) -> ToolAnswer {
    let Some(path) = context.active_file.as_deref() else {
        return EditorRefusal::NoActiveFile.answer();
    };

    let mut lines = vec![format!("Active file: {path}")];

    match &context.cursor {
        Some(cursor) => lines.push(format!(
            "Cursor: line {}, character {}",
            cursor.line, cursor.character
        )),
        None => lines.push("Cursor: not reported.".to_string()),
    }

    match &context.viewport {
        Some(viewport) => lines.push(format!(
            "Visible lines: {}-{}",
            viewport.first_visible_line, viewport.last_visible_line
        )),
        None => lines.push("Visible lines: not reported.".to_string()),
    }

    // dirty / pinned live on the matching open-tab entry; abstain if this file is
    // somehow not among the open tabs rather than inventing a state for it.
    match context.open_files.iter().find(|file| file.path == path) {
        Some(entry) => {
            lines.push(format!(
                "Unsaved changes: {}",
                if entry.dirty { "yes" } else { "no" }
            ));
            lines.push(format!("Pinned: {}", if entry.pinned { "yes" } else { "no" }));
        }
        None => lines.push(
            "This file is not among the reported open tabs, so its unsaved/pinned state is unknown."
                .to_string(),
        ),
    }

    lines.push(POSITION_NOTE.to_string());
    ToolAnswer::ok(lines.join("\n"))
}

/// The `get_selection` answer.
pub fn get_selection(context: &EditorContext) -> ToolAnswer {
    let Some(path) = context.active_file.as_deref() else {
        return EditorRefusal::NoActiveFile.answer();
    };

    let Some(selection) = &context.selection else {
        // A genuine answer: there is a file, nothing is selected. Not a refusal.
        return ToolAnswer::ok(format!(
            "No text is selected in {path}. This is the complete answer, not a truncated one."
        ));
    };

    ToolAnswer::ok(format!(
        "Selection in {path}: from line {}, character {} to line {}, character {}\n{} \
         character(s) selected:\n{}\n{POSITION_NOTE}",
        selection.start_line,
        selection.start_character,
        selection.end_line,
        selection.end_character,
        selection_length(selection),
        selection.text,
    ))
}

/// The number of characters in a selection's text (Unicode scalar values).
fn selection_length(selection: &EditorSelection) -> usize {
    selection.text.chars().count()
}

/// The `get_open_files` answer.
pub fn get_open_files(context: &EditorContext) -> ToolAnswer {
    if context.open_files.is_empty() {
        return ToolAnswer::ok(
            "No files are open in the editor. This is the complete answer, not a truncated one."
                .to_string(),
        );
    }

    let mut lines = vec![format!("{} open file(s):", context.open_files.len())];
    for file in &context.open_files {
        lines.push(open_file_line(file));
    }
    ToolAnswer::ok(lines.join("\n"))
}

fn open_file_line(file: &EditorOpenFile) -> String {
    let mut flags = Vec::new();
    if file.active {
        flags.push("active");
    }
    if file.dirty {
        flags.push("unsaved");
    }
    if file.pinned {
        flags.push("pinned");
    }
    if flags.is_empty() {
        format!("  {}", file.path)
    } else {
        format!("  {}  ({})", file.path, flags.join(", "))
    }
}

/// The `get_recent_files` answer.
pub fn get_recent_files(context: &EditorContext) -> ToolAnswer {
    if context.recent_files.is_empty() {
        return ToolAnswer::ok(
            "No files have been edited recently. This is the complete answer, not a truncated one."
                .to_string(),
        );
    }

    let mut lines = vec![format!(
        "{} recently edited file(s), most recent first:",
        context.recent_files.len()
    )];
    for file in &context.recent_files {
        lines.push(recent_file_line(file));
    }
    ToolAnswer::ok(lines.join("\n"))
}

fn recent_file_line(file: &EditorRecentFile) -> String {
    format!("  {}", file.path)
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
