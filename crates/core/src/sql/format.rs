//! Rendering database values as display strings for the results grid.
//!
//! Pure formatting, no I/O. The same abstain rule applies to types as to
//! everything else: a value whose type is not understood is reported as
//! unrenderable rather than being guessed at or silently stringified, and a
//! SQL `NULL` and an empty string are two different things.
//!
//! The module takes [`Cell`] — values a driver has *already decoded* into
//! ordinary Rust — and never a driver type, so the whole rendering path is
//! provable with no database software installed. A driver adapter's job is to
//! map its own column type onto a `Cell`; the honest answers for a column it
//! cannot decode ([`Cell::Unsupported`]) and one it failed to read
//! ([`Cell::Error`]) are part of that vocabulary rather than an error it has to
//! invent a value for.
//!
//! Five outcomes are kept apart here that a careless renderer collapses into a
//! blank cell, and each one means something different to somebody reading a
//! result set:
//!
//! | Outcome | Means |
//! |---|---|
//! | [`SqlValue::Null`] | the database holds no value here |
//! | `Text { text: "" }` | the database holds the empty string |
//! | [`SqlValue::Text`] with `truncated` | there is more, and it was cut for display |
//! | [`SqlValue::Unsupported`] | this build cannot decode that column type |
//! | [`SqlValue::Unavailable`] | the driver errored reading this one cell |
//!
//! `Unsupported` is the landing place for Postgres' binary wire protocol in
//! particular, where `uuid`, `jsonb`, `numeric`, `interval`, arrays and enums
//! each need per-OID handling: a build without that handling says the type name
//! out loud rather than showing an empty cell that reads as `NULL`.

use serde::{Deserialize, Serialize};
use specta::Type;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// How many characters of a text value reach the grid.
///
/// A results grid shows one line per cell, so nothing past a few thousand
/// characters is read there — it is read by opening the value. The cap exists
/// so a table with one `text` column holding a megabyte per row cannot make the
/// grid unusable, and it is generous enough that ordinary data (names, paths,
/// short JSON) is never cut at all.
pub const MAX_TEXT_CHARS: usize = 4096;

/// How many *bytes* of a text value reach the grid.
///
/// A character cap alone does not bound the payload: [`MAX_TEXT_CHARS`] astral
/// characters encode to four times as many bytes, so a row could weigh 16 KiB
/// while appearing to be capped. Both limits apply and whichever bites first
/// wins; the cut is then moved back to a character boundary, never taken
/// mid-character.
pub const MAX_TEXT_BYTES: usize = 8192;

/// How many bytes of a binary value are rendered as hex.
///
/// Hex doubles the size, so this is 512 characters in the grid — enough to
/// recognise a header or a magic number, which is what a blob column is
/// actually scanned for. The *true* length always crosses in the `byteLength`
/// field, so a cut blob still reports how big it really was rather than how
/// much of it was shown.
pub const MAX_BYTES_RENDERED: usize = 256;

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// One cell, as a driver adapter decoded it.
///
/// Deliberately not a driver type: this is the seam that keeps the rendering
/// rules testable with no database crate compiled in. The last two variants are
/// the important ones — they let an adapter admit ignorance instead of
/// producing a value it does not have.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// SQL `NULL`. Never an empty string.
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// An exact numeric the driver already rendered — a `decimal`/`numeric`
    /// whose scale is part of the value (`0.10` is not `0.1` to a ledger), so
    /// it is carried as the server's own text rather than re-derived.
    Numeric(String),
    Text(String),
    Bytes(Vec<u8>),
    /// The column's type was recognised but this build has no decoder for it.
    /// `type_name` is the server's own name for it, so the cell can say what is
    /// missing.
    Unsupported {
        type_name: String,
    },
    /// The driver failed on this one cell. Distinct from both `Null` and
    /// `Unsupported`: the value may well exist and be decodable, and the next
    /// attempt may succeed.
    Error {
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// A cell as it crosses IPC.
///
/// Numbers cross as **strings**. Not because every number must overflow a
/// JavaScript number, but because some do — a `bigint` past 2^53, and any
/// `decimal` carrying more precision than a double — and a rounded number is
/// indistinguishable from the real one on the screen. Same reasoning as
/// [`crate::inspect::model::ObjectValue`] and its hex addresses.
///
/// This type belongs in [`super::model`] and is defined here only so the
/// rendering rules could be written and tested before that module landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SqlValue {
    /// SQL `NULL`.
    Null,
    Bool {
        value: bool,
    },
    /// Already formatted, as text. See the type comment.
    Number {
        text: String,
    },
    /// `truncated` is set when a cap cut it. Never a silent shortening.
    Text {
        text: String,
        truncated: bool,
    },
    /// Lowercase hex, no prefix. `byte_length` is the size of the *whole*
    /// value, not of `hex`.
    Bytes {
        hex: String,
        byte_length: u64,
        truncated: bool,
    },
    /// This build cannot decode that column type. Never blank, never guessed.
    Unsupported {
        type_name: String,
    },
    /// The driver could not read this cell, with the reason. Not a `NULL`.
    Unavailable {
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render one decoded cell for the grid.
pub fn format_cell(cell: Cell) -> SqlValue {
    match cell {
        Cell::Null => SqlValue::Null,
        Cell::Bool(value) => SqlValue::Bool { value },
        Cell::Int(n) => SqlValue::Number {
            text: n.to_string(),
        },
        Cell::Float(n) => SqlValue::Number {
            text: format_float(n),
        },
        Cell::Numeric(text) => SqlValue::Number { text },
        Cell::Text(text) => {
            let (text, truncated) = truncate_text(text);
            SqlValue::Text { text, truncated }
        }
        Cell::Bytes(bytes) => {
            let byte_length = bytes.len() as u64;
            let truncated = bytes.len() > MAX_BYTES_RENDERED;
            let shown = &bytes[..bytes.len().min(MAX_BYTES_RENDERED)];
            let mut hex = String::with_capacity(shown.len() * 2);
            for b in shown {
                use std::fmt::Write as _;
                let _ = write!(hex, "{b:02x}");
            }
            SqlValue::Bytes {
                hex,
                byte_length,
                truncated,
            }
        }
        Cell::Unsupported { type_name } => SqlValue::Unsupported { type_name },
        Cell::Error { reason } => SqlValue::Unavailable { reason },
    }
}

/// A float as text, with the three values JSON cannot carry spelled out rather
/// than emitted as `NaN`/`inf` — which no JSON parser accepts, and which would
/// therefore fail the whole response rather than the one cell.
fn format_float(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else {
        n.to_string()
    }
}

/// Cut `text` to the display caps, reporting whether anything was removed.
///
/// The cut is always moved back to a character boundary. Slicing a `String` at
/// an arbitrary byte index panics in Rust and produces mojibake in most other
/// languages; either way, what the user sees would not be a prefix of what the
/// database holds. (Grapheme clusters are a rendering concern and are
/// deliberately not considered — a cut between a base character and its
/// combining mark still yields valid, honest UTF-8.)
fn truncate_text(text: String) -> (String, bool) {
    let by_chars = text.char_indices().nth(MAX_TEXT_CHARS).map(|(i, _)| i);
    let by_bytes =
        (text.len() > MAX_TEXT_BYTES).then(|| floor_char_boundary(&text, MAX_TEXT_BYTES));

    let cut = match (by_chars, by_bytes) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    match cut {
        Some(at) => {
            let mut text = text;
            text.truncate(at);
            (text, true)
        }
        None => (text, false),
    }
}

/// The largest character boundary at or below `index`.
///
/// `str::floor_char_boundary` is still unstable, so this is spelled out.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
