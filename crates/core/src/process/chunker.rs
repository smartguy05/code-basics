//! Incremental UTF-8 decoding for streamed process output.
//!
//! Output is read as raw byte chunks rather than lines, because test runners
//! and build tools draw progress with bare `\r` and ANSI escapes that a
//! line-buffered reader would swallow until the next newline. Reading raw means
//! a chunk boundary can land in the middle of a multi-byte character, so
//! decoding has to carry the remainder across reads.

/// Accumulates bytes and yields the longest valid UTF-8 prefix available,
/// holding back any trailing partial character until its remaining bytes
/// arrive.
#[derive(Debug, Default)]
pub struct Utf8Chunker {
    pending: Vec<u8>,
}

impl Utf8Chunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes, returning whatever is now decodable.
    ///
    /// Returns an empty string when the input only extended an incomplete
    /// character. Genuinely invalid sequences become `U+FFFD` rather than
    /// stalling the stream.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();

        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    // SAFETY-free: valid_up_to guarantees this prefix is UTF-8.
                    out.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap_or_default());

                    match e.error_len() {
                        // An incomplete trailing character: keep it for next time.
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                        // A genuinely malformed sequence: substitute and continue.
                        Some(bad) => {
                            out.push(char::REPLACEMENT_CHARACTER);
                            self.pending.drain(..valid + bad);
                        }
                    }
                }
            }
        }

        out
    }

    /// Flush anything still buffered at end of stream, substituting for any
    /// truncated trailing character.
    pub fn finish(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let mut out = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        if out.is_empty() {
            out.push(char::REPLACEMENT_CHARACTER);
        }
        out
    }
}

/// Splits a byte/character stream into complete lines across chunk boundaries.
///
/// Used for progress detection only — the console itself receives raw chunks.
#[derive(Debug, Default)]
pub struct LineSplitter {
    buf: String,
}

impl LineSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed decoded text, returning any lines completed by it.
    ///
    /// Both `\n` and bare `\r` terminate a line: runners that redraw a progress
    /// line with `\r` produce meaningful intermediate states.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.buf.push_str(text);
        let mut lines = Vec::new();

        while let Some(idx) = self.buf.find(['\n', '\r']) {
            let line: String = self.buf[..idx].into();
            // Consume the terminator, treating CRLF as one.
            let mut rest = idx + 1;
            if self.buf.as_bytes()[idx] == b'\r' && self.buf.as_bytes().get(rest) == Some(&b'\n') {
                rest += 1;
            }
            self.buf.drain(..rest);
            if !line.is_empty() {
                lines.push(line);
            }
        }
        lines
    }

    /// Return any unterminated trailing line at end of stream.
    pub fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buf))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_ascii() {
        let mut c = Utf8Chunker::new();
        assert_eq!(c.push(b"hello"), "hello");
        assert_eq!(c.finish(), "");
    }

    #[test]
    fn carries_split_multibyte_char_across_chunks() {
        // '✓' is E2 9C 93 — split it across three reads.
        let mut c = Utf8Chunker::new();
        assert_eq!(c.push(&[0xE2]), "");
        assert_eq!(c.push(&[0x9C]), "");
        assert_eq!(c.push(&[0x93]), "✓");
        assert_eq!(c.finish(), "");
    }

    #[test]
    fn emits_text_before_a_partial_char() {
        let mut c = Utf8Chunker::new();
        assert_eq!(c.push(b"ok \xE2\x9C"), "ok ");
        assert_eq!(c.push(&[0x93]), "✓");
    }

    #[test]
    fn substitutes_invalid_sequences_without_stalling() {
        let mut c = Utf8Chunker::new();
        let out = c.push(&[b'a', 0xFF, b'b']);
        assert_eq!(out, "a\u{FFFD}b");
    }

    #[test]
    fn finish_substitutes_truncated_trailing_char() {
        let mut c = Utf8Chunker::new();
        assert_eq!(c.push(&[0xE2, 0x9C]), "");
        assert_eq!(c.finish(), "\u{FFFD}");
    }

    #[test]
    fn splits_lines_across_chunks() {
        let mut s = LineSplitter::new();
        assert_eq!(s.push("one\ntw"), vec!["one".to_string()]);
        assert_eq!(s.push("o\nthree"), vec!["two".to_string()]);
        assert_eq!(s.finish(), Some("three".to_string()));
    }

    #[test]
    fn treats_crlf_as_a_single_terminator() {
        let mut s = LineSplitter::new();
        assert_eq!(s.push("a\r\nb\r\n"), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn treats_bare_cr_as_a_line_so_progress_redraws_are_visible() {
        let mut s = LineSplitter::new();
        assert_eq!(
            s.push("50%\r100%\r"),
            vec!["50%".to_string(), "100%".to_string()]
        );
    }
}
