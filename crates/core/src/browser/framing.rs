//! The wire format of the browser control pipe — **reused, not rewritten**.
//!
//! # The question this module exists to answer
//!
//! The browser MCP shim talks to the running app over a named pipe carrying
//! JSON-RPC. The obvious question is which of this crate's two existing framing
//! layers that is, and the answer is that it is already written:
//! [`crate::mcp::ndjson`] fits unchanged, so this module re-exports it and adds
//! one constant. **There is no second copy.**
//!
//! [`crate::lsp::framing`] is the wrong one, for the same reason it was the
//! wrong one for MCP: it frames with a `Content-Length` header, and a header
//! whose length is unreadable leaves no way to find the next frame — which is
//! why that decoder *poisons* on the first error.
//!
//! # Why `mcp::ndjson` fits, on the four counts that matter
//!
//! 1. **The boundary is a newline.** The pipe carries JSON-RPC messages over a
//!    byte stream with no length prefix, exactly like MCP stdio, so a `\n` is
//!    genuinely where one message ends.
//! 2. **Its two poison rules are already the right way round here.** A line
//!    that is not JSON is *one bad message*, because the newline after it is a
//!    real resynchronisation point — so `LineDecoder` reports it as
//!    [`Line::Malformed`] and carries on. An over-long line with no terminator
//!    **does** poison, because the boundary is then genuinely unknown. Both
//!    apply verbatim to this pipe.
//! 3. **It reports rather than drops.** A malformed line reaching the caller as
//!    a value is what lets the panel show a probe counter instead of silently
//!    discarding traffic — and on this pipe an unexplained malformed line is
//!    more interesting than on MCP stdio, because anything on the machine that
//!    can read `%APPDATA%` can connect.
//! 4. **[`encode`] is compact with exactly one trailing newline**, which the
//!    JSON-RPC-over-NDJSON contract requires and which that module already
//!    pins with its own test.
//!
//! # The one thing this module adds, and why it is *smaller*
//!
//! [`MAX_PIPE_LINE_BYTES`] is a tighter cap than [`crate::mcp::ndjson`]'s own
//! 8 MiB. A browser *request* is a tool call — a URL, an element reference, a
//! short string — and never a page dump: page text travels in the **reply**,
//! which this process writes and does not have to buffer from a peer. Meanwhile
//! the pipe's reachable-by-any-local-process exposure makes the allocation a
//! prober can force worth bounding harder. Pass it to
//! [`LineDecoder::with_limit`] on the read side.
//!
//! Nothing else here. If this module ever grows a decoder of its own, the thing
//! to check first is whether the reason is real or whether `mcp::ndjson` simply
//! needed a parameter.

pub use crate::mcp::ndjson::{encode, Line, LineDecoder, NdjsonError};

/// The read cap for one line on the browser control pipe: 1 MiB.
///
/// See the module doc — deliberately smaller than
/// [`crate::mcp::ndjson::MAX_LINE_BYTES`], because a request on this pipe is
/// always small and the pipe is reachable by any process running as this user.
pub const MAX_PIPE_LINE_BYTES: usize = 1024 * 1024;

/// A decoder for the browser pipe: `mcp::ndjson`'s, with this module's cap.
///
/// A named constructor rather than a comment telling every call site to pass
/// the constant, so a reader who forgets cannot silently get the 8 MiB default.
pub fn pipe_decoder() -> LineDecoder {
    LineDecoder::with_limit(MAX_PIPE_LINE_BYTES)
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
