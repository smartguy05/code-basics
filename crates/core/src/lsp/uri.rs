//! Paths in, `file:` URIs out, and back again.
//!
//! # Why this is the riskiest pure code in the subsystem
//!
//! Get it wrong and the server answers every question with an empty list. There
//! is no error, no log line and nothing to search for — the request was
//! well-formed, it just named a document the server has never heard of. Two
//! spellings of a Windows path are both in the wild:
//!
//! * `file:///C%3A/x/y.cs` — percent-encoded colon. rust-analyzer emits this,
//!   and it is what a `url`-crate default produces.
//! * `file:///C:/x/y.cs` — plain colon. **Verified against the real Roslyn
//!   server this session**: an `initialize` and a `didOpen` in this form
//!   resolved a cross-file reference correctly. The VS Code C# extension sends
//!   the same shape (it serialises with `vscode.Uri.toString(true)`, i.e.
//!   skipEncoding), differing only in using a lower-case drive letter — so the
//!   drive's case does not matter and the colon's encoding might.
//!
//! Hence [`UriStyle`]: the spelling is a per-server property in the registry,
//! not a global constant.
//!
//! # The rule that makes the choice survivable
//!
//! **Identity is never a URI-string comparison.** Every incoming URI is turned
//! into a path immediately, and equality is decided on paths (through
//! [`crate::symbols::index::relative_to_root`]). That demotes [`UriStyle`] from
//! a correctness requirement to a compatibility one: even if we send a spelling
//! a server does not prefer and it echoes a different one back, matching still
//! works. With four servers that disagree, no other arrangement is safe.
//!
//! # Where this abstains
//!
//! [`from_file_uri`] returns `None` rather than a best guess for a non-`file:`
//! scheme, a malformed escape, or bytes that are not UTF-8. Roslyn really does
//! emit `source-generated:` and metadata URIs, and there is no path behind them;
//! fabricating one would open an unrelated file that happens to sit where the
//! guess landed. Callers must render such a result as *present but unopenable*
//! rather than dropping it, because dropping it would make a count wrong.
//!
//! [`to_file_uri`] returns `None` for a relative path. A `file:` URI is absolute
//! by definition, so a relative one means a caller has lost track of which root
//! it was taken against — the defect class this repository has hit repeatedly —
//! and joining it onto a guessed root is how the wrong file gets opened.
//!
//! The drive-letter rules are decided **on the string, not with `Path`**, the
//! same way [`crate::architecture`]'s `is_rooted` does and for the same reason:
//! `Path::is_absolute` calls `C:\x` relative on Linux, so a test would disagree
//! with itself across platforms.

use std::path::{Path, PathBuf};

/// How a server wants the drive colon spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriStyle {
    /// `file:///C%3A/x` — the conservative form.
    Encoded,
    /// `file:///C:/x` — what the C# extension sends Roslyn.
    Plain,
}

/// Turn an absolute path into a `file:` URI, or abstain.
pub fn to_file_uri(path: &Path, style: UriStyle) -> Option<String> {
    let raw = path.to_str()?;
    let slashed = raw.replace('\\', "/");
    // A trailing separator has to go: the workspace root arrives with one from
    // `git2`'s `workdir()`, and a `rootUri` ending in `/` makes some servers
    // compute a different relative path for every file in the project.
    let trimmed = slashed.trim_end_matches('/');

    if let Some(rest) = trimmed.strip_prefix("//") {
        // UNC: the host becomes the URI's authority, which is what it is.
        let (host, tail) = rest.split_once('/')?;
        if host.is_empty() || tail.is_empty() {
            return None;
        }
        return Some(format!("file://{host}/{}", encode_path(tail)));
    }

    if let Some(drive) = drive_of(trimmed) {
        // `C:x` is relative to that drive's current directory and is not
        // locatable from here, so the separator is required.
        let rest = trimmed.get(2..)?.strip_prefix('/')?;
        let colon = match style {
            UriStyle::Encoded => "%3A",
            UriStyle::Plain => ":",
        };
        return Some(format!("file:///{drive}{colon}/{}", encode_path(rest)));
    }

    let rest = trimmed.strip_prefix('/')?;
    if rest.is_empty() {
        return None;
    }
    Some(format!("file:///{}", encode_path(rest)))
}

/// Turn a `file:` URI into a path, or abstain.
pub fn from_file_uri(uri: &str) -> Option<PathBuf> {
    let (scheme, rest) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let rest = rest.strip_prefix("//")?;

    // The authority is everything before the first separator. `localhost` is a
    // legal spelling of "this machine" and is not a share on a host of that
    // name.
    let (authority, path) = match rest.find(['/', '\\']) {
        Some(at) => (&rest[..at], &rest[at + 1..]),
        None => (rest, ""),
    };
    let path = decode(path)?;
    // Some servers escape the separator itself. A `%5C` left in a component
    // would be a literal backslash in a filename, which cannot exist on
    // Windows, so nothing would open.
    let path = path.replace('\\', "/");
    if path.is_empty() {
        return None;
    }

    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return Some(PathBuf::from(format!(
            r"\\{authority}\{}",
            path.replace('/', "\\")
        )));
    }
    if drive_of(&path).is_some() {
        return Some(PathBuf::from(path.replace('/', "\\")));
    }
    Some(PathBuf::from(format!("/{path}")))
}

/// The drive letter of a `X:`-prefixed string, whatever follows it.
fn drive_of(path: &str) -> Option<char> {
    let mut chars = path.chars();
    let letter = chars.next()?;
    if letter.is_ascii_alphabetic() && chars.next() == Some(':') {
        Some(letter)
    } else {
        None
    }
}

/// Percent-encode everything but the unreserved set, keeping `/` a separator.
///
/// `#` and `?` are the ones that matter beyond tidiness: an unencoded `#` makes
/// the rest of the path a fragment, so `C:\a#b\c.cs` would name `C:\a` and the
/// server would open the wrong file and report success. `%` must be encoded too,
/// or a filename containing `%3A` would decode into a colon that was never there.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Percent-decode, abstaining on a malformed escape or non-UTF-8 bytes.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        // `%zz` is not a byte, and neither dropping it nor passing it through
        // gives a path naming the file the server meant.
        let hex = text.get(index + 1..index + 3)?;
        out.push(u8::from_str_radix(hex, 16).ok()?);
        index += 3;
    }
    // Lossy conversion would substitute U+FFFD and name a file that cannot exist.
    String::from_utf8(out).ok()
}

#[cfg(test)]
#[path = "uri_tests.rs"]
mod uri_tests;
