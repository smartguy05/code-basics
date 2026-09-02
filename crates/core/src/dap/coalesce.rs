//! Merging a burst of debuggee output into as few console messages as possible.
//!
//! # Why this exists at all
//!
//! A debug adapter's `output` events are not a trickle. NetCoreDbg emits **two**
//! for every line the debuggee logs — the raw text, then a second copy carrying
//! the `source` it came from — and an application that logs structured telemetry
//! adds a multi-kilobyte JSON blob per record on top of that. A real ASP.NET
//! startup produces the better part of a thousand events in its first half
//! minute.
//!
//! That would merely be wasteful if the client could fall behind harmlessly. It
//! cannot, and this is the failure the module exists to prevent:
//!
//! **A client that stops draining the adapter's stdout deadlocks the debuggee.**
//! The pipe buffer fills, the adapter blocks writing to it, and — because the
//! adapter is writing from inside a runtime debug callback, which holds every
//! debuggee thread suspended — the application never resumes. It freezes with no
//! output, no error and no exit: from the outside, indistinguishable from an
//! application that is simply quiet. Reproduced deliberately by stalling a
//! client's reader: the debuggee stopped accumulating CPU entirely and every one
//! of its threads sat in `Wait, UserRequest` forever.
//!
//! So the read loop must never be the slow part. Reading is decoupled from
//! emitting, and this module is the decision the emitting half makes: what may
//! be merged into one message, and what must not be.
//!
//! # The abstain rule, in this module's terms
//!
//! Merging is a claim that two chunks are the same message, and a wrong merge is
//! not recoverable — the console has no way to un-merge it. So:
//!
//! * **Only one stream at a time.** `stdout` and `stderr` are never merged, in
//!   either direction. [`crate::process::Stream`] is what the console falls back
//!   to when a line carries no severity marker of its own, so a merge across
//!   streams would silently re-rank real output.
//! * **Order is preserved exactly.** Chunks concatenate in arrival order, with
//!   nothing inserted between them — an adapter's chunk boundaries do not fall
//!   on line boundaries, so a separator would corrupt every split line.
//! * **An oversized chunk is never split.** [`MAX_BATCH_BYTES`] bounds what
//!   *accumulates*, not what one chunk may be. Cutting a chunk could land inside
//!   a multi-byte character, and would hand the console half a line to rank on
//!   its own.

use crate::process::Stream;

/// How many bytes of debuggee output may accumulate into one console message.
///
/// A cap rather than a target: it stops a long burst becoming one enormous IPC
/// message, while still collapsing the common case — hundreds of small events —
/// into a handful. It is **not** a limit on a single chunk; see the module note
/// on why an oversized chunk travels whole.
pub const MAX_BATCH_BYTES: usize = 64 * 1024;

/// Output ready to be emitted as one console message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBatch {
    pub stream: Stream,
    pub text: String,
}

/// Accumulates adjacent, same-stream output chunks.
///
/// The caller pushes every `output` event as it is decoded and emits whatever
/// comes back, then calls [`Coalescer::take`] when the adapter goes quiet or a
/// non-output message needs to be emitted in order. Nothing here blocks and
/// nothing here allocates per event beyond the accumulating string.
#[derive(Debug, Default)]
pub struct Coalescer {
    stream: Option<Stream>,
    text: String,
}

impl Coalescer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a [`Coalescer::take`] would produce anything.
    ///
    /// Lets an idle flush do no work at all, rather than emitting empty
    /// messages on a timer while the debuggee is silent.
    pub fn pending(&self) -> bool {
        self.stream.is_some()
    }

    /// Buffer one output chunk.
    ///
    /// Returns a batch that must be emitted **before** this chunk's own text
    /// ever is — because the chunk could not join what was already pending,
    /// either for coming from the other stream or for overflowing the cap.
    /// `None` means the chunk was absorbed and nothing needs emitting yet.
    ///
    /// An empty chunk is ignored entirely: it is not worth a console message,
    /// and treating it as a stream change would split the batch around it.
    pub fn push(&mut self, stream: Stream, text: &str) -> Option<OutputBatch> {
        if text.is_empty() {
            return None;
        }
        let flush = match self.stream {
            Some(current) if current != stream => true,
            // Only overflow an *existing* batch. With nothing pending there is
            // nothing to flush, and the chunk is buffered whole however large
            // it is.
            Some(_) => self.text.len() + text.len() > MAX_BATCH_BYTES,
            None => false,
        };
        let ready = if flush { self.take() } else { None };
        self.stream = Some(stream);
        self.text.push_str(text);
        ready
    }

    /// Everything buffered, leaving the coalescer empty.
    pub fn take(&mut self) -> Option<OutputBatch> {
        let stream = self.stream.take()?;
        Some(OutputBatch {
            stream,
            text: std::mem::take(&mut self.text),
        })
    }
}

#[cfg(test)]
#[path = "coalesce_tests.rs"]
mod tests;
