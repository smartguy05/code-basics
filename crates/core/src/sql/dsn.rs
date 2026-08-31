//! Parsing a connection string into the parts a driver needs, and — just as
//! importantly — producing the redacted description that is the *only* form
//! allowed to leave this module.
//!
//! A password is never echoed, not in a log, not in an error. A string that
//! cannot be parsed produces an error naming the problem and the position,
//! never the input text.
//!
//! # Three rules, in decreasing order of how easy they are to get wrong
//!
//! **1. A half-redacted secret is a leaked secret.** The hard case is an
//! *unquoted* value containing a `;`. ADO.NET says a value containing `;`, `'`
//! or `"` must be quoted, but nothing enforces that, and a driver error is
//! prose with a DSN inside it rather than a well-formed connection string. So
//! the end of an unquoted password value is only recognised where it can be
//! *proved*:
//!
//! - a `;` **followed by a known connection-string keyword and an `=`** — the
//!   only positive evidence that the value has ended;
//! - a newline, which no value can contain;
//! - a `'` or `"` **that closes a quoted span opened before the field** — the
//!   shape a driver error takes when it embeds a whole DSN in quotes, which
//!   keeps the surrounding sentence readable;
//! - otherwise, the end of the text.
//!
//! A *bare* quote is not a terminator. ADO.NET says an unquoted value may not
//! contain one, but nothing enforces that and the user picks the password, so
//! `Password=ab'cd` is possible and reading the quote as the end emitted
//! `Password=***'cd`. Half a secret is a leaked secret.
//!
//! That last clause is the abstention: `Password=s3cr3t and then some prose`
//! elides the prose too. Losing a sentence of a driver error is a cost; leaking
//! `s3cr3t` because `and` looked like a word boundary is not a cost this module
//! is allowed to pay. [`redact`] therefore over-redacts and never
//! under-redacts.
//!
//! **1b. A labelled secret is redacted wherever it appears, not only where a
//! connection string would put it.** [`redact`] used to walk the top-level
//! `key=value` structure, which meant a `Password=` *inside* another field's
//! value was never looked at: sqlx echoes the unknown *value* of a known
//! parameter verbatim, so `sqlite:app.db?immutable=Password=hunter2` produced
//! an error carrying `hunter2` intact — the third time this module leaked
//! through a shape its author had not anticipated (the other two: a quote
//! terminating a value early, and a URL password containing `/`).
//!
//! Widening the list of shapes a fourth time would have bought the same
//! guarantee for one more shape, so the *guarantee* moved instead. [`redact`]
//! now scans `=` signs rather than fields ([`password_spans`]): a `=` whose
//! preceding text ends with a password keyword is a password field, at any
//! nesting depth, inside any value, in prose or in a DSN. Structure no longer
//! has to be understood for a labelled secret to be found, and where the value
//! end still cannot be proved the remainder is elided as rule 1 requires.
//!
//! **What this does and does not promise.** It promises that no value
//! *introduced by a password keyword* survives, whatever surrounds it. It
//! cannot promise that text which is a secret but is not labelled as one is
//! removed — `?vfs=hunter2`, where the user's password is the value of a
//! parameter that is not a credential, is indistinguishable from a legitimate
//! VFS name, and nothing short of eliding every value could catch it.
//!
//! The line is drawn at the *keyword*, not at the position, and the vocabulary
//! stays exactly `password`/`pwd` ([`is_password_key`], shared with
//! [`display_form`]'s auth detection so the two can never disagree about what
//! a credential is). Widening it further trades readability for a shrinking
//! return, and an error redacted into `***` is undiagnosable — which is its own
//! harm, and the reason this module elides a *tail* it cannot parse rather than
//! the whole message.
//!
//! **2. [`sniff_engine`] abstains on ambiguity, not just on ignorance.** A
//! string carrying evidence for two engines yields [`None`], exactly like a
//! string carrying evidence for none. There is no default and no "probably SQL
//! Server": guessing here points a query at the wrong driver.
//!
//! **3. [`display_form`] is built from a whitelist, never by removal.** It
//! copies out only the four things it names — engine, server, database, auth
//! mode — so a password cannot survive by being unrecognised. Where the parse
//! is not provable (a `;`-bearing password swallowing the remainder of the
//! string), the fields it could not prove come back as [`None`] rather than
//! carved out of a secret.
//!
//! And where the parse is not merely incomplete but *untrustworthy*, it refuses
//! outright: [`SqlDisplayConfidence::Refused`] is a third answer beside "here is
//! the description" and "this is not a connection string". A repeated key means
//! one of the two pairs was read out of the inside of a value — that is how
//! `Password=pa;server=fragment;Server=real` came back reporting `fragment` as
//! the host — and a URL authority carrying a `user:password` with no `@` to end
//! it is the same problem in the other syntax. Neither is describable, so
//! neither is described.

use serde::{Deserialize, Serialize};
use specta::Type;

/// What replaces a secret. Fixed-width and value-independent: a redaction whose
/// length tracked the password would leak the password's length.
pub const REDACTED: &str = "***";

/// The database engines the SQL console can speak to.
///
/// Defined here rather than in [`crate::sql::model`] because sniffing is what
/// produces one; if `model` later needs it, it should re-export this rather
/// than declare a second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SqlEngine {
    Postgres,
    SqlServer,
    Sqlite,
}

/// How a connection string says it authenticates.
///
/// Four answers, not two: "the string declares no credentials" (a SQLite file)
/// and "this could not be read as a connection string at all" are different
/// facts, and collapsing them would let an unparsed string look like a
/// deliberately credential-free one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SqlAuthMode {
    /// Windows / integrated authentication was requested.
    Integrated,
    /// A password is present in the string. The password itself never leaves.
    Password,
    /// Parsed, and it states no credentials.
    NoneStated,
    /// Not recognisable as a connection string.
    Unknown,
}

/// Whether the string could be described at all.
///
/// A third answer beside the fields themselves. `Refused` and an all-[`None`]
/// `Described` are different facts: the second says "read, and it states
/// nothing"; the first says "read far enough to know that anything reported
/// might be a slice of the password". Collapsing them would let a refusal look
/// like an empty connection string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SqlDisplayConfidence {
    /// The fields below are what the string says.
    Described,
    /// The string could not be parsed with enough certainty to describe. Every
    /// other field is empty — not because the string was empty, but because
    /// nothing in it could be quoted safely.
    Refused,
}

/// The redacted view of a connection string — the **only** form allowed to
/// cross IPC.
///
/// Every field is copied from a whitelisted source; there is no field that
/// could carry a password even if the parse went wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlConnectionDisplay {
    /// [`None`] when the engine could not be determined *or* was ambiguous.
    pub engine: Option<SqlEngine>,
    /// Host (and port, when stated). [`None`] for a file-backed database.
    pub server: Option<String>,
    /// Database name, or the file path for SQLite.
    pub database: Option<String>,
    pub auth_mode: SqlAuthMode,
    pub confidence: SqlDisplayConfidence,
}

// ---------------------------------------------------------------------------
// Engine sniffing
// ---------------------------------------------------------------------------

/// Decide which engine a connection string is for, or abstain.
///
/// Yields [`None`] for a string that matches nothing **and** for one that
/// matches more than one engine. Never a default.
pub fn sniff_engine(s: &str) -> Option<SqlEngine> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();

    let mut found: Vec<SqlEngine> = Vec::new();

    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        found.push(SqlEngine::Postgres);
    } else if lower.starts_with("sqlite:") {
        found.push(SqlEngine::Sqlite);
    } else if !lower.contains("://") {
        let pairs = key_values(t);
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };

        // Npgsql's canonical spelling.
        if get("host").is_some() {
            found.push(SqlEngine::Postgres);
        }
        // SqlClient-only keywords.
        if get("initial catalog").is_some() || get("trusted_connection").is_some() {
            found.push(SqlEngine::SqlServer);
        }
        // `Server=` alone is spoken by several drivers; paired with `Database=`
        // it is the ordinary SqlClient form.
        if get("server").is_some() && get("database").is_some() {
            found.push(SqlEngine::SqlServer);
        }
        // SQL Server also uses `Data Source=`, so only a recognised SQLite file
        // counts here.
        if get("data source").is_some_and(looks_like_sqlite_file) {
            found.push(SqlEngine::Sqlite);
        }
    }

    found.dedup();
    match found.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// A `Data Source=` value that is unambiguously a SQLite database file.
fn looks_like_sqlite_file(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    if v == ":memory:" {
        return true;
    }
    [".db", ".db3", ".sqlite", ".sqlite3"]
        .iter()
        .any(|ext| v.ends_with(ext))
}

// ---------------------------------------------------------------------------
// The redacted display form
// ---------------------------------------------------------------------------

/// Build the redacted description of a connection string.
///
/// Whitelist-based: only engine, server, database and auth mode are copied out.
/// A field that could not be *proved* — because a `;`-bearing password swallowed
/// the rest of the string — comes back as [`None`], and a parse that cannot be
/// trusted at all comes back as [`SqlDisplayConfidence::Refused`] with nothing
/// in it.
pub fn display_form(s: &str) -> SqlConnectionDisplay {
    let t = s.trim();
    let engine = sniff_engine(t);

    if let Some((scheme, rest)) = t.split_once("://") {
        return display_from_url(engine, &scheme.to_ascii_lowercase(), rest);
    }

    let pairs = key_values(t);
    if has_repeated_key(&pairs) {
        // A well-formed connection string states each keyword once. A repeat is
        // evidence that one of the two pairs was parsed out of the *inside* of a
        // value — `Password=pa;server=fragment;Server=real` yielded
        // `server: Some("fragment")`, a slice of the password rendered as the
        // host — and there is no telling which of the two the author wrote. So
        // the whole description is refused rather than half-filled.
        return refused();
    }
    if pairs.is_empty() {
        return SqlConnectionDisplay {
            engine,
            server: None,
            database: None,
            auth_mode: SqlAuthMode::Unknown,
            confidence: SqlDisplayConfidence::Described,
        };
    }

    let get = |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    let data_source = get("data source");
    let sqlite = engine == Some(SqlEngine::Sqlite);

    // For SQLite the `Data Source=` value is the database *file*, not a host.
    let (ds_server, ds_database) = if sqlite {
        (None, data_source)
    } else {
        (data_source, None)
    };
    let server = get("server").or_else(|| get("host")).or(ds_server);
    let database = get("database")
        .or_else(|| get("initial catalog"))
        .or(ds_database);

    let has_password = pairs
        .iter()
        .any(|(k, v)| is_password_key(k) && !v.is_empty());
    let integrated = pairs.iter().any(|(k, v)| {
        matches!(k.as_str(), "trusted_connection" | "integrated security") && is_truthy(v)
    });
    let auth_mode = if has_password {
        SqlAuthMode::Password
    } else if integrated {
        SqlAuthMode::Integrated
    } else {
        SqlAuthMode::NoneStated
    };

    SqlConnectionDisplay {
        engine,
        server: non_empty(server),
        database: non_empty(database),
        auth_mode,
        confidence: SqlDisplayConfidence::Described,
    }
}

fn display_from_url(engine: Option<SqlEngine>, scheme: &str, rest: &str) -> SqlConnectionDisplay {
    // Everything from `?`/`#` on is driver options, not identity.
    let body = rest.split(['?', '#']).next().unwrap_or("");

    if scheme == "sqlite" {
        return SqlConnectionDisplay {
            engine,
            server: None,
            database: non_empty(Some(body.to_string())),
            auth_mode: SqlAuthMode::NoneStated,
            confidence: SqlDisplayConfidence::Described,
        };
    }

    let (authority, path) = match body.find('/') {
        Some(k) => (&body[..k], &body[k + 1..]),
        None => (body, ""),
    };

    // The *last* `@` ends the userinfo: a password containing an unencoded `@`
    // would otherwise leave its tail inside the host.
    let (userinfo, host) = match authority.rfind('@') {
        Some(k) => (&authority[..k], &authority[k + 1..]),
        None => ("", authority),
    };

    if userinfo.is_empty() {
        if let Some(colon) = host.find(':') {
            let seg = &host[colon + 1..];
            if !(seg.is_empty() || seg.bytes().all(|b| b.is_ascii_digit())) {
                // No `@`, and what follows the colon is not a port: this is a
                // `user:password` whose end could not be established (an
                // unencoded `/` in the password does exactly this). Emitting
                // `app:pa` as the server would publish a fragment of it.
                return refused();
            }
        }
    }

    let auth_mode = match userinfo.split_once(':') {
        Some((_, pw)) if !pw.is_empty() => SqlAuthMode::Password,
        _ => SqlAuthMode::NoneStated,
    };

    SqlConnectionDisplay {
        engine,
        server: non_empty(Some(host.to_string())),
        database: non_empty(path.split('/').next().map(str::to_string)),
        auth_mode,
        confidence: SqlDisplayConfidence::Described,
    }
}

/// The one answer that is neither a description nor "not a connection string":
/// it *is* one, and it could not be read with enough certainty to describe.
fn refused() -> SqlConnectionDisplay {
    SqlConnectionDisplay {
        engine: None,
        server: None,
        database: None,
        auth_mode: SqlAuthMode::Unknown,
        confidence: SqlDisplayConfidence::Refused,
    }
}

/// Does any key occur twice? Aliases (`server` and `host`) are different keys
/// and are [`sniff_engine`]'s problem, not this one.
fn has_repeated_key(pairs: &[(String, String)]) -> bool {
    pairs
        .iter()
        .enumerate()
        .any(|(i, (k, _))| pairs[..i].iter().any(|(prev, _)| prev == k))
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "sspi" | "1"
    )
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

/// Scrub connection-string passwords out of arbitrary text.
///
/// Driver errors routinely embed the whole DSN, so every error passes through
/// here before becoming a `SqlEvent::Failed`. Two forms are handled: a
/// `Password=`/`Pwd=` keyword (any casing, any spacing around the `=`, **at any
/// position in the text**), and a URL userinfo password
/// (`postgres://user:pw@host/db`).
///
/// Where the end of an unquoted value cannot be proved, the remainder of the
/// text is elided rather than half-redacted — see the module docs, which also
/// state exactly what this function does and does not guarantee.
pub fn redact(text: &str) -> String {
    let staged = redact_url_userinfo(text);

    let mut out = String::with_capacity(staged.len());
    let mut cursor = 0usize;
    for (value_start, value_end) in password_spans(&staged) {
        if value_start < cursor {
            continue;
        }
        out.push_str(&staged[cursor..value_start]);
        out.push_str(REDACTED);
        cursor = value_end;
    }
    out.push_str(&staged[cursor..]);
    out
}

/// Byte spans of every password *value* in `text`, in order.
///
/// This walks `=` signs rather than fields, which is the whole difference
/// between it and [`scan_fields`]: a field parse only ever reaches the top
/// level, so a `Password=` sitting **inside** another field's value was never
/// looked at. `sqlite:app.db?immutable=Password=hunter2` is one real message
/// of that shape — sqlx echoes the unknown *value* of a known parameter
/// verbatim — and it crossed into a `SqlEvent::Failed` intact.
///
/// A `=` is a password field when the text before it *ends with* a password
/// keyword at a word boundary ([`is_password_key`], the same predicate the
/// field parse uses). Nothing about the surrounding structure has to be
/// understood for that to hold, which is the point: there is no nesting depth
/// at which a labelled secret becomes invisible.
///
/// The value end is decided exactly as before — [`quoted_value_end`] for a
/// quoted value, [`password_value_end`] (which elides to the end of the text
/// when no end can be proved) otherwise.
fn password_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let len = text.len();
    let mut spans = Vec::new();
    let mut i = 0usize;

    while i < len {
        if bytes[i] != b'=' || !ends_with_password_key(&text[..i]) {
            i += 1;
            continue;
        }
        let mut value_start = i + 1;
        while value_start < len && matches!(bytes[value_start], b' ' | b'\t') {
            value_start += 1;
        }
        let value_end = if value_start < len && matches!(bytes[value_start], b'"' | b'\'') {
            quoted_value_end(text, value_start)
        } else {
            password_value_end(text, value_start)
        };
        spans.push((value_start, value_end));
        i = value_end.max(i + 1);
    }

    spans
}

/// Does the text immediately before an `=` end with a password keyword?
///
/// Only the tail can match, so only the tail is examined — a fixed window wide
/// enough for the longest keyword plus the boundary character in front of it,
/// which also keeps this from re-lowercasing the whole message at every `=`.
fn ends_with_password_key(prefix: &str) -> bool {
    /// `password` is 8 characters; the window covers it, the boundary
    /// character before it, and room to spare.
    const WINDOW: usize = 16;
    let trimmed = prefix.trim_end();
    let tail_start = trimmed
        .char_indices()
        .rev()
        .take(WINDOW)
        .last()
        .map_or(trimmed.len(), |(k, _)| k);
    is_password_key(&trimmed[tail_start..])
}

/// Replace the password in every `scheme://user:password@host` authority.
///
/// The scan runs over the whole URL **token** — up to whitespace, a quote, a
/// comma or a closing paren — and not up to the first `/`. Stopping at the `/`
/// meant an unencoded `/` inside the userinfo ended the authority before the
/// `@`, so no password was found at all and `postgres://app:pa/ss@host/db`
/// crossed through intact.
fn redact_url_userinfo(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;

    while let Some(pos) = text[i..].find("://") {
        let auth_start = i + pos + 3;
        let mut j = auth_start;
        while j < text.len() {
            let c = bytes[j];
            if matches!(c, b'"' | b'\'' | b',' | b')') || (c as char).is_ascii_whitespace() {
                break;
            }
            j += 1;
        }
        match userinfo_password_span(&text[auth_start..j]) {
            Some((from, to)) => {
                out.push_str(&text[i..auth_start + from]);
                out.push_str(REDACTED);
                out.push_str(&text[auth_start + to..j]);
            }
            None => out.push_str(&text[i..j]),
        }
        i = j;
    }

    out.push_str(&text[i..]);
    out
}

/// Byte span of the password inside a URL token (everything after the `://`).
///
/// With an `@` present the password is provably `colon + 1 .. last @`. Without
/// one there is no proof of where it ends, so the only two safe answers are
/// "that colon introduces a port, so there is no secret here" and "elide the
/// rest of the token".
fn userinfo_password_span(token: &str) -> Option<(usize, usize)> {
    if let Some(at) = token.rfind('@') {
        let colon = token[..at].find(':')?;
        return Some((colon + 1, at));
    }
    // A token starting with `/` is a path, not an authority (`sqlite:///a/b`).
    if token.starts_with('/') {
        return None;
    }
    let colon = token.find(':')?;
    let seg = token[colon + 1..]
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if seg.is_empty() || seg.bytes().all(|b| b.is_ascii_digit()) {
        return None; // `host:5432` — a port, and a port is not a secret.
    }
    Some((colon + 1, token.len()))
}

// ---------------------------------------------------------------------------
// The shared scanner
// ---------------------------------------------------------------------------

/// One `key=value` occurrence, located in the source text.
struct Field {
    key: String,
    /// Byte range of the raw value, quotes included.
    value_start: usize,
    value_end: usize,
    quoted: bool,
}

/// Walk `text` collecting `key=value` fields.
///
/// Tolerant by design: it runs over prose as happily as over a connection
/// string, because a driver error is prose with a DSN inside it. The only
/// subtlety is where a value ends, and for a password that question is answered
/// conservatively — see [`password_value_end`].
///
/// This is a *structural* parse and reaches only the top level, which is why
/// [`redact`] does not use it: a `Password=` inside another field's value is
/// invisible here. It still recognises password keys, because a password value
/// that swallows the rest of the string must yield no further pairs — that is
/// what makes [`display_form`] abstain rather than report a fragment.
fn scan_fields(text: &str) -> Vec<Field> {
    let bytes = text.as_bytes();
    let len = text.len();
    let mut fields = Vec::new();
    let mut i = 0usize;

    while i < len {
        while i < len && (bytes[i] == b';' || (bytes[i] as char).is_ascii_whitespace()) {
            i += 1;
        }
        if i >= len {
            break;
        }

        let key_start = i;
        while i < len && !matches!(bytes[i], b'=' | b';' | b'\n' | b'\r') {
            i += 1;
        }
        if i >= len || bytes[i] != b'=' {
            // A token with no `=` is not a field. Whatever stopped us is
            // consumed by the skip loop on the next pass.
            continue;
        }
        let key = text[key_start..i].trim().to_ascii_lowercase();
        i += 1;

        while i < len && matches!(bytes[i], b' ' | b'\t') {
            i += 1;
        }
        let value_start = i;
        let is_password = is_password_key(&key);

        let (value_end, quoted) = if value_start < len && matches!(bytes[value_start], b'"' | b'\'')
        {
            (quoted_value_end(text, value_start), true)
        } else if is_password {
            (password_value_end(text, value_start), false)
        } else {
            let mut j = value_start;
            while j < len && !matches!(bytes[j], b';' | b'\n' | b'\r') {
                j += 1;
            }
            (j, false)
        };

        fields.push(Field {
            key,
            value_start,
            value_end,
            quoted,
        });
        i = value_end;
    }

    fields
}

/// End of a quoted value: the byte after the closing quote. A doubled quote is
/// a literal quote, as ADO.NET specifies. An unterminated quote consumes the
/// rest of the text — the safe reading, since the alternative is guessing where
/// the author meant it to close.
fn quoted_value_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let len = text.len();
    let q = bytes[start];
    let mut j = start + 1;
    while j < len {
        if bytes[j] == q {
            if j + 1 < len && bytes[j + 1] == q {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    len
}

/// End of an **unquoted password** value — the one place a wrong answer leaks a
/// secret, so only a provable end is accepted.
///
/// A `;` ends the value **only** when what follows positively identifies itself
/// as another connection-string key. A newline ends it because no value can
/// contain one. A quote ends it **only when it closes a quoted span that opened
/// before this field** — the shape a driver error takes when it embeds a whole
/// DSN in quotes. Everything else runs to the end of the text, eliding whatever
/// follows.
///
/// The quote rule is narrower than it first looks, and deliberately so. ADO.NET
/// says an unquoted value may not contain a quote, but nothing enforces that and
/// it is the *user* who picks the password: `Password=ab@cd` with a quote for
/// the `@` is a perfectly possible string, and reading that quote as a
/// terminator emitted `Password=***` followed by the rest of the secret — a
/// partially redacted secret, which is a leaked secret. With no opening quote to
/// pair it with there is no proof either way, so the value runs to the end of
/// the text and the tail is elided whole.
fn password_value_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let len = text.len();
    let before = &bytes[..start];
    let mut j = start;
    while j < len {
        match bytes[j] {
            b'\n' | b'\r' => return j,
            c @ (b'"' | b'\'') => {
                return if before.contains(&c) { j } else { len };
            }
            b';' if starts_known_key(&text[j + 1..]) => return j,
            _ => j += 1,
        }
    }
    len
}

/// Does `rest` begin with a known connection-string keyword followed by `=`?
///
/// This is the positive evidence [`password_value_end`] requires. An unknown
/// keyword is *not* evidence: it is far likelier to be part of a password than
/// a key nobody has heard of.
fn starts_known_key(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    let mut i = 0usize;
    while i < rest.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    let start = i;
    while i < rest.len() && !matches!(bytes[i], b'=' | b';' | b'\n' | b'\r') {
        if i - start > 40 {
            return false;
        }
        i += 1;
    }
    if i >= rest.len() || bytes[i] != b'=' {
        return false;
    }
    let key = rest[start..i].trim().to_ascii_lowercase();
    KNOWN_KEYS.contains(&key.as_str())
}

/// Is this key a password key?
///
/// Matches `password` and `pwd` exactly, and as a suffix behind a non-word
/// character — so a driver error's `connection pwd` is caught while a distinct
/// setting named `mypassword` is not.
fn is_password_key(key: &str) -> bool {
    let k = key.trim().to_ascii_lowercase();
    for suffix in ["password", "pwd"] {
        if k == suffix {
            return true;
        }
        if let Some(prefix) = k.strip_suffix(suffix) {
            match prefix.chars().next_back() {
                Some(c) if c.is_alphanumeric() || c == '_' => {}
                _ => return true,
            }
        }
    }
    false
}

/// Parse a connection string into `(key, value)` pairs, keys lowercased and
/// values unquoted.
///
/// Uses the same scanner as [`redact`], so a password that swallowed the rest of
/// the string yields *no further pairs* — which is what makes [`display_form`]
/// abstain instead of reporting a database name carved out of a secret.
fn key_values(text: &str) -> Vec<(String, String)> {
    scan_fields(text)
        .into_iter()
        .map(|f| {
            let raw = &text[f.value_start..f.value_end];
            let value = if f.quoted {
                let q = raw.chars().next().unwrap_or('"');
                let doubled = format!("{q}{q}");
                let inner = raw.trim_start_matches(q);
                let inner = inner.strip_suffix(q).unwrap_or(inner);
                inner.replace(&doubled, &q.to_string())
            } else {
                raw.trim().to_string()
            };
            (f.key, value)
        })
        .collect()
}

/// Connection-string keywords recognised across the three engines.
///
/// Used **only** as evidence that a `;` ended a value, never to validate a
/// string: an unlisted keyword makes this module more conservative, not less.
const KNOWN_KEYS: &[&str] = &[
    "application name",
    "applicationname",
    "async",
    "attachdbfilename",
    "authentication",
    "cache",
    "channel binding",
    "client encoding",
    "column encryption setting",
    "command timeout",
    "connect timeout",
    "connection idle lifetime",
    "connection lifetime",
    "connection timeout",
    "current language",
    "data source",
    "database",
    "default timeout",
    "encoding",
    "encrypt",
    "enlist",
    "failover partner",
    "foreign keys",
    "host",
    "include error detail",
    "initial catalog",
    "integrated security",
    "journal mode",
    "keepalive",
    "max pool size",
    "min pool size",
    "mode",
    "multipleactiveresultsets",
    "multisubnetfailover",
    "network library",
    "no reset on close",
    "options",
    "packet size",
    "password",
    "persist security info",
    "pooling",
    "port",
    "pragma",
    "pwd",
    "read only",
    "recursive triggers",
    "replication",
    "root certificate",
    "search path",
    "server",
    "ssl certificate",
    "ssl key",
    "ssl mode",
    "sslmode",
    "target session attrs",
    "tcp keepalive",
    "timeout",
    "trustservercertificate",
    "trusted_connection",
    "type system version",
    "uid",
    "user",
    "user id",
    "username",
    "version",
    "workstation id",
];

#[cfg(test)]
#[path = "dsn_tests.rs"]
mod tests;
