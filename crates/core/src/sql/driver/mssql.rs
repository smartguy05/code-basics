//! The SQL Server driver, over `tiberius`.
//!
//! Modelled on [`super::sqlite`] — borrowed decode, the three-answer
//! [`StopWait`], the connect deadline, and both caps charged at true source
//! size — and it must be read *against* it, because the one thing it cannot
//! copy is the part that matters most.
//!
//! # Read-only here is the weakest of the three engines, and says so
//!
//! SQLite opens the handle `SQLITE_OPEN_READONLY` and PostgreSQL has the server
//! hold the session read-only; in both, **a write never runs**.
//!
//! **SQL Server has no read-only session mode at all.** There is no
//! `SET SESSION … READ ONLY`, and the things that look like one are not:
//! `ApplicationIntent=ReadOnly` is *routing* for an availability group and
//! enforces nothing, and a read-only database or filegroup is a property of the
//! server that the console cannot set. The best available is to run each
//! statement inside a transaction that is always rolled back, which is what
//! this driver does — so a write **executes** and is then undone.
//!
//! That is why [`super::declared_read_only_enforcement`] maps this engine to
//! [`ReadOnlyEnforcement::RolledBackTransaction`], whose
//! [`strength`](super::EnforcementStrength::Undone) is a separate arm from
//! [`Refused`](super::EnforcementStrength::Refused) with no ordering between
//! them. **A rollback is not a sandbox.** Anything the statement does outside
//! the transaction is not undone by it: `xp_cmdshell`, a linked server, an
//! autonomous procedure, `sp_send_dbmail`, a CLR assembly. Nothing in this file
//! may be worded as though it were, and a test pins the wording.
//!
//! Two consequences worth stating plainly:
//!
//! - The transaction is opened and rolled back **per statement**, not per
//!   connection, so a failure to roll one back is reported against the
//!   statement that opened it rather than being discovered later.
//! - A **stopped** statement does not get a rollback sent after it. Sending one
//!   means calling `simple_query` again, and tiberius' `flush_stream` first
//!   drains every remaining packet of the result set the user just asked to
//!   stop reading. So the connection is *poisoned* instead — it refuses every
//!   later statement — and the reader is told, in a [`SqlEvent::Notice`], that
//!   the transaction ends when the connection does. SQL Server rolls back an
//!   uncommitted transaction when the session ends; nothing here can commit it
//!   in the meantime.
//!
//! The cost that buys, and it is worth knowing before changing any of it: a
//! statement the row loop leaves early for **any other** reason — a cap bit, a
//! second result set appeared — does get its rollback, and `flush_stream` drains
//! the rest of the result set off the wire first. That drain is not caused by
//! the rollback; it is what reusing a tiberius connection after an abandoned
//! stream costs at all. The only way to avoid it is to poison the connection,
//! and a capped query is far too ordinary a thing to end a connection over.
//!
//! # Integrated authentication is Windows-only, and that is its own outcome
//!
//! `AuthMethod::Integrated` exists only under `cfg(windows)` with tiberius'
//! `winauth` feature, which is exactly how this workspace enables it. On any
//! other platform a connection string asking for integrated auth cannot work,
//! and tiberius does not say so: its ADO parser falls through to
//! `AuthMethod::sql_server("", "")`, which fails with *Login failed for user
//! ''* — a message that means "wrong password" and would send the reader to
//! check credentials that were never in play.
//!
//! Worse, tiberius reads only `IntegratedSecurity` / `Integrated Security`.
//! **`Trusted_Connection` — the spelling almost every .NET connection string
//! uses — it ignores entirely**, so that same empty login happens on Windows
//! too. So this module detects the request itself ([`integrated_auth`]),
//! refuses before opening a socket where it cannot work ([`auth_preflight`]),
//! and on Windows supplies the auth method tiberius could not see.
//!
//! # Temporal values abstain
//!
//! This build enables neither of tiberius' `chrono`/`time` features, so a
//! `date`/`time`/`datetime2`/`datetimeoffset` arrives as the raw TDS day and
//! increment counts. Rendering one means calendar arithmetic that nothing in
//! this repository can check against a server, and a wrong date looks exactly
//! like a right one. So the cell becomes [`Cell::Unsupported`] naming the SQL
//! type — never a number that reads as data, and never a blank that reads as
//! `NULL`. Enabling a tiberius date feature is the fix, and it is a dependency
//! decision rather than a decoding one.

use std::pin::Pin;
use std::time::{Duration, Instant};

use futures_core::Stream;
use tiberius::{ColumnData, ColumnType, Config, QueryItem, SqlBrowser};
use tokio::sync::mpsc;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use super::caps::{self, charged_row_bytes, CapTracker, Limits};
use super::value::to_wire;
use super::{
    ConnectSpec, DriverError, DriverFuture, DriverResult, ErrorStage, ReadOnlyEnforcement,
    SqlConnection, SqlDriver, StatementOutcome, StopSignal, DEFAULT_CONNECT_TIMEOUT,
};
use crate::sql::dsn::SqlEngine;
use crate::sql::format::{format_cell, Cell};
use crate::sql::model::{SqlColumn, SqlEvent, SqlResultSet, SqlValue};

/// How many rows are batched into one [`SqlEvent::Rows`]. Same reasoning as
/// [`super::sqlite::ROW_BATCH`].
pub const ROW_BATCH: usize = 100;

/// What a fixed-width scalar costs the byte budget — the eight bytes the server
/// stores, which [`caps::charged_cell_bytes`] takes only if it exceeds the
/// rendered size, so this can never undercharge.
const SCALAR_SOURCE_BYTES: u64 = 8;

/// Whether this build can authenticate as the logged-in Windows user.
///
/// `cfg(windows)` and not a tiberius feature probe, because the workspace
/// manifest is what ties the two together: `winauth` is enabled under
/// `[target.'cfg(windows)'.dependencies]`, so on Windows it is on and nowhere
/// else is it available at all.
pub const INTEGRATED_AUTH_AVAILABLE: bool = cfg!(windows);

/// Opens the transaction every read-only statement runs inside.
pub const BEGIN_SQL: &str = "BEGIN TRANSACTION";

/// Closes it — guarded, because the statement may have closed it already.
///
/// The user's own SQL may `COMMIT`, and a server-side error may abort the
/// transaction on its own. A bare `ROLLBACK` then raises error 3903 ("The
/// ROLLBACK TRANSACTION request has no corresponding BEGIN TRANSACTION"),
/// which this driver would have to report as *the transaction may still be
/// open* — the opposite of the truth, and exactly the sort of false alarm that
/// teaches people to ignore the real one.
pub const ROLLBACK_SQL: &str = "IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION";

// ---------------------------------------------------------------------------
// Connection-string reading (pure)
// ---------------------------------------------------------------------------

/// Which connection-string grammar a string is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `Server=…;Database=…` — the ordinary .NET form.
    Ado,
    /// `jdbc:sqlserver://…`.
    Jdbc,
}

/// Which grammar to hand tiberius. Only the `jdbc:` prefix is positive
/// evidence; everything else is ADO, which is what a `.NET` workspace writes.
pub fn dialect(dsn: &str) -> Dialect {
    let t = dsn.trim_start();
    if t.len() >= 5 && t[..5].eq_ignore_ascii_case("jdbc:") {
        Dialect::Jdbc
    } else {
        Dialect::Ado
    }
}

/// Split an ADO.NET connection string into lowercased keys and their values.
///
/// Quote-aware, and that is not a nicety: ADO.NET quotes a value containing a
/// `;`, so a naive split reads keys *out of a password*. `Password='a;Trusted_
/// Connection=True'` would otherwise be seen as a request for integrated auth
/// and refused, on a string that asked for nothing of the kind.
///
/// Deliberately not shared with [`crate::sql::dsn`]'s splitter, for the reason
/// [`super::sqlite`] gives for its own: that one exists to find secrets to
/// redact and errs towards over-matching, which is the wrong bias for deciding
/// what a string asked for.
///
/// Where a quoted value is never closed, the rest of the string is taken as
/// part of it rather than split — the same abstention as `dsn::redact`, and for
/// the same reason: inventing keys out of password text is worse than losing a
/// trailing pair.
pub fn keyword_pairs(dsn: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = dsn.chars().collect();
    let mut pairs = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        // Key: up to the next '=' (or ';', which ends a valueless fragment).
        let start = i;
        while i < chars.len() && chars[i] != '=' && chars[i] != ';' {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect();
        let key = key.trim().to_ascii_lowercase();
        if i >= chars.len() {
            break;
        }
        if chars[i] == ';' {
            i += 1;
            continue;
        }
        i += 1; // past '='
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let value = if i < chars.len() && (chars[i] == '\'' || chars[i] == '"') {
            let quote = chars[i];
            i += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == quote {
                    if i + 1 < chars.len() && chars[i + 1] == quote {
                        value.push(quote);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                value.push(chars[i]);
                i += 1;
            }
            if !closed {
                // Never proved to end. Everything after it belongs to this
                // value; do not split it into keys.
                if !key.is_empty() {
                    pairs.push((key, value));
                }
                return pairs;
            }
            // Skip to the separator.
            while i < chars.len() && chars[i] != ';' {
                i += 1;
            }
            value
        } else {
            let start = i;
            while i < chars.len() && chars[i] != ';' {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            raw.trim().to_string()
        };
        if i < chars.len() {
            i += 1; // past ';'
        }
        if !key.is_empty() {
            pairs.push((key, value));
        }
    }
    pairs
}

/// The keys tiberius' own ADO parser reads as a request for integrated auth.
const TIBERIUS_INTEGRATED_KEYS: &[&str] = &["integratedsecurity", "integrated security"];

/// The keys .NET accepts that tiberius does **not** read.
const IGNORED_INTEGRATED_KEYS: &[&str] = &[
    "trusted_connection",
    "trusted connection",
    "trustedconnection",
];

/// What a connection string says about integrated (Windows) authentication.
///
/// Three answers, not two. `Integrated Security=maybe` is neither a request nor
/// a refusal, and reading it as "not requested" would quietly attempt a SQL
/// login the string never asked for — then fail with a credentials error about
/// credentials nobody supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegratedAuth {
    /// No such key, or an explicit `false`/`no`.
    NotRequested,
    /// Integrated auth was asked for. `understood_by_tiberius` is false for the
    /// spellings tiberius ignores, which this module then has to supply itself.
    Requested {
        key: String,
        understood_by_tiberius: bool,
    },
    /// The key is present and its value is not a boolean this can read. Its own
    /// answer, never folded into [`NotRequested`](IntegratedAuth::NotRequested).
    UnreadableValue { key: String },
}

/// Read what the string asks for. Pure.
///
/// A request wins over an unreadable value, and an unreadable value wins over
/// silence — the safe direction, since both produce a message naming the key
/// rather than a login failure naming nothing.
pub fn integrated_auth(dsn: &str) -> IntegratedAuth {
    let mut unreadable: Option<String> = None;
    let mut requested: Option<(String, bool)> = None;
    for (key, value) in keyword_pairs(dsn) {
        let understood = TIBERIUS_INTEGRATED_KEYS.contains(&key.as_str());
        if !understood && !IGNORED_INTEGRATED_KEYS.contains(&key.as_str()) {
            continue;
        }
        match integrated_value(&value) {
            Some(true) => {
                let already = requested.as_ref().map(|(_, u)| *u).unwrap_or(false);
                if requested.is_none() {
                    requested = Some((key, understood));
                } else if understood && !already {
                    requested = Some((key, true));
                }
            }
            Some(false) => {}
            None => {
                let _ = unreadable.get_or_insert(key);
            }
        }
    }
    match (requested, unreadable) {
        (Some((key, understood_by_tiberius)), _) => IntegratedAuth::Requested {
            key,
            understood_by_tiberius,
        },
        (None, Some(key)) => IntegratedAuth::UnreadableValue { key },
        (None, None) => IntegratedAuth::NotRequested,
    }
}

/// The booleans .NET accepts here, plus `sspi`. `None` is *unreadable*, which
/// the caller must not turn into `false`.
fn integrated_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "sspi" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

/// Refuse, before any socket opens, a string this platform cannot honour.
///
/// `supported` is a parameter rather than a `cfg!` so both platforms' behaviour
/// is provable on either platform — the outcome under test is a *message*, and
/// a message only one build can produce is a message only one build can check.
///
/// Neither refusal repeats any part of the connection string: the key name is a
/// fixed vocabulary, and the value is never quoted back.
pub fn auth_preflight(dsn: &str, supported: bool) -> DriverResult<()> {
    match integrated_auth(dsn) {
        IntegratedAuth::NotRequested => Ok(()),
        IntegratedAuth::Requested { key, .. } if !supported => Err(DriverError::new(
            ErrorStage::Connect,
            format!(
                "This connection string asks for integrated Windows authentication (`{key}`), \
                 which is available only on Windows. On this platform SQL Server authentication \
                 is the only option: give the connection a `User Id` and a `Password` instead. \
                 Nothing was sent."
            ),
        )),
        IntegratedAuth::Requested { .. } => Ok(()),
        IntegratedAuth::UnreadableValue { key } => Err(DriverError::new(
            ErrorStage::Connect,
            format!(
                "The `{key}` setting in this connection string is not a value that can be read as \
                 yes or no (expected `true`, `false`, `yes`, `no` or `SSPI`). Rather than guess \
                 which kind of authentication was meant, nothing was sent."
            ),
        )),
    }
}

/// The user and password the string states, read the way tiberius reads them.
///
/// Only used on Windows, and only for the spelling tiberius cannot see: it is
/// what decides between `AuthMethod::Integrated` and
/// `AuthMethod::windows(user, password)`, mirroring tiberius' own rule rather
/// than inventing a second one. The password is returned to a caller that hands
/// it straight to tiberius; it is never logged, stored or put in an error.
fn stated_credentials(dsn: &str) -> (Option<String>, Option<String>) {
    let pairs = keyword_pairs(dsn);
    let find = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            pairs
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        })
    };
    (
        find(&["uid", "username", "user", "user id"]),
        find(&["password", "pwd"]),
    )
}

/// The named instance the string asks for, if any.
///
/// Decides which of two connect paths is taken, and the difference is a
/// *message*: tiberius' `connect_named` swallows the TCP error and reports
/// "Could not resolve server host" for a refused connection, which would send
/// the reader to check DNS for a server that answered. So the SQL Browser path
/// is taken only when an instance was actually asked for, and the ordinary path
/// keeps the operating system's own error.
pub fn named_instance(dsn: &str) -> Option<String> {
    let value = keyword_pairs(dsn)
        .into_iter()
        .find(|(key, _)| key == "server" || key == "data source")
        .map(|(_, value)| value)?;
    let value = value.strip_prefix("tcp:").unwrap_or(&value);
    // `host\INSTANCE,port` — the port is after the comma, the instance before.
    let host_part = value.split(',').next().unwrap_or(value);
    let (_, instance) = host_part.split_once('\\')?;
    let instance = instance.trim();
    (!instance.is_empty()).then(|| instance.to_string())
}

// ---------------------------------------------------------------------------
// Column types
// ---------------------------------------------------------------------------

/// The SQL type name for a TDS column-type token, or [`None`] when the server
/// reported none.
///
/// [`ColumnType::Null`] becomes `None` for the reason
/// [`super::sqlite::column_type_name`] gives: `Some("null")` claims the
/// column's type *is* the null type, which is a different and untrue claim.
///
/// The subtle half is that TDS metadata reports a *protocol* token, not the
/// declared column type, and three of the tokens name a family rather than a
/// type: `Intn` is a nullable integer of unstated width (tinyint through
/// bigint), `Floatn` a nullable float of either width, `Datetimen` either
/// datetime shape. Those keep the token's own name, because reporting `int` for
/// a `bigint` column is a width the server never sent.
pub fn column_type_name(column_type: ColumnType) -> Option<String> {
    let name = match column_type {
        ColumnType::Null => return None,
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Intn => "intn",
        ColumnType::Float4 => "real",
        ColumnType::Float8 => "float",
        ColumnType::Floatn => "floatn",
        ColumnType::Money => "money",
        ColumnType::Money4 => "smallmoney",
        ColumnType::Datetime => "datetime",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Datetimen => "datetimen",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigBinary => "binary",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::Text => "text",
        ColumnType::Image => "image",
        ColumnType::NText => "ntext",
        ColumnType::SSVariant => "sql_variant",
    };
    Some(name.to_string())
}

// ---------------------------------------------------------------------------
// Decoding (driver types in, `Cell` out — no rendering decided here)
// ---------------------------------------------------------------------------

/// Decode one cell into the vocabulary [`crate::sql::format`] renders, together
/// with **how many bytes the server holds for it**.
///
/// The second half of the pair is what the byte budget is charged at, for the
/// reason [`super::sqlite::decode_cell`] spells out: a text cell is copied
/// under [`caps::TEXT_COPY_BYTES`] and rendered under
/// [`crate::sql::format::MAX_TEXT_BYTES`], so by the time anything downstream
/// could weigh it, the size that mattered is gone.
///
/// Values are read *borrowed* out of tiberius' own buffers, and the copy that
/// follows each is the bounded one. A binary value past
/// [`caps::MAX_BLOB_COPY_BYTES`] abstains rather than being shortened, because
/// the wire shape reports `byteLength` — the size of the whole value — which a
/// clipped copy would silently understate.
pub fn decode_cell(data: &ColumnData<'_>) -> (Cell, u64) {
    match data {
        ColumnData::U8(v) => scalar(v.map(|n| Cell::Int(n as i64))),
        ColumnData::I16(v) => scalar(v.map(|n| Cell::Int(n as i64))),
        ColumnData::I32(v) => scalar(v.map(|n| Cell::Int(n as i64))),
        ColumnData::I64(v) => scalar(v.map(Cell::Int)),
        // A 32-bit float widened to 64 bits gains thirteen digits that are an
        // artefact of the widening: `3.14f32 as f64` is 3.140000104904175, and
        // on screen that is indistinguishable from a value which really is that
        // precise. `f32::to_string` is the shortest text that reads back as the
        // same f32, so it is handed over as already-rendered text instead —
        // except for the three non-finite values, which are widened losslessly
        // so `format` can spell them in the way JSON accepts.
        ColumnData::F32(v) => scalar(v.map(|n| {
            if n.is_finite() {
                Cell::Numeric(n.to_string())
            } else {
                Cell::Float(n as f64)
            }
        })),
        ColumnData::F64(v) => scalar(v.map(Cell::Float)),
        ColumnData::Bit(v) => scalar(v.map(Cell::Bool)),
        ColumnData::Guid(v) => match v {
            None => (Cell::Null, 0),
            Some(id) => {
                let text = id.to_string();
                let source = text.len() as u64;
                (Cell::Text(text), source)
            }
        },
        ColumnData::String(v) => match v {
            None => (Cell::Null, 0),
            Some(s) => (Cell::Text(caps::clip_text(s).to_string()), s.len() as u64),
        },
        ColumnData::Xml(v) => match v {
            None => (Cell::Null, 0),
            Some(x) => {
                let s: &str = x.as_ref().as_ref();
                (Cell::Text(caps::clip_text(s).to_string()), s.len() as u64)
            }
        },
        ColumnData::Numeric(v) => match v {
            None => (Cell::Null, 0),
            Some(n) => {
                let text = numeric_text(n.value(), n.scale());
                let source = (text.len() as u64).max(SCALAR_SOURCE_BYTES);
                (Cell::Numeric(text), source)
            }
        },
        ColumnData::Binary(v) => match v {
            None => (Cell::Null, 0),
            Some(bytes) => {
                let source = bytes.len() as u64;
                if source > caps::MAX_BLOB_COPY_BYTES {
                    (
                        Cell::Error {
                            reason: oversize_binary(source),
                        },
                        source,
                    )
                } else {
                    (Cell::Bytes(bytes.to_vec()), source)
                }
            }
        },
        // Temporal values: raw TDS counts in this build. See the module docs —
        // the cell names the type it could not decode rather than showing a
        // number that looks like data or a blank that reads as NULL.
        ColumnData::DateTime(v) => temporal(v.is_some(), "datetime"),
        ColumnData::SmallDateTime(v) => temporal(v.is_some(), "smalldatetime"),
        ColumnData::Time(v) => temporal(v.is_some(), "time"),
        ColumnData::Date(v) => temporal(v.is_some(), "date"),
        ColumnData::DateTime2(v) => temporal(v.is_some(), "datetime2"),
        ColumnData::DateTimeOffset(v) => temporal(v.is_some(), "datetimeoffset"),
    }
}

/// A fixed-width value, or the `NULL` in its place. A `NULL` is a `NULL`
/// whatever its column's type is, and costs the budget nothing.
fn scalar(cell: Option<Cell>) -> (Cell, u64) {
    match cell {
        Some(cell) => (cell, SCALAR_SOURCE_BYTES),
        None => (Cell::Null, 0),
    }
}

/// A temporal value this build has no decoder for — or the `NULL` in its place,
/// which is still just a `NULL`.
fn temporal(present: bool, type_name: &str) -> (Cell, u64) {
    if present {
        (
            Cell::Unsupported {
                type_name: type_name.to_string(),
            },
            0,
        )
    } else {
        (Cell::Null, 0)
    }
}

/// Why a binary value was not read. Names a size and nothing else: the bytes
/// themselves are exactly what this refuses to copy.
fn oversize_binary(source_bytes: u64) -> String {
    format!(
        "this value is {source_bytes} bytes, past the {} bytes a single cell is read under, so its contents were not read",
        caps::MAX_BLOB_COPY_BYTES
    )
}

/// A TDS `decimal`/`numeric` as text, from its scaled integer and its scale.
///
/// Exact, by construction: the digits are the integer's own and the point is
/// inserted, so nothing goes through a float. `0.10` stays `0.10` — to a ledger
/// that is not `0.1`, and it is the server's own scale that says which.
pub fn numeric_text(value: i128, scale: u8) -> String {
    if scale == 0 {
        return value.to_string();
    }
    let scale = scale as usize;
    let digits = value.unsigned_abs().to_string();
    let digits = if digits.len() <= scale {
        format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits)
    } else {
        digits
    };
    let split = digits.len() - scale;
    format!(
        "{}{}.{}",
        if value < 0 { "-" } else { "" },
        &digits[..split],
        &digits[split..]
    )
}

// ---------------------------------------------------------------------------
// Notices and failures the transaction story needs
// ---------------------------------------------------------------------------

/// What the reader is told when a statement inside the read-only transaction
/// was stopped.
///
/// It must not claim a rollback happened here, because none was sent — see the
/// module docs for why sending one would have to drain the stream the user
/// asked to stop reading.
pub fn stopped_in_transaction_notice() -> String {
    "This statement was stopped while its read-only transaction was still open, so no rollback \
     was sent — sending one would first have to read the rest of the result set you asked to \
     stop reading. The transaction is rolled back by the server when this connection closes, and \
     this connection will not run another statement."
        .to_string()
}

/// What the reader is told when a statement produced more than one result set.
///
/// A second result set has different columns, so it cannot be appended to the
/// first, and this layer has one [`SqlResultSet`] to report. Saying nothing
/// would show a partial answer that looks complete.
pub fn extra_result_sets_notice() -> String {
    "This statement returned more than one result set. Only the first is shown; the rest were not \
     read."
        .to_string()
}

/// What the reader is told when the rollback itself failed.
///
/// Never reassuring: at this point the statement has run and the transaction
/// may still be open, so a write may still be there.
pub fn rollback_failed_message(reason: &str) -> String {
    format!(
        "The statement ran inside a transaction that could not then be rolled back, so the \
         transaction may still be open and anything the statement wrote may still be there. This \
         connection will not run another statement. The server said: {reason}"
    )
}

/// What a poisoned connection says to every later statement.
const POISONED_MESSAGE: &str = "This connection was left with an open read-only transaction and \
                                will not run another statement. Close it and connect again.";

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// The client type this driver holds: tiberius over a tokio socket, adapted
/// with `tokio_util`'s `compat` because tiberius speaks the `futures` I/O
/// traits and everything else in this workspace speaks tokio's.
type MssqlClient = tiberius::Client<Compat<tokio::net::TcpStream>>;

/// Opens SQL Server connections.
#[derive(Debug, Clone, Copy)]
pub struct MssqlDriver {
    connect_timeout: Duration,
}

impl Default for MssqlDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MssqlDriver {
    pub fn new() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    pub fn with_connect_timeout(connect_timeout: Duration) -> Self {
        Self { connect_timeout }
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }
}

impl SqlDriver for MssqlDriver {
    fn engine(&self) -> SqlEngine {
        SqlEngine::SqlServer
    }

    fn read_only_enforcement(&self, writes_allowed: bool) -> ReadOnlyEnforcement {
        // Deferred to the one place each engine's real guarantee is written
        // down, so this driver cannot drift into claiming a stronger mechanism
        // than it has. For SQL Server that resolves to
        // `RolledBackTransaction` — the write runs and is undone.
        super::expected_enforcement(Some(SqlEngine::SqlServer), writes_allowed)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> DriverFuture<'a, Box<dyn SqlConnection>> {
        let enforcement = self.read_only_enforcement(spec.writes_allowed);
        let deadline = self.connect_timeout;
        let writes_allowed = spec.writes_allowed;
        Box::pin(async move {
            // Before any socket: a request this platform cannot honour is its
            // own answer, not a login failure. See the module docs.
            auth_preflight(&spec.dsn, INTEGRATED_AUTH_AVAILABLE)?;

            let mut config = match dialect(&spec.dsn) {
                Dialect::Ado => Config::from_ado_string(&spec.dsn),
                Dialect::Jdbc => Config::from_jdbc_string(&spec.dsn),
            }
            .map_err(|e| DriverError::connect(e.to_string()))?;
            apply_integrated_auth(&mut config, &spec.dsn);
            let instance = named_instance(&spec.dsn);

            // Bounded for the reason `driver/mod.rs` gives: nothing watches the
            // stop signal until the row loop starts, so an unbounded connect
            // makes a stop report success while nothing stops. Running out of
            // it is `ErrorStage::ConnectTimeout`, never a refusal and never a
            // user stop.
            let client = super::within_connect_deadline(deadline, async move {
                let tcp = if instance.is_some() {
                    // Named instances need the SQL Browser to turn a name into
                    // a port. Only taken when one was asked for: this path
                    // reports "Could not resolve server host" for a connection
                    // the operating system refused.
                    <tokio::net::TcpStream as SqlBrowser>::connect_named(&config)
                        .await
                        .map_err(|e| DriverError::connect(e.to_string()))?
                } else {
                    let tcp = tokio::net::TcpStream::connect(config.get_addr())
                        .await
                        .map_err(|e| DriverError::connect(e.to_string()))?;
                    tcp.set_nodelay(true)
                        .map_err(|e| DriverError::connect(e.to_string()))?;
                    tcp
                };
                tiberius::Client::connect(config, tcp.compat_write())
                    .await
                    .map_err(|e| DriverError::connect(e.to_string()))
            })
            .await?;

            Ok(Box::new(MssqlLiveConnection {
                client,
                enforcement,
                writes_allowed,
                poisoned: false,
            }) as Box<dyn SqlConnection>)
        })
    }
}

/// Supply the auth method tiberius could not see.
///
/// Only for the spellings its own ADO parser ignores; where it understood the
/// key, its answer is left alone so there is one rule rather than two. The
/// choice between `Integrated` and `windows(user, password)` mirrors what
/// tiberius does for `IntegratedSecurity`.
#[cfg(windows)]
fn apply_integrated_auth(config: &mut Config, dsn: &str) {
    if let IntegratedAuth::Requested {
        understood_by_tiberius: false,
        ..
    } = integrated_auth(dsn)
    {
        let (user, password) = stated_credentials(dsn);
        config.authentication(match (user, password) {
            (None, None) => tiberius::AuthMethod::Integrated,
            (user, password) => tiberius::AuthMethod::windows(
                user.unwrap_or_default(),
                password.unwrap_or_default(),
            ),
        });
    }
}

/// Off Windows there is nothing to supply: [`auth_preflight`] has already
/// refused every string that would have reached here asking for it.
#[cfg(not(windows))]
fn apply_integrated_auth(_config: &mut Config, _dsn: &str) {}

/// One open SQL Server connection.
struct MssqlLiveConnection {
    client: MssqlClient,
    /// Recorded at open time — and it is only ever
    /// [`ReadOnlyEnforcement::RolledBackTransaction`] because the transaction
    /// is actually opened around every statement below.
    enforcement: ReadOnlyEnforcement,
    writes_allowed: bool,
    /// Set when a read-only transaction was left open. A poisoned connection
    /// runs nothing further, so nothing can commit it; the server rolls it back
    /// when the session ends.
    poisoned: bool,
}

impl SqlConnection for MssqlLiveConnection {
    fn engine(&self) -> SqlEngine {
        SqlEngine::SqlServer
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
            self,
            sql,
            statement_index,
            limits,
            events,
            stop,
        ))
    }
}

async fn run_statement_inner(
    conn: &mut MssqlLiveConnection,
    sql: &str,
    statement_index: usize,
    limits: Limits,
    events: &mpsc::Sender<SqlEvent>,
    mut stop: StopSignal,
) -> DriverResult<StatementOutcome> {
    if conn.poisoned {
        return Err(DriverError::execute(POISONED_MESSAGE));
    }
    let started = Instant::now();
    let _ = events.send(SqlEvent::Started { statement_index }).await;

    // The read-only mechanism, such as it is. Opened before the statement so a
    // failure to open it is reported instead of the statement running outside
    // any transaction at all.
    if !conn.writes_allowed {
        if let Err(reason) = run_utility(&mut conn.client, BEGIN_SQL).await {
            return Err(DriverError::execute(format!(
                "The read-only transaction this statement runs inside could not be opened, so \
                 nothing was sent. The server said: {reason}"
            )));
        }
    }

    let mut columns: Vec<SqlColumn> = Vec::new();
    let mut columns_seen = false;
    let mut rows: Vec<Vec<SqlValue>> = Vec::new();
    let mut tracker = CapTracker::new(limits);
    let mut sent = 0usize;
    let mut stopped = false;
    let mut extra_result_sets = false;

    let stream_result: DriverResult<()> = async {
        let mut stream = conn
            .client
            .simple_query(sql)
            .await
            .map_err(|e| DriverError::execute(e.to_string()))?;
        // Cleared once the stop channel can no longer deliver anything — see
        // `StopWait`: `changed()` on a closed channel is ready every time, so
        // going on selecting on it would spin instead of reading rows.
        let mut watching_stop = true;
        loop {
            // Checked before awaiting: a stop that arrived before the first row
            // would never show up as a *change*.
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
            match item {
                QueryItem::Metadata(meta) => {
                    if columns_seen {
                        // A second result set has different columns, so it
                        // cannot be appended to the first. Reported, then
                        // stopped — never silently dropped.
                        extra_result_sets = true;
                        break;
                    }
                    columns_seen = true;
                    columns = meta
                        .columns()
                        .iter()
                        .map(|c| SqlColumn {
                            name: c.name().to_string(),
                            type_name: column_type_name(c.column_type()),
                        })
                        .collect();
                    let _ = events
                        .send(SqlEvent::Columns {
                            statement_index,
                            columns: columns.clone(),
                        })
                        .await;
                }
                QueryItem::Row(row) => {
                    if tracker.is_full() {
                        // A row exists that will not be delivered — this, and
                        // only this, turns an armed cap into a reported one.
                        tracker.confirm_more();
                        break;
                    }
                    let decoded: Vec<(SqlValue, u64)> = row
                        .cells()
                        .map(|(_, data)| {
                            let (cell, source_bytes) = decode_cell(data);
                            (to_wire(format_cell(cell)), source_bytes)
                        })
                        .collect();
                    tracker.record(charged_row_bytes(&decoded));
                    rows.push(decoded.into_iter().map(|(value, _)| value).collect());
                    // No early exit on `Flow::Full`: the loop must go round once
                    // more to find out whether there *is* another row, or a
                    // result set ending exactly on the limit reads as capped.
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
        }
        Ok(())
    }
    .await;

    // Everything the statement produced is flushed *before* the transaction is
    // settled, so the reader sees the answer and only then the note about how
    // it was closed. Settling first would put the stop notice ahead of the last
    // batch of the rows it is about.
    if !columns_seen {
        // A statement with no result set has no columns, and the event stream
        // says so rather than saying nothing — an absent `Columns` is
        // indistinguishable from one that has not arrived yet.
        let _ = events
            .send(SqlEvent::Columns {
                statement_index,
                columns: Vec::new(),
            })
            .await;
    }
    if extra_result_sets {
        let _ = events
            .send(SqlEvent::Notice {
                statement_index,
                message: extra_result_sets_notice(),
            })
            .await;
    }
    if sent < rows.len() {
        let _ = events
            .send(SqlEvent::Rows {
                statement_index,
                rows: rows[sent..].to_vec(),
            })
            .await;
    }

    // The stream's borrow of the client has ended, which is what lets the
    // transaction be settled at all.
    let settled = settle_transaction(conn, stopped, statement_index, events).await;

    // Two failures, and the rollback's wins: it is the one that says a write
    // may still be there. The statement's own message is carried along rather
    // than dropped, because the two are different facts.
    match (stream_result, settled) {
        (Ok(()), Ok(())) => {}
        (Err(e), Ok(())) => return Err(e),
        (Ok(()), Err(r)) => return Err(r),
        (Err(e), Err(r)) => {
            return Err(DriverError::execute(format!(
                "{r} The statement itself also failed: {e}"
            )))
        }
    }

    let result = SqlResultSet {
        columns,
        rows,
        row_cap: tracker.cap(),
        // Always `None`. tiberius exposes a row count only through
        // `ExecuteResult`, which is the non-streaming path; the streaming one
        // never surfaces the DONE token's count. Reporting `rows.len()` instead
        // would be a different number wearing this one's name — rows returned
        // is not rows affected — and a number that may belong to another
        // question is worse than no number.
        rows_affected: None,
        elapsed_ms: started.elapsed().as_millis() as u64,
        statement_index,
    };
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

/// Close the read-only transaction, or say why it is still open.
///
/// Three outcomes, and they are not interchangeable:
///
/// - writes were allowed, so there is no transaction and nothing to do;
/// - the statement was stopped, so the rollback is *not* sent (it would drain
///   the stream the user asked to stop reading) and the connection is poisoned
///   instead, with a notice saying so;
/// - otherwise the guarded rollback runs, and a failure poisons the connection
///   and is reported — because at that point a write may still be there.
async fn settle_transaction(
    conn: &mut MssqlLiveConnection,
    stopped: bool,
    statement_index: usize,
    events: &mpsc::Sender<SqlEvent>,
) -> DriverResult<()> {
    if conn.writes_allowed {
        return Ok(());
    }
    if stopped {
        conn.poisoned = true;
        let _ = events
            .send(SqlEvent::Notice {
                statement_index,
                message: stopped_in_transaction_notice(),
            })
            .await;
        return Ok(());
    }
    match run_utility(&mut conn.client, ROLLBACK_SQL).await {
        Ok(()) => Ok(()),
        Err(reason) => {
            conn.poisoned = true;
            Err(DriverError::execute(rollback_failed_message(&reason)))
        }
    }
}

/// Run a statement of this driver's own and drain it.
///
/// Drained rather than dropped because a T-SQL error arrives as an item *in*
/// the stream, not from the call that started it — dropping the stream would
/// throw the error away and report success.
async fn run_utility(client: &mut MssqlClient, sql: &'static str) -> Result<(), String> {
    let mut stream = client.simple_query(sql).await.map_err(|e| e.to_string())?;
    while let Some(item) = next_item(&mut stream).await {
        item.map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

/// What waiting on the stop channel produced. Three answers, for the reason
/// [`super::sqlite`] gives at length: `watch::Receiver::changed()` resolves with
/// `Err` the instant the last sender drops, and a `_ =` arm in a `select!`
/// matches that as readily as a real signal — reporting an action nobody took
/// and returning an arbitrary prefix of the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopWait {
    /// A stop was signalled.
    Stop,
    /// The channel woke us without a stop, and can still deliver one.
    Alive,
    /// Every sender is gone: no stop can ever arrive on this channel.
    Gone,
}

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
#[path = "mssql_tests.rs"]
mod tests;
