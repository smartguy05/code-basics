//! Line-delimited JSON framing: the MCP stdio wire.
//!
//! # Why this is not [`crate::lsp::framing`]
//!
//! LSP frames every message with a `Content-Length` header. MCP does not, and
//! the difference is not a matter of taste — the specification says a stdio
//! message is one line and *"MUST NOT contain embedded newlines"*. Reusing the
//! header framing here would produce a server no client can read.
//!
//! The second difference is the one worth a test, because it is a reversal of
//! reasoning that is otherwise identical. [`crate::lsp::framing::LineDecoder`]'s
//! sibling **poisons itself** on the first error and refuses everything
//! afterwards, because once a `Content-Length` is unreadable there is no way to
//! know where the next frame begins, and a resynchronisation attempt would hand
//! a response to the waiter for a different request.
//!
//! **That reasoning does not transfer.** A newline *is* a resynchronisation
//! point. A line that is not JSON is therefore one bad message, not a lost
//! stream: the bytes up to the next `\n` are the whole of the damage, and the
//! message after it is as trustworthy as any other. So [`LineDecoder`] reports
//! the bad line ([`Line::Malformed`], never silently dropped) and carries on.
//!
//! [`NdjsonError::LineTooLarge`] **does** poison, and for exactly the original
//! reason: the terminator was never seen, so the boundary genuinely is unknown.
//! Everything still buffered may be the middle of a message, and where the next
//! one starts is not knowable. The caller's only correct reaction is to stop.

use serde_json::Value;
use thiserror::Error;

/// The largest line this will buffer: 8 MiB.
///
/// A real MCP message is kilobytes. The cap is not a tuning knob but a bound on
/// what a peer that has started emitting garbage — or one that never sends a
/// newline at all — can make this process allocate.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Why the stream cannot be read further. Both variants are terminal; a
/// malformed *line* is not here, because it is not one of these.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NdjsonError {
    #[error("a line exceeded {limit} bytes with no terminator, so where the next message begins is not knowable")]
    LineTooLarge { limit: usize },
    #[error("the stream cannot be resynchronised after an unterminated over-long line")]
    Poisoned,
}

/// One complete line the decoder read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A well-formed JSON message.
    Message(Value),
    /// A line that was not JSON. Reported rather than dropped: a client that is
    /// sending garbage should be told so, and silence would leave a request
    /// looking like one that was never sent.
    Malformed { text: String, error: String },
}

/// Wrap one message for the wire.
///
/// Compact, never pretty: `serde_json::to_string_pretty` is what almost every
/// other writer in this tree uses, and here it would put newlines *inside* a
/// message, which the specification forbids and which this module's own decoder
/// would read as several broken ones. Pinned by
/// `encoded_output_contains_exactly_one_newline_and_it_is_last`.
pub fn encode(message: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(message).unwrap_or_else(|_| b"null".to_vec());
    bytes.push(b'\n');
    bytes
}

/// Reassembles messages from arbitrary byte chunks.
#[derive(Debug)]
pub struct LineDecoder {
    buffer: Vec<u8>,
    limit: usize,
    poisoned: bool,
}

impl Default for LineDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::with_limit(MAX_LINE_BYTES)
    }

    /// A decoder with a smaller cap, so the over-long case can be exercised
    /// without allocating the real one.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            limit,
            poisoned: false,
        }
    }

    /// Whether this decoder has given up. Once true it stays true.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Feed bytes in; get back every line they completed.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Line>, NdjsonError> {
        if self.poisoned {
            return Err(NdjsonError::Poisoned);
        }
        self.buffer.extend_from_slice(chunk);

        let mut lines = Vec::new();
        loop {
            let Some(at) = self.buffer.iter().position(|b| *b == b'\n') else {
                // No terminator yet. The cap applies to what is still pending,
                // because that is precisely the unterminated case.
                if self.buffer.len() > self.limit {
                    self.poisoned = true;
                    self.buffer.clear();
                    return Err(NdjsonError::LineTooLarge { limit: self.limit });
                }
                return Ok(lines);
            };
            let mut line = self.buffer.drain(..=at).collect::<Vec<u8>>();
            line.pop(); // the '\n'
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            // A blank line is not a message and is not an error: it is what a
            // peer writing "\r\n" line endings, or a stray terminator, produces.
            if line.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            match serde_json::from_slice::<Value>(&line) {
                Ok(value) => lines.push(Line::Message(value)),
                Err(error) => lines.push(Line::Malformed {
                    text: String::from_utf8_lossy(&line).into_owned(),
                    error: error.to_string(),
                }),
            }
        }
    }
}

#[cfg(test)]
#[path = "ndjson_tests.rs"]
mod tests;
