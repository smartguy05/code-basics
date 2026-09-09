//! Applying a server's [`TextEdit`]s to a document, or refusing to.
//!
//! This is the piece with no precedent in the tree. `git/repo.rs` applies a
//! *unified diff* to one file by shelling out to `git apply`; nothing here has
//! ever taken a set of ranges and turned them into text. It is also the piece
//! where a mistake is worst: find-usages getting an offset wrong shows the
//! reader the wrong line, and a rename getting one wrong writes text nobody
//! asked for into a file that may not be open.
//!
//! # Two functions, and the split is the point
//!
//! [`plan`] takes **no text**. Whether two edits overlap, whether a range runs
//! backwards, and whether two insertions collide are all positional facts, so
//! they can be settled before a single file is read — which is what lets a
//! whole multi-file rename be refused having touched nothing. [`apply`] then
//! does only the arithmetic, on a set already proved well-formed.
//!
//! # Refusing rather than choosing
//!
//! Every error here is a case where two answers are equally defensible and
//! picking one silently produces a file the server did not describe:
//!
//! - **Overlapping ranges.** Applying them in either order gives different
//!   text, and neither is what was asked for.
//! - **Two zero-width insertions at one position.** The answer contains no
//!   statement about which goes first.
//! - **A range the document does not contain.** `positions::byte_offset` would
//!   cheerfully clamp line 40 of a three-line file to the end and apply the
//!   edit *somewhere plausible*. That is the stale-mirror bug in its most
//!   damaging form, so it is the one place this module does not clamp: a server
//!   naming a line we do not have is describing a different version of the
//!   file, and the only safe answer is to stop.
//!
//! An exact duplicate is the deliberate exception. Same range *and* same text
//! means deduplicating changes nothing about the result, whereas applying it
//! twice would insert the replacement twice — so that one is collapsed rather
//! than refused. Two different texts for one range is not a duplicate; it is a
//! disagreement, and it is refused as an overlap.
//!
//! # Forward with a cursor
//!
//! [`apply`] walks the document once, copying up to each edit's start and
//! resuming after its end. The usual trick — apply in reverse so earlier
//! offsets stay valid — is shorter and hides the end-of-document case, which
//! here falls out of the loop's tail with nothing special written for it.

use thiserror::Error;

use super::positions::byte_offset;
use super::protocol::{Position, Range, TextEdit};

/// Why a set of edits was refused.
///
/// Never a partially applied file: every one of these is decided before any
/// text is written, and [`apply`] returns the original document untouched.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EditError {
    #[error(
        "two edits overlap — {} and {} — and applying them in either order would \
         produce text the language server did not ask for",
        describe(first),
        describe(second)
    )]
    Overlapping { first: Range, second: Range },

    #[error("an edit's range ends before it starts ({})", describe(range))]
    Backwards { range: Range },

    #[error(
        "an edit names line {}, but this file has {lines}; the language server is \
         describing a different version of it",
        line + 1
    )]
    OutOfDocument { line: u32, lines: u32 },

    #[error(
        "two separate insertions at {} have no defined order",
        describe_position(at)
    )]
    AmbiguousInsertion { at: Position },
}

/// A range as a person reads it: 1-based lines, matching an editor gutter.
///
/// The wire is 0-based and the message is not, because the reader of the
/// message is looking at a gutter and not at a protocol.
fn describe(range: &Range) -> String {
    if range.start.line == range.end.line {
        format!(
            "line {}, columns {}-{}",
            range.start.line + 1,
            range.start.character,
            range.end.character
        )
    } else {
        format!(
            "line {} column {} to line {} column {}",
            range.start.line + 1,
            range.start.character,
            range.end.line + 1,
            range.end.character
        )
    }
}

fn describe_position(at: &Position) -> String {
    format!("line {}, column {}", at.line + 1, at.character)
}

/// Sort, deduplicate and prove a set of edits applicable — without any text.
///
/// Text-free deliberately: an overlap is a positional fact, so the whole rename
/// can be refused before a file is read or written. The returned order is
/// document order and does not depend on the input's, so two servers that
/// answer the same edits in different orders produce byte-identical results.
pub fn plan(edits: &[TextEdit]) -> Result<Vec<TextEdit>, EditError> {
    for edit in edits {
        if key(&edit.range.end) < key(&edit.range.start) {
            return Err(EditError::Backwards { range: edit.range });
        }
    }

    let mut sorted: Vec<TextEdit> = edits.to_vec();
    // By start, then end, then text. Sorting on the text too is what makes the
    // duplicate check below a simple comparison with the previous element
    // rather than a search.
    sorted.sort_by(|a, b| {
        key(&a.range.start)
            .cmp(&key(&b.range.start))
            .then_with(|| key(&a.range.end).cmp(&key(&b.range.end)))
            .then_with(|| a.new_text.cmp(&b.new_text))
    });

    let mut planned: Vec<TextEdit> = Vec::with_capacity(sorted.len());
    for edit in sorted {
        let Some(previous) = planned.last() else {
            planned.push(edit);
            continue;
        };

        if previous.range == edit.range {
            if previous.new_text == edit.new_text {
                // The same edit twice. Collapsing it changes nothing; applying
                // it twice would write the replacement twice.
                continue;
            }
            // Two zero-width edits at one position are checked *before* the
            // equal-range case below, because both are true of them and only
            // this one names the actual problem: the answer contains no
            // statement about which insertion goes first, which is a different
            // complaint from two replacements disagreeing about one span.
            if edit.range.start == edit.range.end {
                return Err(EditError::AmbiguousInsertion {
                    at: edit.range.start,
                });
            }
            // One span, two different replacements. Not a duplicate — a
            // disagreement, with no basis for preferring either.
            return Err(EditError::Overlapping {
                first: previous.range,
                second: edit.range,
            });
        }

        // `end` is exclusive, so touching is adjacency and not overlap.
        if key(&edit.range.start) < key(&previous.range.end) {
            return Err(EditError::Overlapping {
                first: previous.range,
                second: edit.range,
            });
        }

        planned.push(edit);
    }

    Ok(planned)
}

/// Apply a planned set to a document.
///
/// Refuses — leaving the caller's text untouched — if any range names a line the
/// document does not have. See the module docs for why that one case is not
/// clamped when everything in `positions` is.
pub fn apply(text: &str, planned: &[TextEdit]) -> Result<String, EditError> {
    if planned.is_empty() {
        return Ok(text.to_string());
    }

    let lines = line_count(text);
    for edit in planned {
        for line in [edit.range.start.line, edit.range.end.line] {
            if line >= lines {
                return Err(EditError::OutOfDocument { line, lines });
            }
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for edit in planned {
        let start = offset_of(text, &edit.range.start);
        let end = offset_of(text, &edit.range.end);
        // `plan` proved the set ordered and non-overlapping, so `start` cannot
        // precede the cursor — but `max` costs nothing and a slice panicking
        // inside a command is what the user experiences as the app breaking.
        let start = start.max(cursor);
        let end = end.max(start);
        out.push_str(&text[cursor..start]);
        out.push_str(&edit.new_text);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Ok(out)
}

/// What each planned edit is about to overwrite.
///
/// The evidence behind the stale-mirror check: if these are not the identifier
/// being renamed, the server computed its ranges from text this app does not
/// have. Diagnostic rather than authoritative, so unlike [`apply`] it reports
/// what it can for a range past the end instead of refusing — a caller asking
/// "what is about to be replaced?" is owed an answer even when the answer is
/// that the ranges make no sense.
pub fn replaced_texts(text: &str, planned: &[TextEdit]) -> Vec<String> {
    planned
        .iter()
        .map(|edit| {
            let start = offset_of(text, &edit.range.start);
            let end = offset_of(text, &edit.range.end).max(start);
            text[start..end].to_string()
        })
        .collect()
}

/// How many lines LSP would say this document has.
///
/// A trailing newline opens a final empty line, and that line is addressable:
/// an insertion there is how a file gets appended to. So `"a\n"` is two lines,
/// and the empty document is one.
fn line_count(text: &str) -> u32 {
    let mut lines = 1u32;
    let bytes = text.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                lines += 1;
                at += 1;
            }
            b'\r' => {
                lines += 1;
                at += if bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
            }
            _ => at += 1,
        }
    }
    lines
}

fn offset_of(text: &str, position: &Position) -> usize {
    byte_offset(text, position.line, position.character)
}

/// A position as one comparable value.
///
/// `character` is a `u32`, so a `(line, character)` tuple orders correctly
/// without arithmetic that could overflow on the `u32::MAX` a malformed answer
/// carries.
fn key(position: &Position) -> (u32, u32) {
    (position.line, position.character)
}

#[cfg(test)]
#[path = "edits_tests.rs"]
mod edits_tests;
