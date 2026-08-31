//! The two ceilings a result set is read under, and the arithmetic that decides
//! when one of them has bitten.
//!
//! Pure. No driver type appears here, so the whole cap rule is provable with no
//! database software installed.
//!
//! There are **two** caps rather than one because they fail in opposite
//! directions. A row ceiling alone lets a hundred rows of one-megabyte `TEXT`
//! columns arrive as 100 MB of JSON; a byte ceiling alone lets ten million
//! narrow rows crawl through one at a time. Whichever bites first stops the
//! stream, and which one it was is reported, because the two suggest different
//! next actions: raising a row limit does nothing for a result set the byte
//! budget stopped.
//!
//! # A cap is only real once a further row has been seen
//!
//! The subtle part, and the reason [`CapTracker::confirm_more`] exists. A table
//! holding exactly [`Limits::max_rows`] rows fills the tracker to capacity and
//! then ends. Reporting a [`RowCap`] there would tell the user their answer is
//! incomplete when they are in fact looking at all of it — a lie in the same
//! family as a silently shortened list, just pointing the other way. So
//! capacity being reached only *arms* the cap; it becomes a reported
//! [`RowCap`] when the driver actually sees a row it will not deliver.
//!
//! # A cell is weighed at its source size, and copied under a bound
//!
//! The budget is charged at whichever is larger, what crosses the wire or what
//! the database holds ([`charged_cell_bytes`]). Charging the wire size alone
//! defeats the budget outright: [`crate::sql::format`] renders a cell under its
//! own display caps, so a column of two-megabyte values weighs a few kilobytes
//! a row and an eight-megabyte budget never bites. Charging the source size
//! means one such cell can exhaust the budget by itself, which is reported as
//! an ordinary [`RowCapReason::ByteLimit`] — a cap, not an out-of-memory.
//!
//! [`TEXT_COPY_BYTES`] and [`MAX_BLOB_COPY_BYTES`] are the other half: the
//! driver copies a bounded prefix rather than the whole value, so the budget is
//! consulted about something that was never materialised twice.

use crate::sql::format::MAX_TEXT_BYTES;
use crate::sql::model::{RowCap, RowCapReason, SqlValue};

/// How many rows reach the grid.
pub const DEFAULT_MAX_ROWS: u32 = 1000;

/// How many bytes of decoded value data reach the grid, before the JSON
/// envelope. Eight mebibytes: large enough that ordinary result sets are never
/// touched, small enough that one pathological row cannot wedge the frontend.
pub const DEFAULT_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The largest prefix of a text value copied out of the driver's buffer.
///
/// Four bytes — one whole character — past what
/// [`crate::sql::format::MAX_TEXT_BYTES`] will render, so a value longer than
/// the display cap still *arrives* longer than it and is reported as
/// `truncated`. Clipping exactly at the display cap would make a two-megabyte
/// value indistinguishable from one that happens to end there.
pub const TEXT_COPY_BYTES: usize = MAX_TEXT_BYTES + 4;

/// The largest single binary value copied out of the driver's buffer.
///
/// A blob is the one cell a bounded copy cannot honestly shorten: the wire
/// shape reports `byteLength` — the size of the *whole* value — so handing the
/// renderer a prefix would report the prefix's length as the value's length,
/// which is a lie rather than a cut. Past this ceiling the driver therefore
/// abstains, naming the size it declined to read, and still charges the byte
/// budget at that size.
pub const MAX_BLOB_COPY_BYTES: u64 = 1024 * 1024;

/// Charged per cell on top of its payload.
///
/// A row of a hundred `NULL`s costs nothing to hold and a great deal to
/// serialise, render and lay out. Without a per-cell charge a very wide,
/// very empty result set is invisible to the byte budget.
pub const VALUE_OVERHEAD_BYTES: u64 = 16;

/// The ceilings for one statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_rows: u32,
    pub max_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_MAX_ROWS,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// Whether the row loop may read another row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Capacity is reached. The loop must still *look* for one more row — see
    /// the module docs — but must not deliver it.
    Full,
}

/// Accounts rows against [`Limits`] and reports the cap that bit.
#[derive(Debug, Clone)]
pub struct CapTracker {
    limits: Limits,
    rows: u32,
    bytes: u64,
    /// Set the moment capacity is reached. Not yet a reportable cap.
    armed: Option<RowCapReason>,
    /// Set when a further, undelivered row was actually seen.
    confirmed: bool,
}

impl CapTracker {
    pub fn new(limits: Limits) -> Self {
        // A ceiling of zero is reached before the first row, so the tracker is
        // armed from the start rather than after a row it must not deliver.
        let armed = if limits.max_bytes == 0 {
            Some(RowCapReason::ByteLimit)
        } else if limits.max_rows == 0 {
            Some(RowCapReason::RowLimit)
        } else {
            None
        };
        Self {
            limits,
            rows: 0,
            bytes: 0,
            armed,
            confirmed: false,
        }
    }

    /// Charge one delivered row and say whether the loop continues.
    ///
    /// The row is charged *after* being admitted, so the byte budget may be
    /// overshot by the size of one row. That is deliberate: rejecting a row
    /// wider than the whole budget would render an empty grid for a table that
    /// has data in it, which is a worse answer than a slightly large payload.
    pub fn record(&mut self, row_bytes: u64) -> Flow {
        if self.armed.is_some() {
            return Flow::Full;
        }
        self.rows = self.rows.saturating_add(1);
        self.bytes = self.bytes.saturating_add(row_bytes);

        // Byte budget takes precedence when both bite on the same row: it is
        // the binding constraint, and saying `RowLimit` there would invite the
        // user to raise a ceiling that would not give them another row.
        if self.bytes >= self.limits.max_bytes {
            self.armed = Some(RowCapReason::ByteLimit);
        } else if self.rows >= self.limits.max_rows {
            self.armed = Some(RowCapReason::RowLimit);
        }
        match self.armed {
            Some(_) => Flow::Full,
            None => Flow::Continue,
        }
    }

    /// True when no further row may be delivered.
    pub fn is_full(&self) -> bool {
        self.armed.is_some()
    }

    /// Record that a row existed beyond the ones delivered. Only this turns an
    /// armed cap into a reported one.
    pub fn confirm_more(&mut self) {
        if self.armed.is_some() {
            self.confirmed = true;
        }
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The cap to put on the result set. [`None`] means every row is present.
    pub fn cap(&self) -> Option<RowCap> {
        if !self.confirmed {
            // Armed but never confirmed: the rows ran out exactly at the
            // ceiling, so every row is present. See the module docs.
            return None;
        }
        self.armed.map(|reason| RowCap {
            limit: self.rows,
            reason,
        })
    }
}

/// The byte cost of one rendered cell.
pub fn value_bytes(value: &SqlValue) -> u64 {
    let payload = match value {
        SqlValue::Null => 0,
        SqlValue::Bool { .. } => 1,
        SqlValue::Number { text } => text.len(),
        SqlValue::Text { text, .. } => text.len(),
        SqlValue::Bytes { hex, .. } => hex.len(),
        SqlValue::Unsupported { type_name } => type_name.len(),
        SqlValue::Unavailable { reason } => reason.len(),
    };
    payload as u64 + VALUE_OVERHEAD_BYTES
}

/// The byte cost of one rendered row.
pub fn row_bytes(row: &[SqlValue]) -> u64 {
    row.iter().map(value_bytes).sum()
}

/// What one cell costs the byte budget, given how big the value is in the
/// database.
///
/// Whichever is larger: what crosses the wire, or what the database holds. The
/// second half is the one that matters. A cell is rendered under
/// [`crate::sql::format`]'s display caps, so a two-megabyte value reaches the
/// grid as eight kilobytes — and charging *that* lets an unbounded amount of
/// data stream past the budget while the tracker reads a few kilobytes a row,
/// which is precisely the thing a byte budget exists to stop. Charging the
/// source size instead means one such cell can exhaust the budget on its own,
/// which is reported as a [`RowCapReason::ByteLimit`] like any other.
pub fn charged_cell_bytes(value: &SqlValue, source_bytes: u64) -> u64 {
    value_bytes(value).max(source_bytes.saturating_add(VALUE_OVERHEAD_BYTES))
}

/// What one row costs the byte budget. Each cell is paired with its source
/// size, so the two can never be zipped out of step.
pub fn charged_row_bytes(row: &[(SqlValue, u64)]) -> u64 {
    row.iter()
        .map(|(value, source)| charged_cell_bytes(value, *source))
        .sum()
}

/// The prefix of `text` worth copying, cut at a character boundary.
///
/// Never cuts mid-character: slicing a `str` at an arbitrary byte index panics,
/// and what the user saw would not be a prefix of what the database holds. The
/// caller keeps the *source* length separately — this returns what is copied,
/// not what was there.
pub fn clip_text(text: &str) -> &str {
    if text.len() <= TEXT_COPY_BYTES {
        return text;
    }
    &text[..floor_char_boundary(text, TEXT_COPY_BYTES)]
}

/// The largest character boundary at or below `index`.
///
/// `str::floor_char_boundary` is still unstable, so this is spelled out — as it
/// is in [`crate::sql::format`], which cuts for display where this one cuts for
/// memory.
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
#[path = "caps_tests.rs"]
mod tests;
