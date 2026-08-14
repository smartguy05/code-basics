//! Counting characters and lines the way LSP does, and the way this app does.
//!
//! Two conversions live here, and both are places an off-by-one is invisible
//! until it puts the cursor somewhere the user did not ask for.
//!
//! **Characters.** An LSP `character` is a count of **UTF-16 code units**, not
//! bytes and not `char`s. Rust strings are UTF-8, so the three disagree on any
//! line containing non-ASCII: `é` is 2 bytes and 1 unit, `€` is 3 and 1, and an
//! emoji is 4 bytes and **2** units. Byte offsets are what Rust needs to slice a
//! line; UTF-16 offsets are what the protocol speaks. The conversion is free on
//! the frontend, incidentally — CodeMirror document offsets already are UTF-16
//! code units — which is why the client offers `utf-16` and refuses a server
//! that insists on anything else, rather than negotiating.
//!
//! **Lines.** LSP counts from zero; every line number in this app is 1-based,
//! matching an editor gutter, `SymbolIndex::line` and the whole existing
//! open-a-file-at-a-line chain. The conversion exists in exactly one place so
//! there is one thing to get right.
//!
//! # Clamping rather than panicking
//!
//! A `character` past the end of its line is not a malformed message: a range
//! whose end is the start of the next line is a common spelling for "to end of
//! line", and servers do send positions against a buffer version they have and
//! we have already changed. Slicing on one would panic *inside a command*, which
//! the user experiences as the app breaking. Everything here clamps, which is the
//! same conclusion `lineToPos` reached on the frontend.

/// The widest snippet a usage row will carry.
///
/// A minified bundle is one line of half a megabyte, and the row exists to show
/// the reader the use site, not the file.
pub const SNIPPET_MAX_CHARS: usize = 200;

/// How much of a long line to keep before the match, so it has context.
const SNIPPET_LEAD_CHARS: usize = 60;

/// The byte offset of a UTF-16 code-unit offset within one line.
///
/// An offset landing halfway through a surrogate pair clamps back to the start
/// of that character: there is no byte boundary there, and the alternative is a
/// panic.
pub fn utf16_to_byte(line: &str, utf16: u32) -> usize {
    let wanted = utf16 as usize;
    let mut units = 0usize;
    for (offset, character) in line.char_indices() {
        if units >= wanted {
            return offset;
        }
        let next = units + character.len_utf16();
        if next > wanted {
            // The offset falls *inside* this character — the second half of a
            // surrogate pair. There is no byte boundary there, so clamp back to
            // where the character starts.
            return offset;
        }
        units = next;
    }
    line.len()
}

/// The UTF-16 code-unit offset of a byte offset within one line.
///
/// A byte offset inside a character clamps to that character's own offset, for
/// the same reason: nothing should produce one, and a clamp is cheaper than a
/// panic reaching the UI.
pub fn byte_to_utf16(line: &str, byte: usize) -> u32 {
    let mut units = 0usize;
    for (offset, character) in line.char_indices() {
        if offset >= byte || offset + character.len_utf8() > byte {
            break;
        }
        units += character.len_utf16();
    }
    units as u32
}

/// A 0-based LSP line as this app's 1-based line.
pub fn to_editor_line(lsp: u32) -> u32 {
    lsp.saturating_add(1)
}

/// A 1-based line as LSP's 0-based one.
///
/// A 1-based line of zero is not a line; saturating rather than wrapping means
/// it asks about the first line instead of one four billion down.
pub fn to_lsp_line(editor: u32) -> u32 {
    editor.saturating_sub(1)
}

/// One line of context for a usage row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    /// Byte offsets into `text`, for underlining the match.
    ///
    /// Byte offsets because everything that consumes this in Rust slices `text`;
    /// the IPC layer converts to UTF-16 with [`byte_to_utf16`] on the way out,
    /// where the reader is JavaScript.
    pub highlight: Option<(u32, u32)>,
    pub truncated: bool,
}

/// Trim one source line down to something a narrow list can show.
///
/// The indentation goes — every C# line starts eight columns in, and keeping it
/// would show a column of blanks and hide the code — and the highlight moves
/// with it. A range the trim would cut into loses its highlight rather than
/// having it shifted: no underline is honest, an underline over the wrong
/// characters is a claim.
pub fn snippet(line: &str, range: Option<(u32, u32)>) -> Snippet {
    // A line split on `\n` out of a CRLF file keeps its `\r`, which renders as a
    // stray glyph or eats the rest of the row depending on the terminal.
    let line = line.trim_end_matches('\r');
    let body = line.trim();
    let indent = line.len() - line.trim_start().len();

    let highlight = range.and_then(|(start, end)| {
        if end <= start {
            // Zero-width is a cursor position and backwards is not a span.
            return None;
        }
        let start = utf16_to_byte(line, start);
        let end = utf16_to_byte(line, end).min(indent + body.len());
        if start < indent || end <= start {
            return None;
        }
        Some(((start - indent) as u32, (end - indent) as u32))
    });

    if body.chars().count() <= SNIPPET_MAX_CHARS {
        return Snippet {
            text: body.to_string(),
            highlight,
            truncated: false,
        };
    }
    cut(body, highlight)
}

/// Keep a window of a long line, centred so the match survives.
///
/// Cutting from the left would show two hundred characters of a minified bundle
/// and none of the thing the row is about. The window is chosen in *characters*
/// and converted to bytes at boundaries, because slicing a `String` anywhere
/// else panics — and a long line of non-ASCII is exactly where that happens.
fn cut(body: &str, highlight: Option<(u32, u32)>) -> Snippet {
    let boundaries: Vec<usize> = body
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(body.len()))
        .collect();
    let match_start_char = highlight
        .map(|(start, _)| boundaries.partition_point(|&b| b < start as usize))
        .unwrap_or(0);

    let first = match_start_char.saturating_sub(SNIPPET_LEAD_CHARS);
    let last = (first + SNIPPET_MAX_CHARS).min(boundaries.len() - 1);
    let (from, to) = (boundaries[first], boundaries[last]);

    Snippet {
        text: body[from..to].to_string(),
        highlight: highlight.and_then(|(start, end)| {
            let (start, end) = (start as usize, end as usize);
            // A match the window does not contain whole gets no underline
            // rather than a truncated one pointing at the wrong characters.
            if start < from || end > to {
                return None;
            }
            Some(((start - from) as u32, (end - from) as u32))
        }),
        truncated: true,
    }
}

#[cfg(test)]
#[path = "positions_tests.rs"]
mod positions_tests;
