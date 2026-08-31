//! Reading connection strings out of environment files (`.env` and friends) and
//! .NET `appsettings*.json` connection-string sections.
//!
//! This is the one place a value is deliberately read rather than refused, and
//! it exists *outside* `architecture/` for exactly that reason — the graph's
//! refusal to surface connection-string values is unchanged by it. A value read
//! here is handed to the caller for an explicit, user-initiated connection; it
//! is never persisted, logged, or fed into a graph.
//!
//! # What this parser deliberately does not do
//!
//! **It does not interpolate.** `$VAR`, `${VAR}` and `%VAR%` are left exactly
//! as written and the value is returned as [`EnvValue::Unresolved`] rather than
//! as a value. A checked-in `.env` is very often a *template* that something
//! else (CI, a container runtime, a shell) substitutes at deploy time, and this
//! parser sees neither that process nor its environment. Expanding from whatever
//! happens to be in *this* process' environment would silently hand the SQL
//! console a different host than the one the file names — and connecting to the
//! wrong database is far worse than not connecting. So the reference is
//! preserved verbatim and the caller is told, in a distinct outcome it can list,
//! show and refuse to connect with; it is neither expanded, nor dropped, nor
//! returned as if it were a real value.
//!
//! The detection is deliberately eager and one-directional: anything *shaped*
//! like a reference is treated as one, so a literal value that happens to
//! contain `$name` is reported as unresolved. That is the abstain direction —
//! the user is asked rather than connected on a guess — and it applies inside
//! single quotes too. Single quotes are the no-expansion form in most `.env`
//! dialects, but a literal `${DB_HOST}` is not a usable host whoever was
//! supposed to substitute it, so the answer is the same.
//!
//! **It does not skip what it cannot read.** A line that is not blank, not a
//! comment and not a valid assignment becomes an [`EnvProblem`] carrying its
//! 1-based line number, because a silently shorter list of keys is
//! indistinguishable from a correct one. A problem carries a *reason*, never the
//! offending text: a malformed line is exactly where a secret is likeliest to be
//! mistyped, and rule 3 of the subsystem docs holds here too.
//!
//! No I/O happens here at all — [`parse`] takes the file's text — so every rule
//! above is provable with no filesystem.

use serde::{Deserialize, Serialize};
use specta::Type;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The value side of one assignment.
///
/// Two variants rather than one string: see the module docs. A value carrying an
/// unsubstituted reference is a different answer from a value, and must not
/// reach a connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EnvValue {
    /// The value as the file gives it, with quoting removed and (for a
    /// double-quoted value) escapes applied. Usable as-is.
    Literal { text: String },
    /// The value still contains something shaped like a variable reference.
    ///
    /// `raw` is the text exactly as written, so a caller can show the user what
    /// the file actually says. It is display text and not a value: it may still
    /// carry the resolved *parts* of a connection string, so anything logging it
    /// passes it through [`crate::sql::dsn::redact`] first. `reason` is the one
    /// half that is *not* display text — it is copied into a wire type and
    /// shown, so it names the *syntax class* of the reference that was found
    /// ([`RefSyntax`]) and never the match itself, and it says expansion is
    /// *refused*, not unsupported.
    Unresolved { raw: String, reason: String },
}

impl EnvValue {
    /// Whether this value may be used as-is (handed to a connector, say).
    ///
    /// The one place the two variants are compared, so no caller has to
    /// re-derive the rule with a `matches!` of its own.
    pub fn is_usable(&self) -> bool {
        matches!(self, EnvValue::Literal { .. })
    }

    /// The text as written, whichever variant this is — for display only.
    pub fn as_written(&self) -> &str {
        match self {
            EnvValue::Literal { text } => text,
            EnvValue::Unresolved { raw, .. } => raw,
        }
    }
}

/// One `KEY=VALUE` assignment, with the line it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub key: String,
    pub value: EnvValue,
    /// 1-based, counting blank and comment lines, so it matches what an editor
    /// shows.
    pub line: u32,
}

/// Why a line could not be read.
///
/// Five kinds rather than one "bad line": a missing `=`, a name that is not a
/// name, and a quote that never closes are different mistakes with different
/// fixes, and the user is the one who has to make the fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EnvProblemKind {
    /// The line has no `=` at all.
    NoAssignment,
    /// There is an `=`, but nothing to the left of it.
    EmptyKey,
    /// The name to the left of `=` is not a valid environment variable name.
    InvalidKey,
    /// A quoted value has no closing quote on its line.
    UnterminatedQuote,
    /// A quoted value is followed by something that is neither whitespace nor a
    /// comment, so where the value ends is unclear.
    TrailingCharacters,
}

/// A line that was not turned into an entry.
///
/// Deliberately carries **no copy of the line**: a problem list is the kind of
/// thing that gets shown, copied, and pasted into a bug report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvProblem {
    /// 1-based line number.
    pub line: u32,
    pub kind: EnvProblemKind,
    /// A sentence for the user, built only from the kind and the line number.
    pub reason: String,
}

/// The whole of one parsed environment file.
///
/// `problems` is part of the result rather than an error, because one unreadable
/// line does not make the other twenty unreadable — but it is never empty when
/// something was skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvFile {
    /// Every assignment, in file order. A key assigned twice appears twice —
    /// nothing is collapsed here, so a caller can show the duplication.
    pub entries: Vec<EnvEntry>,
    pub problems: Vec<EnvProblem>,
}

impl EnvFile {
    /// The effective entry for `key`: the **last** assignment, which is what a
    /// shell sourcing the file would end up with.
    pub fn get(&self, key: &str) -> Option<&EnvEntry> {
        self.entries.iter().rev().find(|e| e.key == key)
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse the text of a `.env`-style file.
pub fn parse(text: &str) -> EnvFile {
    let mut file = EnvFile::default();
    // A UTF-8 BOM is invisible in an editor and would otherwise become the first
    // character of the first key.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    for (index, raw_line) in text.split('\n').enumerate() {
        let line_no = (index + 1) as u32;
        // `split('\n')` keeps the CR of a CRLF file; it belongs to no value.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim_start();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = strip_export(line);

        let Some((key_text, value_text)) = line.split_once('=') else {
            file.problems
                .push(problem(line_no, EnvProblemKind::NoAssignment));
            continue;
        };

        let key = key_text.trim();
        if key.is_empty() {
            file.problems
                .push(problem(line_no, EnvProblemKind::EmptyKey));
            continue;
        }
        if !is_valid_key(key) {
            file.problems
                .push(problem(line_no, EnvProblemKind::InvalidKey));
            continue;
        }

        match read_value(value_text) {
            Ok(text) => file.entries.push(EnvEntry {
                key: key.to_string(),
                value: classify(text),
                line: line_no,
            }),
            Err(kind) => file.problems.push(problem(line_no, kind)),
        }
    }

    file
}

/// Strip a leading `export` **only** when whitespace follows it, so a key named
/// `exported` keeps its first six letters.
fn strip_export(line: &str) -> &str {
    match line.strip_prefix("export") {
        Some(rest) if rest.starts_with([' ', '\t']) => rest.trim_start(),
        _ => line,
    }
}

/// `[A-Za-z_][A-Za-z0-9_.]*` — the portable shape of an environment variable
/// name, plus `.`, which .NET configuration keys use for nesting.
fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// The text right of the first `=`, unquoted and with a trailing comment
/// removed. Returns the problem *kind* rather than a message, so the caller owns
/// the wording.
fn read_value(text: &str) -> Result<String, EnvProblemKind> {
    let text = text.trim_start();
    let mut chars = text.chars();

    match chars.next() {
        Some('"') => {
            let mut out = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('r') => out.push('\r'),
                        Some('t') => out.push('\t'),
                        Some('\\') => out.push('\\'),
                        Some('"') => out.push('"'),
                        Some('\'') => out.push('\''),
                        // An escape this dialect does not define keeps its
                        // backslash: a Windows path is far likelier than an
                        // escape sequence nobody wrote.
                        Some(other) => {
                            out.push('\\');
                            out.push(other);
                        }
                        None => return Err(EnvProblemKind::UnterminatedQuote),
                    },
                    '"' => {
                        closed = true;
                        break;
                    }
                    other => out.push(other),
                }
            }
            if !closed {
                return Err(EnvProblemKind::UnterminatedQuote);
            }
            check_tail(chars.as_str())?;
            Ok(out)
        }
        Some('\'') => {
            let rest = chars.as_str();
            let Some(end) = rest.find('\'') else {
                return Err(EnvProblemKind::UnterminatedQuote);
            };
            check_tail(&rest[end + 1..])?;
            Ok(rest[..end].to_string())
        }
        // Unquoted. A `#` starts a comment only at the start or after
        // whitespace, so `PW=abc#def` keeps its `#` — a password is likelier
        // than a comment somebody forgot to space.
        _ => Ok(text[..comment_start(text).unwrap_or(text.len())]
            .trim_end()
            .to_string()),
    }
}

/// What may follow a closing quote: nothing, whitespace, or a comment.
fn check_tail(rest: &str) -> Result<(), EnvProblemKind> {
    let rest = rest.trim();
    if rest.is_empty() || rest.starts_with('#') {
        Ok(())
    } else {
        Err(EnvProblemKind::TrailingCharacters)
    }
}

fn comment_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len())
        .find(|&i| bytes[i] == b'#' && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t'))
}

/// Classify a value that arrived from somewhere other than a `.env` line — a
/// `ConnectionStrings` entry in an `appsettings.json`, say, or a string the
/// user typed.
///
/// The unresolved-reference rule in the module docs is not about `.env` files;
/// it is about *substitution this process cannot see*, and `${DB_HOST}` in an
/// `appsettings.json` means exactly what it means in a `.env`. So
/// [`crate::sql::discover`] classifies through here rather than re-deriving the
/// rule, and there stays one description of it.
///
/// Only the classification is shared: quoting, escapes and comments are `.env`
/// syntax and are not applied to text that never came from one.
pub fn classify_value(text: String) -> EnvValue {
    classify(text)
}

/// Which of the three reference syntaxes was found.
///
/// A *class*, not the matched text. The match itself is a fragment read out of a
/// value — a password containing a `$` is enough to make the eager detector fire
/// on part of that password — and rule 3 of the subsystem docs forbids it
/// crossing into a `reason`, which [`crate::sql::discover`] copies into a
/// `Serialize` field that reaches the UI. The variable *name* is excluded for the
/// same reason: it is still text read out of the value, so only the shape
/// crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefSyntax {
    /// `${NAME}`
    DollarBrace,
    /// `$NAME`
    Dollar,
    /// `%NAME%`
    Percent,
}

impl RefSyntax {
    /// The syntax as a literal, written out here rather than sliced out of the
    /// input, so nothing can turn this back into value text.
    fn shape(self) -> &'static str {
        match self {
            RefSyntax::DollarBrace => "${NAME}",
            RefSyntax::Dollar => "$NAME",
            RefSyntax::Percent => "%NAME%",
        }
    }

    /// How the syntax is described in prose. Three descriptions, so the three
    /// outcomes stay distinguishable to a reader.
    fn description(self) -> &'static str {
        match self {
            RefSyntax::DollarBrace => "a dollar-brace reference",
            RefSyntax::Dollar => "a bare dollar reference",
            RefSyntax::Percent => "a percent-delimited reference",
        }
    }
}

/// Turn a read value into [`EnvValue::Literal`] or [`EnvValue::Unresolved`].
fn classify(text: String) -> EnvValue {
    match find_reference(&text) {
        Some(syntax) => EnvValue::Unresolved {
            reason: format!(
                "contains {} (`{}`), which this file leaves for something else to \
                 substitute. It is shown as written rather than expanded, because \
                 expanding it here could name a different server than the one intended.",
                syntax.description(),
                syntax.shape()
            ),
            raw: text,
        },
        None => EnvValue::Literal { text },
    }
}

/// The first thing *shaped* like a variable reference: `${NAME}`, `$NAME` or
/// `%NAME%`. Shape only — nothing is ever looked up, and only the shape is
/// returned, never the match: see [`RefSyntax`].
fn find_reference(text: &str) -> Option<RefSyntax> {
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                if text[i + 2..].contains('}') {
                    return Some(RefSyntax::DollarBrace);
                }
            }
            b'$' if i + 1 < bytes.len() && is_name_start(bytes[i + 1]) => {
                return Some(RefSyntax::Dollar);
            }
            b'%' if i + 1 < bytes.len() && is_name_start(bytes[i + 1]) => {
                let end = name_end(bytes, i + 1);
                if end < bytes.len() && bytes[end] == b'%' {
                    return Some(RefSyntax::Percent);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn name_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn problem(line: u32, kind: EnvProblemKind) -> EnvProblem {
    let reason = match kind {
        EnvProblemKind::NoAssignment => {
            format!("line {line} is not a comment and has no `=`, so it names no variable")
        }
        EnvProblemKind::EmptyKey => format!("line {line} has nothing before its `=`"),
        EnvProblemKind::InvalidKey => format!(
            "the name before `=` on line {line} is not a valid variable name (letters, digits, \
             `_` and `.`, not starting with a digit)"
        ),
        EnvProblemKind::UnterminatedQuote => {
            format!("the quoted value on line {line} is never closed")
        }
        EnvProblemKind::TrailingCharacters => format!(
            "the quoted value on line {line} is followed by more text, so where it ends is \
             unclear"
        ),
    };
    EnvProblem { line, kind, reason }
}

#[cfg(test)]
#[path = "dotenv_tests.rs"]
mod tests;
