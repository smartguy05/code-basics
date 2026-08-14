//! The `Content-Length` framing that carries every LSP message.
//!
//! Byte-oriented on purpose, and deliberately *not* built on
//! [`crate::process::Utf8Chunker`]. That type exists to emit partial UTF-8 as
//! display text as soon as it arrives, which is exactly wrong here: a frame is
//! length-delimited in **bytes**, and a body must be handed on whole or not at
//! all. Decoding to text first would also make a multi-byte character straddling
//! a read boundary change the apparent length of the body.
//!
//! # Failing closed
//!
//! A framing error is not recoverable. There is no way to know where the next
//! frame begins once the length is unreadable, and a resynchronisation attempt
//! would silently misalign every later message — so a response could be handed
//! to the waiter for a different request. The decoder therefore poisons itself
//! on the first error and refuses everything afterwards, including well-formed
//! frames. The caller's only correct reaction is to tear the server down.

use thiserror::Error;

/// The largest body this will buffer: 32 MiB.
///
/// A real message is kilobytes; the cap is not a performance tuning knob but a
/// bound on what a server that has started emitting garbage can make us
/// allocate.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// The largest header block this will buffer before giving up: 8 KiB.
///
/// Needed as a separate cap because the frame cap cannot be applied until a
/// `Content-Length` has been read, and a stream that never terminates its
/// header block would otherwise grow without limit.
pub const MAX_HEADER_BYTES: usize = 8 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FramingError {
    #[error("header block has no Content-Length: {header:?}")]
    MissingContentLength { header: String },
    #[error("Content-Length is not a byte count: {value:?}")]
    BadContentLength { value: String },
    #[error("header block is malformed: {detail}")]
    MalformedHeader { detail: String },
    #[error("header block exceeded {MAX_HEADER_BYTES} bytes with no terminator")]
    HeaderTooLarge,
    #[error("message body of {len} bytes exceeds the {MAX_FRAME_BYTES} byte limit")]
    FrameTooLarge { len: usize },
    #[error("the stream cannot be resynchronised after a framing error")]
    Poisoned,
}

/// Wrap a message body in the header the protocol requires.
pub fn encode(payload: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
    let mut framed = Vec::with_capacity(header.len() + payload.len());
    framed.extend_from_slice(header.as_bytes());
    framed.extend_from_slice(payload);
    framed
}

/// Reassembles message bodies from arbitrary byte chunks.
pub struct Decoder {
    buf: Vec<u8>,
    max_frame: usize,
    max_header: usize,
    /// Set by the first error. See the module doc on failing closed.
    poisoned: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self::with_limits(MAX_FRAME_BYTES, MAX_HEADER_BYTES)
    }

    pub fn with_limits(max_frame: usize, max_header: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_frame,
            max_header,
            poisoned: false,
        }
    }

    /// How many bytes are held waiting for the rest of their frame.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Feed bytes in, take whole message bodies out.
    ///
    /// Returns every frame that completed, in arrival order — which is the only
    /// ordering the protocol guarantees, and the order responses must be
    /// correlated in.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, FramingError> {
        if self.poisoned {
            return Err(FramingError::Poisoned);
        }
        self.buf.extend_from_slice(bytes);
        match self.drain() {
            Ok(frames) => Ok(frames),
            Err(error) => {
                self.poisoned = true;
                Err(error)
            }
        }
    }

    fn drain(&mut self) -> Result<Vec<Vec<u8>>, FramingError> {
        let mut frames = Vec::new();
        loop {
            let Some(header_end) = self.header_end()? else {
                return Ok(frames);
            };
            let header = &self.buf[..header_end];
            let len = content_length(header)?;
            if len > self.max_frame {
                return Err(FramingError::FrameTooLarge { len });
            }

            let body_start = header_end + 4;
            if self.buf.len() < body_start + len {
                // The header has arrived and the body has not. Nothing is
                // consumed, so the next push re-parses the same header — cheap,
                // and it keeps the buffer a single contiguous region.
                return Ok(frames);
            }
            frames.push(self.buf[body_start..body_start + len].to_vec());
            self.buf.drain(..body_start + len);
        }
    }

    /// The offset of the `\r\n\r\n` that ends the header block, if it is here.
    fn header_end(&self) -> Result<Option<usize>, FramingError> {
        let crlf = find(&self.buf, b"\r\n\r\n");
        // A bare `\n\n` terminator is refused rather than tolerated. Accepting
        // it would mean disagreeing with every real server about where the body
        // starts, and the disagreement is silent: the body would be read short
        // and every later frame misaligned. Only a `\n\n` *before* the first
        // proper terminator is a malformed header — one after it belongs to a
        // body, which is length-delimited and may contain anything.
        if let Some(lf) = find(&self.buf, b"\n\n") {
            if match crlf {
                Some(crlf) => lf < crlf,
                None => true,
            } {
                return Err(FramingError::MalformedHeader {
                    detail: "the header block must end with CRLF CRLF, not LF LF".into(),
                });
            }
        }
        match crlf {
            Some(end) => Ok(Some(end)),
            None if self.buf.len() > self.max_header => Err(FramingError::HeaderTooLarge),
            None => Ok(None),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Read the body length out of a header block.
///
/// Header field names are ASCII-case-insensitive — every casing of this one has
/// shipped in a real server — and any other field (`Content-Type` is the one in
/// the wild) is ignored rather than refused.
fn content_length(header: &[u8]) -> Result<usize, FramingError> {
    // The header block is ASCII by specification. A non-ASCII byte in it is not
    // a header we can read, and lossy conversion would only move the failure.
    let header = std::str::from_utf8(header).map_err(|_| FramingError::MalformedHeader {
        detail: "the header block is not valid UTF-8".into(),
    })?;
    for line in header.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("Content-Length") {
            continue;
        }
        let value = value.trim();
        return value
            .parse::<usize>()
            .map_err(|_| FramingError::BadContentLength {
                value: value.to_string(),
            });
    }
    Err(FramingError::MissingContentLength {
        header: header.to_string(),
    })
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod framing_tests;
