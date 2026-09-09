//! The PostgreSQL driver, over `sqlx`.
//!
//! Modelled on [`super::sqlite`] — same shape, same three-answer stop handling,
//! same connect deadline, same two caps charged at the value's true source
//! size. What differs is worth stating, because each difference is a rule
//! rather than a preference.
//!
//! 1. **Read-only is enforced by the *server*, not by the handle.** SQLite
//!    opens the file `SQLITE_OPEN_READONLY`; PostgreSQL has no such handle
//!    mode, but it does have a real session setting, so this driver issues
//!    [`SET_SESSION_READ_ONLY`] and the server then refuses a write before it
//!    runs. That is [`ReadOnlyEnforcement::ServerSession`], which is as strong
//!    as SQLite's and is a *different mechanism*, so it gets its own wording.
//!
//!    The setting is **read back** ([`SHOW_SESSION_READ_ONLY`]) and the connect
//!    fails if the server does not confirm it. `ServerSession` may only be
//!    reported once the setting was accepted — continuing as though it had been
//!    would downgrade a promise the user already consented to, silently. It is
//!    also asked for a second time in the startup options
//!    (`default_transaction_read_only=on`), so the very first statement on the
//!    connection is already covered.
//!
//! 2. **Postgres uses the binary wire protocol**, and this build has no
//!    `chrono`/`uuid`/`bigdecimal` sqlx features, so nothing decodes itself.
//!    Every value is therefore decoded here from the bytes the server sent, by
//!    a table keyed on the type name ([`decoder_for`]). The policy is the one
//!    [`super::sqlite`] established and is not negotiable: **what this build
//!    can decode, it decodes; what it cannot becomes
//!    [`Cell::Unsupported`] naming the type, never a blank cell that reads as
//!    `NULL` and never a guess.** Arrays, ranges, geometric and network types,
//!    `money` (whose rendering depends on the server's locale), `timetz` (whose
//!    binary zone field has a sign convention that cannot be verified without a
//!    server) and every user-defined type abstain, by name.
//!
//!    Both wire formats are handled, and the *same* type map chooses the
//!    decoder for each — a type is decodable or it is not, and how the value
//!    happened to arrive does not change that answer. The binary format is what
//!    a prepared statement returns and is what this driver actually uses; the
//!    text arm exists because [`sqlx::postgres::PgValueFormat`] is data on the
//!    value, and reading text bytes as if they were binary would not fail
//!    loudly — it would produce a wrong number.
//!
//! 3. **`numeric` crosses as a string.** `0.10` is not `0.1` to a ledger and
//!    a `numeric` routinely carries more precision than a double, so the scale
//!    is part of the value: [`render_numeric`] renders Postgres' own base-10000
//!    representation to text and it reaches the wire as
//!    [`crate::sql::model::SqlValue::Number`] without ever being a float. Same
//!    for `float4`, which is rendered at *its own* precision — widening
//!    `0.1f32` to an `f64` and printing that yields `0.10000000149011612`,
//!    a number the database does not hold.
//!
//! 4. **`rows_affected` is reported, unlike SQLite's.** SQLite abstains because
//!    `sqlite3_changes()` may describe an earlier statement. Postgres sends a
//!    `CommandComplete` tag for *this* statement, so the count is attributable
//!    and is reported — but only when the loop actually reached it. A run that
//!    stopped early, or that a cap ended, leaves it [`None`]: *no row count to
//!    report* is a different answer from `Some(0)`.

use std::pin::Pin;
use std::str::FromStr;
use std::time::{Duration, Instant};

use futures_core::Stream;
use sqlx::postgres::{PgConnectOptions, PgRow, PgSslMode, PgValueFormat, PgValueRef};
use sqlx::{Column, ConnectOptions, Connection, Executor, PgConnection, Row, Statement};
use sqlx::{Either, TypeInfo, ValueRef};
use tokio::sync::mpsc;

use super::caps::{self, charged_row_bytes, CapTracker, Limits};
use super::value::to_wire;
use super::{
    ConnectSpec, DriverError, DriverFuture, DriverResult, ReadOnlyEnforcement, SqlConnection,
    SqlDriver, StatementOutcome, StopSignal, DEFAULT_CONNECT_TIMEOUT,
};
use crate::sql::dsn::SqlEngine;
use crate::sql::format::{format_cell, Cell};
use crate::sql::model::{SqlColumn, SqlEvent, SqlResultSet};

/// How many rows are batched into one [`SqlEvent::Rows`]. Same reasoning as
/// [`super::sqlite::ROW_BATCH`].
pub const ROW_BATCH: usize = 100;

/// What is sent to hold the session read-only.
///
/// This is a real server-side guarantee: within a read-only transaction the
/// server refuses `INSERT`/`UPDATE`/`DELETE`/DDL before executing them.
pub const SET_SESSION_READ_ONLY: &str = "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY";

/// What is asked afterwards to confirm the setting took.
///
/// Sending the `SET` is not the same fact as the session *being* read-only, and
/// [`ReadOnlyEnforcement::ServerSession`] is a claim about the second.
pub const SHOW_SESSION_READ_ONLY: &str = "SHOW default_transaction_read_only";

/// The only answer to [`SHOW_SESSION_READ_ONLY`] that permits the connect to
/// succeed with writes disallowed.
pub const READ_ONLY_CONFIRMATION: &str = "on";

/// Days from the Unix epoch (1970-01-01) to the PostgreSQL epoch (2000-01-01).
const PG_EPOCH_DAYS_FROM_UNIX: i64 = 10_957;

const MICROS_PER_DAY: i64 = 86_400_000_000;

// ---------------------------------------------------------------------------
// The connection target (pure)
// ---------------------------------------------------------------------------

/// Which wire format a value arrived in.
///
/// A local mirror of [`sqlx::postgres::PgValueFormat`] so the whole decoding
/// path is a pure function of `(type name, format, bytes)` and is provable with
/// no server: a `PgValueRef` cannot be constructed outside sqlx.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// What a prepared statement returns, and what this driver uses.
    Binary,
    /// What the simple query protocol returns.
    Text,
}

/// Why a connection string could not be resolved to a PostgreSQL server.
///
/// No variant carries any of the input: a Postgres DSN holds a password, and an
/// error message is exactly the leak the subsystem docs forbid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgTargetError {
    /// Nothing but whitespace.
    Empty,
    /// It is neither a `postgres://` URL nor a keyword string naming a server.
    NoHostNamed,
    /// A `Port=` keyword whose value is not a TCP port.
    BadPort,
    /// An `Ssl Mode=` keyword this build does not recognise. Refused rather
    /// than defaulted, because the default is weaker than every spelling the
    /// user might have meant — a typo in `verify-full` must not silently become
    /// an unverified connection.
    UnknownSslMode,
}

impl PgTargetError {
    pub fn message(self) -> &'static str {
        match self {
            PgTargetError::Empty => "The connection string is empty.",
            PgTargetError::NoHostNamed => {
                "The connection string names no PostgreSQL server. Expected a `postgres://` URL \
                 or a `Host=`, `Server=`, or `Data Source=` keyword."
            }
            PgTargetError::BadPort => {
                "The `Port=` value is not a TCP port number between 1 and 65535."
            }
            PgTargetError::UnknownSslMode => {
                "The `Ssl Mode=` value is not one this build recognises. Expected disable, allow, \
                 prefer, require, verify-ca or verify-full."
            }
        }
    }
}

/// How TLS is negotiated.
///
/// This crate's own enum rather than [`sqlx::postgres::PgSslMode`], which
/// implements neither `PartialEq` nor `Eq` — so a resolved target could not be
/// compared, and the one thing worth pinning about this field is exactly that
/// a spelling maps to the mode the user meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    Disable,
    Allow,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    /// The sqlx spelling. A total mapping, so a mode cannot be added here
    /// without being answered.
    pub fn to_sqlx(self) -> PgSslMode {
        match self {
            SslMode::Disable => PgSslMode::Disable,
            SslMode::Allow => PgSslMode::Allow,
            SslMode::Prefer => PgSslMode::Prefer,
            SslMode::Require => PgSslMode::Require,
            SslMode::VerifyCa => PgSslMode::VerifyCa,
            SslMode::VerifyFull => PgSslMode::VerifyFull,
        }
    }
}

/// The fields an Npgsql-style keyword string can name.
///
/// `Debug` is hand-written: a derived one prints the password into every `{:?}`,
/// every failed assertion and every `tracing` line.
#[derive(Clone, PartialEq, Eq)]
pub struct PgKeywords {
    pub host: String,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ssl_mode: Option<SslMode>,
    pub application_name: Option<String>,
    pub search_path: Option<String>,
}

impl std::fmt::Debug for PgKeywords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgKeywords")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("username", &self.username)
            .field(
                "password",
                &self
                    .password
                    .as_ref()
                    .map(|_| crate::sql::dsn::REDACTED.to_string()),
            )
            .field("ssl_mode", &self.ssl_mode)
            .field("application_name", &self.application_name)
            .field("search_path", &self.search_path)
            .finish()
    }
}

/// What a PostgreSQL connection string resolved to.
///
/// `Debug` is hand-written for the same reason as [`PgKeywords`]: the URL arm
/// carries `postgres://user:password@host/db` whole.
#[derive(Clone, PartialEq, Eq)]
pub enum PgTarget {
    /// A `postgres:`/`postgresql:` URL, handed to sqlx verbatim so its own
    /// query parameters (`sslmode`, `application_name`, `options`) keep working.
    Url(String),
    /// An Npgsql-style keyword string.
    Keywords(PgKeywords),
}

impl std::fmt::Debug for PgTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PgTarget::Url(_) => write!(f, "PgTarget::Url({})", crate::sql::dsn::REDACTED),
            PgTarget::Keywords(k) => f.debug_tuple("PgTarget::Keywords").field(k).finish(),
        }
    }
}

/// Resolve a connection string to something sqlx can open. Pure.
pub fn connect_target(dsn: &str) -> Result<PgTarget, PgTargetError> {
    let trimmed = dsn.trim();
    if trimmed.is_empty() {
        return Err(PgTargetError::Empty);
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        return Ok(PgTarget::Url(trimmed.to_string()));
    }

    let pairs = keyword_pairs(trimmed);
    let get = |names: &[&str]| -> Option<String> {
        pairs
            .iter()
            .find(|(k, v)| names.contains(&k.as_str()) && !v.is_empty())
            .map(|(_, v)| v.clone())
    };

    // The caller has already selected PostgreSQL, so accept the server aliases
    // supported by Npgsql even though they are ambiguous during engine sniffing.
    let Some(host) = get(&["host", "server", "data source", "datasource"]) else {
        return Err(PgTargetError::NoHostNamed);
    };

    let port = match get(&["port"]) {
        None => None,
        Some(text) => Some(text.parse::<u16>().map_err(|_| PgTargetError::BadPort)?),
    };
    if port == Some(0) {
        return Err(PgTargetError::BadPort);
    }

    let ssl_mode = match get(&["ssl mode", "sslmode"]) {
        None => None,
        Some(text) => Some(parse_ssl_mode(&text).ok_or(PgTargetError::UnknownSslMode)?),
    };

    Ok(PgTarget::Keywords(PgKeywords {
        host,
        port,
        database: get(&["database"]),
        username: get(&["username", "user id", "userid", "user name", "user"]),
        password: get(&["password", "pwd"]),
        ssl_mode,
        application_name: get(&["application name", "applicationname"]),
        search_path: get(&["search path", "searchpath"]),
    }))
}

/// Every spelling of an SSL mode this build accepts.
///
/// A total function over what it recognises and [`None`] for everything else —
/// the caller turns that into [`PgTargetError::UnknownSslMode`] rather than a
/// default, which is the whole point.
fn parse_ssl_mode(value: &str) -> Option<SslMode> {
    let v = value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "");
    Some(match v.as_str() {
        "disable" | "disabled" => SslMode::Disable,
        "allow" => SslMode::Allow,
        "prefer" => SslMode::Prefer,
        "require" | "required" => SslMode::Require,
        "verifyca" => SslMode::VerifyCa,
        "verifyfull" => SslMode::VerifyFull,
        _ => return None,
    })
}

/// Split `a=1;b=2` into lowercased keys and trimmed values.
///
/// Deliberately minimal, and deliberately not shared with [`crate::sql::dsn`]'s
/// own splitter: that one exists to find secrets to redact and errs towards
/// over-matching, which is the wrong bias for choosing a server to connect to.
fn keyword_pairs(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}

/// Turn the driver's reported column type into what crosses IPC.
///
/// sqlx spells "I never resolved this type OID" as the name `?`. On the wire
/// that must be [`None`] (*not reported*), because `Some("?")` reads as a
/// column whose type is named `?`, which is a different and untrue claim. A
/// real type name — including a user-defined one this build cannot decode — is
/// passed through exactly as the server spelled it.
pub fn column_type_name(reported: &str) -> Option<String> {
    let t = reported.trim();
    if t.is_empty() || t == "?" {
        None
    } else {
        Some(t.to_string())
    }
}

// ---------------------------------------------------------------------------
// The type map
// ---------------------------------------------------------------------------

/// Which decoder a PostgreSQL type name selects.
///
/// The map is on the *type*, not on the wire format: a type is decodable or it
/// is not, and how a value happened to arrive must not change that answer.
/// A type absent from [`decoder_for`] abstains rather than being guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgDecoder {
    Bool,
    Int2,
    Int4,
    Int8,
    Oid,
    Float4,
    Float8,
    Numeric,
    Text,
    Bytes,
    Uuid,
    Json,
    Jsonb,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Interval,
}

/// The decoder for a type, or [`None`] when this build has none.
///
/// [`None`] is a real answer and the common one: arrays (`INT4[]`), ranges,
/// geometric and network types, `bit`, `xml`, `money`, `timetz`, composites,
/// domains and every user-defined type land here and are reported by name.
pub fn decoder_for(type_name: &str) -> Option<PgDecoder> {
    // `"char"` — the one-byte internal type — is spelled with its quotes.
    let name = type_name.trim().trim_matches('"').to_ascii_uppercase();
    Some(match name.as_str() {
        "BOOL" => PgDecoder::Bool,
        "INT2" => PgDecoder::Int2,
        "INT4" => PgDecoder::Int4,
        "INT8" => PgDecoder::Int8,
        "OID" => PgDecoder::Oid,
        "FLOAT4" => PgDecoder::Float4,
        "FLOAT8" => PgDecoder::Float8,
        "NUMERIC" => PgDecoder::Numeric,
        "TEXT" | "VARCHAR" | "CHAR" | "BPCHAR" | "NAME" | "UNKNOWN" => PgDecoder::Text,
        "BYTEA" => PgDecoder::Bytes,
        "UUID" => PgDecoder::Uuid,
        "JSON" => PgDecoder::Json,
        "JSONB" => PgDecoder::Jsonb,
        "DATE" => PgDecoder::Date,
        "TIME" => PgDecoder::Time,
        "TIMESTAMP" => PgDecoder::Timestamp,
        "TIMESTAMPTZ" => PgDecoder::TimestampTz,
        "INTERVAL" => PgDecoder::Interval,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Decoding (bytes in, `Cell` out — no rendering decided here)
// ---------------------------------------------------------------------------

/// Decode one non-null Postgres value, together with **how many bytes the
/// server sent for it**.
///
/// The second half of the pair is what the byte budget is charged at, and it is
/// not derivable from the [`Cell`]: text is copied under
/// [`caps::TEXT_COPY_BYTES`] and rendered under
/// [`crate::sql::format::MAX_TEXT_BYTES`], so by the time anything downstream
/// could weigh it, the size that mattered is gone.
///
/// A `NULL` never reaches here — the caller answers that from
/// [`ValueRef::is_null`] — so there is no arm that could confuse *no value*
/// with *a value this build cannot read*.
pub fn decode_value(type_name: &str, format: WireFormat, bytes: &[u8]) -> (Cell, u64) {
    let source = bytes.len() as u64;
    let Some(decoder) = decoder_for(type_name) else {
        // Not a guess and not a blank: this build has no decoder for whatever
        // the server reported, and the cell says which type that was.
        return (
            Cell::Unsupported {
                type_name: type_name.to_string(),
            },
            source,
        );
    };
    let decoded = match format {
        WireFormat::Binary => decode_binary(decoder, bytes),
        WireFormat::Text => decode_text(decoder, bytes),
    };
    (
        decoded.unwrap_or_else(|reason| Cell::Error { reason }),
        source,
    )
}

/// Decode a value in Postgres' binary wire format.
fn decode_binary(decoder: PgDecoder, bytes: &[u8]) -> Result<Cell, String> {
    Ok(match decoder {
        PgDecoder::Bool => match bytes {
            [0] => Cell::Bool(false),
            [1] => Cell::Bool(true),
            // Not `false`: an encoding this build has not seen is an admission,
            // never a value.
            _ => return Err("a bool value did not arrive as a single 0 or 1 byte".to_string()),
        },
        PgDecoder::Int2 => Cell::Int(i16::from_be_bytes(fixed(bytes, "int2")?) as i64),
        PgDecoder::Int4 => Cell::Int(i32::from_be_bytes(fixed(bytes, "int4")?) as i64),
        PgDecoder::Int8 => Cell::Int(i64::from_be_bytes(fixed(bytes, "int8")?)),
        // `oid` is unsigned: reading it as an i32 turns 4294967295 into -1.
        PgDecoder::Oid => Cell::Int(u32::from_be_bytes(fixed(bytes, "oid")?) as i64),
        PgDecoder::Float4 => Cell::Numeric(render_f32(f32::from_bits(u32::from_be_bytes(fixed(
            bytes, "float4",
        )?)))),
        PgDecoder::Float8 => {
            Cell::Float(f64::from_bits(u64::from_be_bytes(fixed(bytes, "float8")?)))
        }
        PgDecoder::Numeric => Cell::Numeric(render_numeric(bytes)?),
        PgDecoder::Text | PgDecoder::Json => text_cell(bytes)?,
        PgDecoder::Jsonb => {
            let (version, body) = bytes
                .split_first()
                .ok_or_else(|| "a jsonb value arrived with no version byte".to_string())?;
            if *version != 1 {
                // Decoding a version this build has not seen as if it were
                // version 1 would drop or shift a byte, silently.
                return Err(format!(
                    "this build reads jsonb version 1; the server sent version {version}"
                ));
            }
            text_cell(body)?
        }
        PgDecoder::Bytes => bytes_cell(bytes),
        PgDecoder::Uuid => Cell::Text(render_uuid(&fixed::<16>(bytes, "uuid")?)),
        PgDecoder::Date => Cell::Text(render_date(i32::from_be_bytes(fixed(bytes, "date")?))),
        PgDecoder::Time => Cell::Text(render_time(i64::from_be_bytes(fixed(bytes, "time")?))),
        PgDecoder::Timestamp => Cell::Text(render_timestamp(
            i64::from_be_bytes(fixed(bytes, "timestamp")?),
            false,
        )),
        PgDecoder::TimestampTz => Cell::Text(render_timestamp(
            i64::from_be_bytes(fixed(bytes, "timestamptz")?),
            true,
        )),
        PgDecoder::Interval => {
            let raw: [u8; 16] = fixed(bytes, "interval")?;
            let micros = i64::from_be_bytes(raw[0..8].try_into().expect("8 bytes"));
            let days = i32::from_be_bytes(raw[8..12].try_into().expect("4 bytes"));
            let months = i32::from_be_bytes(raw[12..16].try_into().expect("4 bytes"));
            Cell::Text(render_interval(micros, days, months))
        }
    })
}

/// Decode a value in Postgres' text wire format — the server's own rendering.
///
/// Kept as the server spelled it wherever that *is* the value (dates,
/// intervals, `numeric`), and parsed wherever the wire type is not a string, so
/// a byte sequence that is not a number cannot reach the grid *as* a number.
fn decode_text(decoder: PgDecoder, bytes: &[u8]) -> Result<Cell, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| format!("the value is not valid UTF-8: {e}"))?;
    Ok(match decoder {
        PgDecoder::Bool => match text {
            "t" => Cell::Bool(true),
            "f" => Cell::Bool(false),
            _ => return Err("a bool value was neither `t` nor `f`".to_string()),
        },
        PgDecoder::Int2 | PgDecoder::Int4 | PgDecoder::Int8 => Cell::Int(
            text.parse::<i64>()
                .map_err(|_| "the value is not an integer".to_string())?,
        ),
        PgDecoder::Oid => Cell::Int(
            text.parse::<u32>()
                .map_err(|_| "the value is not an object identifier".to_string())?
                as i64,
        ),
        PgDecoder::Float4 | PgDecoder::Float8 | PgDecoder::Numeric => {
            if text.parse::<f64>().is_err() {
                return Err("the value is not a number".to_string());
            }
            // Kept verbatim: the server's own rendering carries the scale, and
            // re-deriving it through a float would lose it.
            Cell::Numeric(text.to_string())
        }
        PgDecoder::Bytes => parse_hex_bytea(text)?,
        PgDecoder::Text
        | PgDecoder::Uuid
        | PgDecoder::Json
        | PgDecoder::Jsonb
        | PgDecoder::Date
        | PgDecoder::Time
        | PgDecoder::Timestamp
        | PgDecoder::TimestampTz
        | PgDecoder::Interval => Cell::Text(caps::clip_text(text).to_string()),
    })
}

/// A fixed-width binary body, or a message naming both widths.
fn fixed<const N: usize>(bytes: &[u8], what: &str) -> Result<[u8; N], String> {
    bytes.try_into().map_err(|_| {
        format!(
            "a {what} value arrived as {} bytes rather than {N}",
            bytes.len()
        )
    })
}

/// Text, copied under [`caps::TEXT_COPY_BYTES`] rather than whole.
fn text_cell(bytes: &[u8]) -> Result<Cell, String> {
    // Borrowed first: nothing enormous is copied, and the copy that follows is
    // the bounded one.
    let text =
        std::str::from_utf8(bytes).map_err(|e| format!("the value is not valid UTF-8: {e}"))?;
    Ok(Cell::Text(caps::clip_text(text).to_string()))
}

/// Binary, refused rather than shortened past [`caps::MAX_BLOB_COPY_BYTES`].
///
/// A blob is the one cell a bounded copy cannot honestly shorten: the wire
/// shape reports `byteLength` — the size of the *whole* value — so handing the
/// renderer a prefix would report the prefix's length as the value's.
fn bytes_cell(bytes: &[u8]) -> Cell {
    let source = bytes.len() as u64;
    if source > caps::MAX_BLOB_COPY_BYTES {
        Cell::Error {
            reason: oversize_blob(source),
        }
    } else {
        Cell::Bytes(bytes.to_vec())
    }
}

/// Why a binary value was not read. Names a size and nothing else: the bytes
/// themselves are exactly what this refuses to copy. Worded as
/// [`super::sqlite`]'s is, because the reader should not have to learn two
/// sentences for one rule.
fn oversize_blob(source_bytes: u64) -> String {
    format!(
        "this value is {source_bytes} bytes, past the {} bytes a single cell is read under, so its contents were not read",
        caps::MAX_BLOB_COPY_BYTES
    )
}

/// Postgres' `hex` output format for `bytea`, which is what every server since
/// 9.0 emits by default.
///
/// The pre-9.0 `escape` format is a *different* encoding, so a body that is not
/// `\x`-prefixed hex is refused rather than reinterpreted.
fn parse_hex_bytea(text: &str) -> Result<Cell, String> {
    let body = text.strip_prefix("\\x").ok_or_else(|| {
        "a bytea value did not arrive in the `\\x` hex format this build reads".to_string()
    })?;
    if body.len() % 2 != 0 {
        return Err("a bytea hex body has an odd number of digits".to_string());
    }
    if (body.len() / 2) as u64 > caps::MAX_BLOB_COPY_BYTES {
        return Ok(Cell::Error {
            reason: oversize_blob((body.len() / 2) as u64),
        });
    }
    let mut out = Vec::with_capacity(body.len() / 2);
    let raw = body.as_bytes();
    for pair in raw.chunks(2) {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push(hi << 4 | lo);
    }
    Ok(Cell::Bytes(out))
}

fn hex_nibble(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err("a bytea hex body contains a character that is not a hex digit".to_string()),
    }
}

/// A `float4` at its own precision.
///
/// Widening to `f64` and printing that yields `0.10000000149011612` for
/// `0.1f32` — a number the database does not hold. The three values JSON cannot
/// carry are spelled exactly as [`crate::sql::format`] spells them, so the two
/// paths can never disagree.
fn render_f32(value: f32) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else {
        value.to_string()
    }
}

// -- numeric ----------------------------------------------------------------

const NUMERIC_POS: u16 = 0x0000;
const NUMERIC_NEG: u16 = 0x4000;
const NUMERIC_NAN: u16 = 0xC000;
const NUMERIC_PINF: u16 = 0xD000;
const NUMERIC_NINF: u16 = 0xF000;

/// Render Postgres' binary `numeric` — an `i16` count, an `i16` base-10000
/// weight, a `u16` sign and a `u16` display scale, then that many base-10000
/// digit groups.
///
/// The output is a *string* and never a float: the scale is part of the value
/// (`0.10` is not `0.1` to a ledger) and a `numeric` routinely carries more
/// precision than a double. A body that does not match its own header is
/// refused rather than rendered as whatever it happens to decode to.
fn render_numeric(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 8 {
        return Err("a numeric value arrived with an incomplete header".to_string());
    }
    let ndigits = i16::from_be_bytes([bytes[0], bytes[1]]);
    let weight = i16::from_be_bytes([bytes[2], bytes[3]]) as i32;
    let sign = u16::from_be_bytes([bytes[4], bytes[5]]);
    let dscale = u16::from_be_bytes([bytes[6], bytes[7]]) as usize;

    match sign {
        NUMERIC_NAN => return Ok("NaN".to_string()),
        NUMERIC_PINF => return Ok("Infinity".to_string()),
        NUMERIC_NINF => return Ok("-Infinity".to_string()),
        NUMERIC_POS | NUMERIC_NEG => {}
        other => {
            return Err(format!(
                "a numeric value carries an unknown sign {other:#06x}"
            ))
        }
    }
    if ndigits < 0 {
        return Err("a numeric value claims a negative digit count".to_string());
    }
    let ndigits = ndigits as usize;
    if bytes.len() != 8 + ndigits * 2 {
        return Err("a numeric value's body does not match its digit count".to_string());
    }
    let mut digits = Vec::with_capacity(ndigits);
    for i in 0..ndigits {
        let at = 8 + i * 2;
        let digit = i16::from_be_bytes([bytes[at], bytes[at + 1]]);
        if !(0..10_000).contains(&digit) {
            return Err("a numeric value carries a digit group outside 0..9999".to_string());
        }
        digits.push(digit);
    }

    let mut out = String::new();
    if sign == NUMERIC_NEG {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        for i in 0..=weight as usize {
            let digit = digits.get(i).copied().unwrap_or(0);
            if i == 0 {
                out.push_str(&digit.to_string());
            } else {
                // A missing trailing group is four zeroes, not nothing.
                out.push_str(&format!("{digit:04}"));
            }
        }
    }
    if dscale > 0 {
        out.push('.');
        let mut frac = String::with_capacity(dscale + 4);
        let mut exponent: i32 = -1;
        while frac.len() < dscale {
            // The group at base-10000 exponent `e` is at index `weight - e`; an
            // index below zero is a leading zero of the fraction.
            let index = weight - exponent;
            let digit = if index >= 0 {
                digits.get(index as usize).copied().unwrap_or(0)
            } else {
                0
            };
            frac.push_str(&format!("{digit:04}"));
            exponent -= 1;
        }
        frac.truncate(dscale);
        out.push_str(&frac);
    }
    Ok(out)
}

// -- uuid, dates and times --------------------------------------------------

fn render_uuid(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Postgres' `date`: days from 2000-01-01, with the two sentinels that are not
/// dates at all.
fn render_date(days: i32) -> String {
    if days == i32::MAX {
        return "infinity".to_string();
    }
    if days == i32::MIN {
        return "-infinity".to_string();
    }
    let (year, month, day) = civil_from_days(days as i64 + PG_EPOCH_DAYS_FROM_UNIX);
    render_civil(year, month, day)
}

/// Postgres' `timestamp`/`timestamptz`: microseconds from 2000-01-01 00:00:00.
///
/// A `timestamptz` is stored in UTC, and the rendering says `+00` rather than
/// showing a UTC instant in a zone nobody named. A `timestamp` has no zone at
/// all and gets no suffix — the two are different facts.
fn render_timestamp(micros: i64, utc: bool) -> String {
    if micros == i64::MAX {
        return "infinity".to_string();
    }
    if micros == i64::MIN {
        return "-infinity".to_string();
    }
    // Floored, not truncated: a microsecond before the epoch is the previous
    // day at 23:59:59.999999, never a negative time on the epoch day.
    let days = micros.div_euclid(MICROS_PER_DAY);
    let rest = micros.rem_euclid(MICROS_PER_DAY);
    let (year, month, day) = civil_from_days(days + PG_EPOCH_DAYS_FROM_UNIX);
    format!(
        "{} {}{}",
        render_civil(year, month, day),
        render_time(rest),
        if utc { "+00" } else { "" }
    )
}

/// A time of day from microseconds since midnight. The fraction is printed only
/// when there is one, and without its trailing zeroes — as Postgres prints it.
fn render_time(micros: i64) -> String {
    let hours = micros / 3_600_000_000;
    let rest = micros % 3_600_000_000;
    let minutes = rest / 60_000_000;
    let rest = rest % 60_000_000;
    let seconds = rest / 1_000_000;
    let fraction = rest % 1_000_000;
    format!(
        "{hours:02}:{minutes:02}:{seconds:02}{}",
        render_fraction(fraction)
    )
}

fn render_fraction(micros: i64) -> String {
    if micros == 0 {
        return String::new();
    }
    let digits = format!("{micros:06}");
    format!(".{}", digits.trim_end_matches('0'))
}

/// Postgres' `interval`: microseconds, days and months, kept as three fields.
///
/// They are not interconvertible — a month is not 30 days and a day is not 24
/// hours across a DST boundary — so summing them into one duration would be an
/// answer the database did not give.
fn render_interval(micros: i64, days: i32, months: i32) -> String {
    let years = months / 12;
    let remaining_months = months % 12;
    let mut parts: Vec<String> = Vec::new();
    if years != 0 {
        parts.push(format!("{years} {}", plural(years as i64, "year", "years")));
    }
    if remaining_months != 0 {
        parts.push(format!(
            "{remaining_months} {}",
            plural(remaining_months as i64, "mon", "mons")
        ));
    }
    if days != 0 {
        parts.push(format!("{days} {}", plural(days as i64, "day", "days")));
    }
    // A zero interval is `00:00:00`, not the empty string.
    if micros != 0 || parts.is_empty() {
        let magnitude = micros.unsigned_abs();
        let hours = magnitude / 3_600_000_000;
        let rest = magnitude % 3_600_000_000;
        let minutes = rest / 60_000_000;
        let rest = rest % 60_000_000;
        let seconds = rest / 1_000_000;
        let fraction = (rest % 1_000_000) as i64;
        parts.push(format!(
            "{}{hours:02}:{minutes:02}:{seconds:02}{}",
            if micros < 0 { "-" } else { "" },
            render_fraction(fraction)
        ));
    }
    parts.join(" ")
}

fn plural(n: i64, one: &'static str, many: &'static str) -> &'static str {
    if n.abs() == 1 {
        one
    } else {
        many
    }
}

/// A year-month-day as Postgres prints it.
///
/// The proleptic year 0 is 1 BC — Postgres has no year zero — so a
/// non-positive year is labelled rather than signed.
fn render_civil(year: i64, month: u32, day: u32) -> String {
    if year <= 0 {
        format!("{:04}-{month:02}-{day:02} BC", 1 - year)
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian date.
///
/// Howard Hinnant's `civil_from_days`, which is exact for every `i64` a date or
/// timestamp can hold and needs no calendar crate. Spelled out rather than
/// pulled in because this build has no `chrono`/`time` sqlx feature and adding
/// one to render a date would be a much larger change than the arithmetic.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153; // [0, 11], March-based
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;
    (year + i64::from(month <= 2), month, day)
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// Opens PostgreSQL connections.
#[derive(Debug, Clone, Copy)]
pub struct PostgresDriver {
    connect_timeout: Duration,
}

impl Default for PostgresDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl PostgresDriver {
    pub fn new() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    /// A driver with a different connect deadline. Every caller gets one:
    /// [`PostgresDriver::new`] carries [`DEFAULT_CONNECT_TIMEOUT`].
    pub fn with_connect_timeout(connect_timeout: Duration) -> Self {
        Self { connect_timeout }
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Build the open options. Separated from [`SqlDriver::connect`] so the
    /// resolution half is reachable without touching a network.
    ///
    /// Neither arm echoes the input on failure: a sqlx URL parse error can
    /// carry the URL, and the URL carries the password.
    fn options(spec: &ConnectSpec) -> DriverResult<PgConnectOptions> {
        let target = connect_target(&spec.dsn).map_err(|e| DriverError::connect(e.message()))?;
        let mut options = match target {
            PgTarget::Url(url) => PgConnectOptions::from_str(&url).map_err(|_| {
                DriverError::connect(
                    "The connection string could not be parsed as a PostgreSQL URL.",
                )
            })?,
            PgTarget::Keywords(k) => {
                let mut o = PgConnectOptions::new().host(&k.host);
                if let Some(port) = k.port {
                    o = o.port(port);
                }
                if let Some(database) = &k.database {
                    o = o.database(database);
                }
                if let Some(username) = &k.username {
                    o = o.username(username);
                }
                if let Some(password) = &k.password {
                    o = o.password(password);
                }
                if let Some(mode) = k.ssl_mode {
                    o = o.ssl_mode(mode.to_sqlx());
                }
                if let Some(name) = &k.application_name {
                    o = o.application_name(name);
                }
                if let Some(path) = &k.search_path {
                    o = o.options([("search_path", path.as_str())]);
                }
                o
            }
        };
        if !spec.writes_allowed {
            // Belt as well as braces: the session `SET` below is the thing that
            // is verified, but this covers the very first statement too, and a
            // later `-c` wins over an earlier one so a DSN's own `options` is
            // not silently overriding this.
            options = options.options([("default_transaction_read_only", "on")]);
        }
        Ok(options.disable_statement_logging())
    }
}

impl SqlDriver for PostgresDriver {
    fn engine(&self) -> SqlEngine {
        SqlEngine::Postgres
    }

    fn read_only_enforcement(&self, writes_allowed: bool) -> ReadOnlyEnforcement {
        // Deferred to the one place each engine's real guarantee is written
        // down, so this driver cannot drift into claiming a mechanism it does
        // not use. For Postgres that resolves to
        // `ReadOnlyEnforcement::ServerSession` — and a connection only ever
        // carries it once the setting was read back and confirmed.
        super::expected_enforcement(Some(SqlEngine::Postgres), writes_allowed)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> DriverFuture<'a, Box<dyn SqlConnection>> {
        let enforcement = self.read_only_enforcement(spec.writes_allowed);
        let deadline = self.connect_timeout;
        let writes_allowed = spec.writes_allowed;
        Box::pin(async move {
            let options = Self::options(spec)?;
            // Bounded, because an unbounded connect cannot be stopped: the stop
            // signal is watched by the row loop, which has not started yet.
            // Making the session read-only is inside the same deadline, because
            // a handle whose `SET` has not answered is not a usable connection
            // either.
            let conn = super::within_connect_deadline(deadline, async {
                let mut conn = PgConnection::connect_with(&options)
                    .await
                    .map_err(|e| DriverError::connect(e.to_string()))?;
                if !writes_allowed {
                    enforce_session_read_only(&mut conn).await?;
                }
                Ok(conn)
            })
            .await?;
            Ok(Box::new(PgLiveConnection { conn, enforcement }) as Box<dyn SqlConnection>)
        })
    }
}

/// Hold the session read-only at the server, and **confirm that it took**.
///
/// Sending the `SET` and the session actually being read-only are two different
/// facts, and [`ReadOnlyEnforcement::ServerSession`] is a claim about the
/// second. A server that would not accept it — or that answers anything but
/// [`READ_ONLY_CONFIRMATION`] — fails the connect rather than quietly leaving
/// the user with a writable connection they were told was not one.
async fn enforce_session_read_only(conn: &mut PgConnection) -> DriverResult<()> {
    conn.execute(sqlx::raw_sql(SET_SESSION_READ_ONLY))
        .await
        .map_err(|e| {
            DriverError::connect(format!(
                "The session could not be set read-only, so the connection was not opened: {e}"
            ))
        })?;
    let row = conn
        .fetch_optional(sqlx::raw_sql(SHOW_SESSION_READ_ONLY))
        .await
        .map_err(|e| {
            DriverError::connect(format!(
                "The read-only session setting could not be read back, so the connection was not \
                 opened: {e}"
            ))
        })?;
    let Some(row) = row else {
        return Err(read_only_not_confirmed());
    };
    let raw = row.try_get_raw(0).map_err(|_| read_only_not_confirmed())?;
    let text = raw.as_str().map_err(|_| read_only_not_confirmed())?;
    if text.trim().eq_ignore_ascii_case(READ_ONLY_CONFIRMATION) {
        Ok(())
    } else {
        Err(read_only_not_confirmed())
    }
}

/// The refusal when the server will not confirm a read-only session.
///
/// Names nothing from the connection string, and says the connection was *not
/// opened*: a connection that cannot refuse writes must never be handed back
/// under a badge that says it can.
fn read_only_not_confirmed() -> DriverError {
    DriverError::connect(
        "The server did not confirm that this session is read-only, so the connection was not \
         opened. A connection that cannot refuse a write must not be presented as one that can.",
    )
}

/// One open PostgreSQL connection.
struct PgLiveConnection {
    conn: PgConnection,
    /// Recorded at open time, and only ever
    /// [`ReadOnlyEnforcement::ServerSession`] once the server confirmed the
    /// setting — see [`enforce_session_read_only`].
    enforcement: ReadOnlyEnforcement,
}

impl SqlConnection for PgLiveConnection {
    fn engine(&self) -> SqlEngine {
        SqlEngine::Postgres
    }

    fn read_only_enforcement(&self) -> ReadOnlyEnforcement {
        self.enforcement
    }

    fn run_statement<'a>(
        &'a mut self,
        sql: &'a str,
        statement_index: usize,
        limits: Limits,
        events: &'a mpsc::Sender<SqlEvent>,
        stop: StopSignal,
    ) -> DriverFuture<'a, StatementOutcome> {
        Box::pin(run_statement_inner(
            &mut self.conn,
            sql,
            statement_index,
            limits,
            events,
            stop,
        ))
    }
}

/// Decode one cell of a Postgres row, together with the bytes the server sent.
///
/// The only place a driver type is touched. Everything it decides is delegated
/// to [`decode_value`], which is pure.
fn decode_cell(row: &PgRow, index: usize) -> (Cell, u64) {
    let value: PgValueRef<'_> = match row.try_get_raw(index) {
        Ok(v) => v,
        Err(e) => {
            return (
                Cell::Error {
                    reason: e.to_string(),
                },
                0,
            )
        }
    };
    if value.is_null() {
        return (Cell::Null, 0);
    }
    let type_name = value.type_info().name().to_string();
    let format = match value.format() {
        PgValueFormat::Binary => WireFormat::Binary,
        PgValueFormat::Text => WireFormat::Text,
    };
    // Borrowed straight out of the row's buffer: nothing enormous is copied
    // before `decode_value` has decided whether to copy it at all.
    let bytes = match value.as_bytes() {
        Ok(b) => b,
        Err(e) => {
            return (
                Cell::Error {
                    reason: e.to_string(),
                },
                0,
            )
        }
    };
    decode_value(&type_name, format, bytes)
}

async fn run_statement_inner(
    conn: &mut PgConnection,
    sql: &str,
    statement_index: usize,
    limits: Limits,
    events: &mpsc::Sender<SqlEvent>,
    mut stop: StopSignal,
) -> DriverResult<StatementOutcome> {
    let started = Instant::now();
    let _ = events.send(SqlEvent::Started { statement_index }).await;

    // Prepared first, and separately, so the columns are known even when the
    // statement returns no rows — and so a user-defined type's OID is resolved
    // to its real name rather than reaching the header as `?`.
    let prepared = conn
        .prepare(sql)
        .await
        .map_err(|e| DriverError::execute(e.to_string()))?;
    let columns: Vec<SqlColumn> = prepared
        .columns()
        .iter()
        .map(|c| SqlColumn {
            name: c.name().to_string(),
            type_name: column_type_name(c.type_info().name()),
        })
        .collect();
    drop(prepared);

    let _ = events
        .send(SqlEvent::Columns {
            statement_index,
            columns: columns.clone(),
        })
        .await;

    let mut tracker = CapTracker::new(limits);
    let mut rows: Vec<Vec<crate::sql::model::SqlValue>> = Vec::new();
    let mut sent = 0usize;
    let mut stopped = false;
    // Only set from this statement's own `CommandComplete`, and only when the
    // loop actually reached it — see the module docs.
    let mut rows_affected: Option<u64> = None;

    {
        // `sqlx::query` carries an (empty) argument list, which is what makes
        // this a *prepared* statement and therefore the binary wire format.
        let mut stream = conn.fetch_many(sqlx::query(sql));
        // Cleared once the stop channel can no longer deliver anything: a dead
        // channel is not a stop, and `changed()` on a closed channel is ready
        // every time, which would spin the loop instead of reading rows.
        let mut watching_stop = true;
        loop {
            // Checked before awaiting, because a stop that arrived before the
            // first row would never show up as a *change*.
            if *stop.borrow_and_update() {
                stopped = true;
                break;
            }
            let item = if watching_stop {
                tokio::select! {
                    biased;
                    wait = stop_wait(&mut stop) => match wait {
                        StopWait::Stop => { stopped = true; break; }
                        StopWait::Alive => continue,
                        StopWait::Gone => { watching_stop = false; continue; }
                    },
                    item = next_item(&mut stream) => item,
                }
            } else {
                next_item(&mut stream).await
            };
            let Some(item) = item else { break };
            let item = item.map_err(|e| DriverError::execute(e.to_string()))?;
            let row = match item {
                Either::Right(row) => row,
                Either::Left(result) => {
                    // Postgres' `CommandComplete` describes *this* statement,
                    // unlike sqlite's `sqlite3_changes()`, so the count is
                    // attributable and is reported.
                    rows_affected = Some(result.rows_affected());
                    continue;
                }
            };
            if tracker.is_full() {
                // A row exists that will not be delivered — this, and only
                // this, turns an armed cap into a reported one.
                tracker.confirm_more();
                break;
            }
            // Each cell paired with the size the *server* sent for it, so the
            // budget is charged at that rather than at the rendered thumbnail
            // of it. See `caps::charged_cell_bytes`.
            let decoded: Vec<(crate::sql::model::SqlValue, u64)> = (0..row.columns().len())
                .map(|i| {
                    let (cell, source_bytes) = decode_cell(&row, i);
                    (to_wire(format_cell(cell)), source_bytes)
                })
                .collect();
            tracker.record(charged_row_bytes(&decoded));
            rows.push(decoded.into_iter().map(|(value, _)| value).collect());
            // No early exit on `Flow::Full`: the loop must go round once more
            // to find out whether there *is* another row, or a result set that
            // ends exactly on the limit would be reported as capped.
            if rows.len() - sent >= ROW_BATCH {
                let _ = events
                    .send(SqlEvent::Rows {
                        statement_index,
                        rows: rows[sent..].to_vec(),
                    })
                    .await;
                sent = rows.len();
            }
        }
    }

    if sent < rows.len() {
        let _ = events
            .send(SqlEvent::Rows {
                statement_index,
                rows: rows[sent..].to_vec(),
            })
            .await;
    }

    let result = SqlResultSet {
        columns,
        rows,
        row_cap: tracker.cap(),
        // `None` where the loop ended early: *no row count to report* is not
        // `Some(0)`.
        rows_affected,
        elapsed_ms: started.elapsed().as_millis() as u64,
        statement_index,
    };
    // The completion carries no rows: they streamed in `SqlEvent::Rows` above,
    // and that stream is the authoritative copy.
    let _ = events
        .send(SqlEvent::Completed {
            completion: result.completion(),
        })
        .await;

    Ok(if stopped {
        StatementOutcome::Stopped(result)
    } else {
        StatementOutcome::Finished(result)
    })
}

/// What waiting on the stop channel produced.
///
/// Three answers, for the reason [`super::sqlite`] spells out at length:
/// `watch::Receiver::changed()` resolves with `Err` the instant the last sender
/// drops, and reading that as a stop reports a user action that never happened
/// while returning an arbitrary prefix of the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopWait {
    /// A stop was signalled.
    Stop,
    /// The channel woke us without a stop, and can still deliver one.
    Alive,
    /// Every sender is gone: no stop can ever arrive on this channel.
    Gone,
}

/// Wait for the stop channel to say something, and say which of the three it
/// was. Cancel-safe: `changed()` is, and nothing else here awaits.
async fn stop_wait(stop: &mut StopSignal) -> StopWait {
    let alive = stop.changed().await.is_ok();
    if *stop.borrow_and_update() {
        // A real signal, whether or not the sender outlived it.
        StopWait::Stop
    } else if alive {
        StopWait::Alive
    } else {
        StopWait::Gone
    }
}

/// One item from a stream, without pulling in `futures-util`'s `StreamExt`.
async fn next_item<S>(stream: &mut S) -> Option<S::Item>
where
    S: Stream + Unpin + ?Sized,
{
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

#[cfg(test)]
#[path = "postgres_tests.rs"]
mod tests;
