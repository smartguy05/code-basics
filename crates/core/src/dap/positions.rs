//! Line and column conventions, and the one thing this app refuses to let an
//! adapter change about them.
//!
//! # Why this is not [`crate::lsp::positions`]
//!
//! LSP fixes its conventions in the specification: lines count from zero and
//! columns count UTF-16 code units, always. DAP does not — it **negotiates**
//! them. The client declares `linesStartAt1` and `columnsStartAt1` in
//! `initialize`, and the adapter is then obliged to speak that convention back.
//! So the conversion here is not a translation between two fixed systems; it is
//! the enforcement of a claim this app made on the wire.
//!
//! [`CLIENT_LINES_START_AT_1`] and [`CLIENT_COLUMNS_START_AT_1`] are what
//! [`super::protocol::initialize_arguments`] sends, and they are `true` because
//! every line number in this app is already 1-based —
//! [`crate::symbols::index::Symbol::line`], the editor gutter, the
//! open-a-file-at-a-line chain. Declaring 1 and then subtracting one somewhere
//! would be an off-by-one nobody could find, because both ends would be
//! individually correct.
//!
//! # Adapters that ignore the negotiation
//!
//! Some do. The failure is a breakpoint that binds one line above or below where
//! the user clicked, which looks like the debugger working and stopping in the
//! wrong place — much worse than not stopping at all.
//!
//! Nothing here tries to detect or correct for it, and that is deliberate: the
//! only evidence available is a line number that *could* legitimately be
//! different, because an adapter is allowed to move a breakpoint to the next
//! executable line and report where it really put it. A correction would be
//! indistinguishable from undoing a legitimate adjustment. What this module does
//! instead is keep the adapter's own reported line
//! ([`super::breakpoints::Breakpoint::actual_line`]) separate from the requested
//! one, so the UI can *show* the difference rather than a guess about it.

/// What `initialize` tells the adapter about this client's line base.
pub const CLIENT_LINES_START_AT_1: bool = true;

/// What `initialize` tells the adapter about this client's column base.
pub const CLIENT_COLUMNS_START_AT_1: bool = true;

/// Clamp a line number arriving from an adapter into this app's 1-based space.
///
/// An adapter that honours the negotiation sends a 1-based line and this is the
/// identity. The clamp catches the two values that cannot be lines here — `0`
/// (a 0-based adapter's first line) and anything negative — and turns them into
/// line 1 rather than into a panic or a `u32` that wrapped.
///
/// It does **not** shift a whole 0-based stream up by one. Shifting would be a
/// guess about which convention the adapter used, made from a single number that
/// is consistent with both; clamping only refuses values that are impossible
/// under either.
pub fn line_from_adapter(line: i64) -> u32 {
    line.clamp(1, u32::MAX as i64) as u32
}

/// The same for a column, which adapters omit far more often than they omit a
/// line — hence the `Option`. `None` means "somewhere on that line", which is a
/// real answer and not column 1.
pub fn column_from_adapter(column: Option<i64>) -> Option<u32> {
    column.map(|c| c.clamp(1, u32::MAX as i64) as u32)
}

/// A line number on its way *to* an adapter.
///
/// Line 0 does not exist in this app's numbering, so it is raised to 1 rather
/// than sent: an adapter receiving 0 while having been told lines start at 1 is
/// entitled to do anything at all with it.
pub fn line_to_adapter(line: u32) -> i64 {
    i64::from(line.max(1))
}

#[cfg(test)]
#[path = "positions_tests.rs"]
mod tests;
