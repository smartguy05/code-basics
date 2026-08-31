//! The SQLite driver, over `sqlx`.
//!
//! The first real driver, and the reference for the two that follow. Three
//! things it establishes, all of them stated in [`super`] and all of them easy
//! to lose:
//!
//! 1. **Read-only is a property of the handle, not of the SQL.** When the
//!    connection does not allow writes this opens with
//!    `SqliteConnectOptions::read_only(true)`, which becomes
//!    `SQLITE_OPEN_READONLY` — sqlite3 itself then refuses the write.
//!    [`super::guard`](crate::sql::guard) still runs first and still says it is
//!    a heuristic; this is the part that is not.
//!
//!    The one-way rule matters here: this **never** calls `read_only(false)`. A
//!    DSN carrying `?mode=ro` has already set the flag, and clearing it would
//!    silently upgrade a connection the author wrote down as read-only.
//!
//! 2. **Nothing here decides a rendering.** A cell is decoded into a
//!    [`Cell`](crate::sql::format::Cell) — including the two honest admissions,
//!    `Unsupported` and `Error` — and [`format_cell`] decides what it looks
//!    like. A storage class this build has no decoder for becomes
//!    `Unsupported { type_name }`, never a blank cell that reads as `NULL`.
//!
//! 3. **`rows_affected` is always [`None`], on purpose.** sqlx reads it from
//!    `sqlite3_changes()`, whose own documentation says it "returns the number
//!    of changes of the *last* statement; not necessarily this one". After a
//!    `SELECT` it still holds whatever the previous write did. A number that may
//!    belong to a different statement is worse than no number, so this driver
//!    reports the honest `None` — *this statement has no row count to report* —
//!    and never a figure it cannot attribute. Pinned by a test so a later
//!    "improvement" has to argue with it.
//!
//! 4. **Only a genuine stop signal stops the stream.** `watch::Receiver::
//!    changed()` resolves with `Err` the moment the last sender drops, so a
//!    `_ =` arm in a `select!` reads a dead channel as a user stop — reporting
//!    an action nobody took and returning a prefix of the rows as if it had
//!    been asked for. [`StopWait`] keeps *stopped*, *woken* and *the channel is
//!    gone* apart, and a gone channel makes the loop read to the end.
//!
//! 5. **A cell is charged at what the database holds, and copied under a
//!    bound.** The byte budget exists to stop an unbounded amount of data
//!    reaching the grid, and a cell is rendered under
//!    [`format`](crate::sql::format)'s display caps — so charging the *rendered*
//!    size lets a table of two-megabyte values stream past an eight-megabyte
//!    budget a few kilobytes at a time. [`decode_cell`] therefore reports the
//!    source size alongside the cell, decodes borrowed so nothing enormous is
//!    ever copied whole, and refuses (rather than shortens) a blob past
//!    [`caps::MAX_BLOB_COPY_BYTES`], whose wire shape would otherwise report a
//!    prefix's length as the value's.

use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::time::{Duration, Instant};

use futures_core::Stream;
use sqlx::sqlite::{SqliteConnectOptions, SqliteRow, SqliteValueRef};
use sqlx::{Column, ConnectOptions, Connection, Decode, Row, Sqlite, SqliteConnection, Statement};
use sqlx::{Either, Executor, TypeInfo, ValueRef};
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

/// How many rows are batched into one [`SqlEvent::Rows`].
///
/// Small enough that a slow query fills the grid as it goes, large enough that a
/// thousand-row result is not a thousand IPC messages.
pub const ROW_BATCH: usize = 100;

// ---------------------------------------------------------------------------
// The connection target (pure)
// ---------------------------------------------------------------------------

/// What a SQLite connection string resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqliteTarget {
    /// A `sqlite:`/`sqlite://` URL, handed to sqlx verbatim so its own query
    /// parameters (`mode`, `immutable`, `vfs`) keep working.
    Url(String),
    /// A filesystem path, or `:memory:`, taken from an ADO.NET-style
    /// `Data Source=` keyword or from a bare path.
    Path(String),
}

/// Why a connection string could not be resolved to a SQLite database.
///
/// Neither variant carries any of the input: a SQLite DSN can hold a SQLCipher
/// `Password=`, and an error message is exactly the leak the subsystem docs
/// forbid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetError {
    /// Nothing but whitespace.
    Empty,
    /// It parsed as keyword pairs, and none of them names a database file.
    /// Abstaining rather than picking a plausible-looking value: guessing wrong
    /// here means opening, or creating, the wrong file.
    NoDatabaseNamed,
}

impl TargetError {
    pub fn message(self) -> &'static str {
        match self {
            TargetError::Empty => "The connection string is empty.",
            TargetError::NoDatabaseNamed => {
                "The connection string names no SQLite database file. Expected a `sqlite:` URL, \
                 a `Data Source=` keyword, or a path."
            }
        }
    }
}

/// Keywords whose value is the database file, lowercased.
const FILE_KEYS: &[&str] = &["data source", "datasource", "filename"];

/// Resolve a connection string to something sqlx can open. Pure.
pub fn connect_target(dsn: &str) -> Result<SqliteTarget, TargetError> {
    let trimmed = dsn.trim();
    if trimmed.is_empty() {
        return Err(TargetError::Empty);
    }
    if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("sqlite:") {
        return Ok(SqliteTarget::Url(trimmed.to_string()));
    }
    if !trimmed.contains('=') {
        // No keyword pairs at all: the whole thing is the file. `:memory:` and
        // a Windows path with a drive letter both land here.
        return Ok(SqliteTarget::Path(trimmed.to_string()));
    }
    for (key, value) in keyword_pairs(trimmed) {
        if FILE_KEYS.contains(&key.as_str()) && !value.is_empty() {
            return Ok(SqliteTarget::Path(value));
        }
    }
    Err(TargetError::NoDatabaseNamed)
}

/// Split `a=1;b=2` into lowercased keys and trimmed values.
///
/// Deliberately minimal, and deliberately not shared with
/// [`crate::sql::dsn`]'s own splitter: that one exists to find secrets to
/// redact and errs towards over-matching, which is the wrong bias for choosing
/// a file to open.
fn keyword_pairs(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}

/// Turn the driver's reported column type into what crosses IPC.
///
/// sqlx spells "sqlite reported no type for this column" as the type name
/// `NULL` (`DataType::Null`), which is what `sqlite3_column_decltype` returns a
/// null pointer for — an expression or a subquery. On the wire that must be
/// [`None`] (*not reported*), because `Some("NULL")` reads as a column whose
/// type is the null type, which is a different and untrue claim.
pub fn column_type_name(reported: &str) -> Option<String> {
    let t = reported.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("NULL") {
        None
    } else {
        Some(t.to_string())
    }
}

// ---------------------------------------------------------------------------
// Decoding (driver types in, `Cell` out — no rendering decided here)
// ---------------------------------------------------------------------------

/// Decode one cell of a SQLite row into the vocabulary `format` renders,
/// together with **how many bytes the database holds for it**.
///
/// The second half of that pair is what the byte budget is charged at. It is
/// not derivable from the [`Cell`]: a text cell is copied under
/// [`caps::TEXT_COPY_BYTES`] and rendered under
/// [`crate::sql::format::MAX_TEXT_BYTES`], so by the time anything downstream
/// could weigh it, the size that mattered is gone.
///
/// Nothing here decides a rendering; the two *copy* bounds are memory rules,
/// not display rules, and they differ per storage class for one reason.
/// `SqlValue::Text` has no field claiming a length, so a clipped copy is still
/// honest once `format` marks it truncated — while `SqlValue::Bytes` reports
/// `byteLength`, *the size of the whole value*, which a clipped copy would
/// silently understate. So an oversized blob abstains ([`Cell::Error`], which
/// renders as `Unavailable`) with its true size named, rather than being
/// shortened into a lie or copied whole into memory.
fn decode_cell(row: &SqliteRow, index: usize) -> (Cell, u64) {
    let value: SqliteValueRef<'_> = match row.try_get_raw(index) {
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
    // Copied out before `value` is consumed by a decode.
    let type_name = value.type_info().name().to_string();

    // SQLite has five storage classes. `type_info()` on a *value* reports the
    // storage class of the value actually present, so this is the real type of
    // this cell, not the column's declared affinity.
    match type_name.as_str() {
        "INTEGER" => (
            decoded::<i64>(value, &type_name).map_or_else(cell_error, Cell::Int),
            SCALAR_SOURCE_BYTES,
        ),
        "REAL" => (
            decoded::<f64>(value, &type_name).map_or_else(cell_error, Cell::Float),
            SCALAR_SOURCE_BYTES,
        ),
        // Decoded *borrowed*, so the whole value is never copied: `&str` and
        // `&[u8]` come straight out of sqlite's own buffer. The copy that
        // follows each is the bounded one.
        "TEXT" => match decoded::<&str>(value, &type_name) {
            Ok(text) => (
                Cell::Text(caps::clip_text(text).to_string()),
                text.len() as u64,
            ),
            Err(reason) => (cell_error(reason), 0),
        },
        "BLOB" => match decoded::<&[u8]>(value, &type_name) {
            Ok(bytes) => {
                let source = bytes.len() as u64;
                if source > caps::MAX_BLOB_COPY_BYTES {
                    (
                        Cell::Error {
                            reason: oversize_blob(source),
                        },
                        source,
                    )
                } else {
                    (Cell::Bytes(bytes.to_vec()), source)
                }
            }
            Err(reason) => (cell_error(reason), 0),
        },
        // Not a guess and not a blank: this build has no decoder for whatever
        // sqlite reported, and the cell says which type that was.
        _ => (Cell::Unsupported { type_name }, 0),
    }
}

/// What a fixed-width scalar costs the byte budget.
///
/// The eight bytes sqlite stores, which is under what the rendered digits
/// weigh — [`caps::charged_cell_bytes`] takes whichever is larger, so this can
/// never undercharge a number.
const SCALAR_SOURCE_BYTES: u64 = 8;

/// Why a binary value was not read. Names a size and nothing else: the bytes
/// themselves are exactly what this refuses to copy.
fn oversize_blob(source_bytes: u64) -> String {
    format!(
        "this value is {source_bytes} bytes, past the {} bytes a single cell is read under, so its contents were not read",
        caps::MAX_BLOB_COPY_BYTES
    )
}

fn decoded<'r, T>(value: SqliteValueRef<'r>, type_name: &str) -> Result<T, String>
where
    T: Decode<'r, Sqlite>,
{
    <T as Decode<Sqlite>>::decode(value)
        .map_err(|e| format!("could not decode a {type_name} value: {e}"))
}

fn cell_error(reason: String) -> Cell {
    Cell::Error { reason }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// Opens SQLite connections.
#[derive(Debug, Clone, Copy)]
pub struct SqliteDriver {
    connect_timeout: Duration,
}

impl Default for SqliteDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl SqliteDriver {
    pub fn new() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    /// A driver with a different connect deadline. Every caller gets one:
    /// [`SqliteDriver::new`] carries [`DEFAULT_CONNECT_TIMEOUT`].
    pub fn with_connect_timeout(connect_timeout: Duration) -> Self {
        Self { connect_timeout }
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Build the open options. Separated from [`SqlDriver::connect`] so the
    /// resolution half is reachable without opening a file.
    fn options(spec: &ConnectSpec) -> DriverResult<SqliteConnectOptions> {
        let target = connect_target(&spec.dsn).map_err(|e| DriverError::connect(e.message()))?;
        let mut options = match target {
            SqliteTarget::Url(url) => SqliteConnectOptions::from_str(&url)
                .map_err(|e| DriverError::connect(e.to_string()))?,
            SqliteTarget::Path(path) => SqliteConnectOptions::new().filename(PathBuf::from(path)),
        };
        if !spec.writes_allowed {
            // One-way: never `read_only(false)`. See the module docs.
            options = options.read_only(true);
        }
        // Never create. A console that conjures an empty database when the path
        // is wrong has answered a question the user did not ask, and has
        // answered it with "this table does not exist".
        options = options.create_if_missing(false).disable_statement_logging();
        Ok(options)
    }
}

impl SqlDriver for SqliteDriver {
    fn engine(&self) -> SqlEngine {
        SqlEngine::Sqlite
    }

    fn read_only_enforcement(&self, writes_allowed: bool) -> ReadOnlyEnforcement {
        // Deferred to the one place each engine's real guarantee is written
        // down, so this driver cannot drift into claiming a mechanism it does
        // not use. For SQLite that resolves to `ReadOnlyEnforcement::Driver`:
        // sqlite3 opens the file with SQLITE_OPEN_READONLY, so this is not the
        // guard's promise — it is the database's.
        super::expected_enforcement(Some(SqlEngine::Sqlite), writes_allowed)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> DriverFuture<'a, Box<dyn SqlConnection>> {
        let enforcement = self.read_only_enforcement(spec.writes_allowed);
        let deadline = self.connect_timeout;
        Box::pin(async move {
            let options = Self::options(spec)?;
            // Bounded, because an unbounded connect cannot be stopped: the stop
            // signal is watched by the row loop, which has not started yet, so
            // a stop aimed at a connecting query reports `Signalled` while
            // nothing stops. Running out of the deadline is its own failure
            // stage — never a user stop, and never an ordinary refusal.
            // Dropping the connect future is what abandons the attempt; sqlx
            // may take a moment to notice, and nothing here waits for it.
            let conn = super::within_connect_deadline(deadline, async {
                SqliteConnection::connect_with(&options)
                    .await
                    .map_err(|e| DriverError::connect(e.to_string()))
            })
            .await?;
            Ok(Box::new(SqliteLiveConnection { conn, enforcement }) as Box<dyn SqlConnection>)
        })
    }
}

/// One open SQLite handle.
struct SqliteLiveConnection {
    conn: SqliteConnection,
    /// Recorded at open time. The handle's flags cannot be read back out of
    /// sqlx, so this is the record of what was asked for — and it is only ever
    /// [`ReadOnlyEnforcement::Driver`] when `read_only(true)` was actually set.
    enforcement: ReadOnlyEnforcement,
}

impl SqlConnection for SqliteLiveConnection {
    fn engine(&self) -> SqlEngine {
        SqlEngine::Sqlite
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

async fn run_statement_inner(
    conn: &mut SqliteConnection,
    sql: &str,
    statement_index: usize,
    limits: Limits,
    events: &mpsc::Sender<SqlEvent>,
    mut stop: StopSignal,
) -> DriverResult<StatementOutcome> {
    let started = Instant::now();
    let _ = events.send(SqlEvent::Started { statement_index }).await;

    // Prepared first, and separately, so the columns are known even when the
    // statement returns no rows: `fetch_many` only reveals them with the first
    // row, and an empty result set would otherwise arrive with no headers at
    // all — indistinguishable from a statement that has no result set.
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

    {
        let mut stream = conn.fetch_many(sqlx::raw_sql(sql));
        // Cleared once the stop channel can no longer deliver anything. See
        // [`StopWait`]: a dead channel is not a stop, and must not go on being
        // selected on — `changed()` on a closed channel is ready *every* time,
        // which would spin the loop instead of reading rows.
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
            let Either::Right(row) = item else {
                // `Either::Left` is the query result, whose row count comes from
                // `sqlite3_changes()` and may belong to an earlier statement.
                // See the module docs: it is deliberately not read.
                continue;
            };
            if tracker.is_full() {
                // A row exists that will not be delivered — this, and only
                // this, turns an armed cap into a reported one.
                tracker.confirm_more();
                break;
            }
            // Each cell paired with the size the *database* holds for it, so
            // the budget is charged at that rather than at the rendered
            // thumbnail of it. See `caps::charged_cell_bytes`.
            let decoded: Vec<(crate::sql::model::SqlValue, u64)> = (0..row.columns().len())
                .map(|i| {
                    let (cell, source_bytes) = decode_cell(&row, i);
                    (to_wire(format_cell(cell)), source_bytes)
                })
                .collect();
            tracker.record(charged_row_bytes(&decoded));
            rows.push(decoded.into_iter().map(|(value, _)| value).collect());
            // No early exit on `Flow::Full`: the loop must go round once more to
            // find out whether there *is* another row, or a result set that
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
        // Always `None` — see the module docs.
        rows_affected: None,
        elapsed_ms: started.elapsed().as_millis() as u64,
        statement_index,
    };
    // The completion carries no rows: they streamed in `SqlEvent::Rows` above,
    // and that stream is the authoritative copy. See `SqlCompletion`.
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
/// Three answers, and the reason this is an enum rather than a `bool`:
/// `watch::Receiver::changed()` resolves with `Err(RecvError)` the instant the
/// last sender drops, and a `_ =` arm in a `select!` matches that exactly as
/// readily as a real signal. Reading a dropped sender as a stop reports a user
/// action that never happened *and* returns an arbitrary prefix of the rows —
/// with the shortfall blamed on the user who never asked for it.
///
/// The rule this encodes: **only a genuine `true` stops the stream.** A sender
/// that has gone away can never deliver one, so the loop stops watching and
/// reads to the end, which is the honest answer when nothing asked it to stop.
/// A `true` that was signalled *before* the sender dropped is still a stop,
/// which is why the flag is read rather than the channel's liveness.
///
/// Impossible-by-construction was the alternative and was rejected: the sender
/// lives in [`crate::sql::session`], which hands it out and removes it on a
/// timeline this loop cannot see, so the loop has to be correct for a channel
/// that dies under it rather than assume one that cannot.
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
#[path = "sqlite_tests.rs"]
mod tests;
