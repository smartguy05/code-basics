//! `sql/driver/` is the **only** module in this subtree that opens a socket or a
//! database handle. Everything above it is pure and is tested with no database
//! software installed. A driver contains no `match` that decides a rendering —
//! it hands [`format.rs`](crate::sql::format) an already-typed value and
//! `format.rs` decides.
//!
//! That is the same containment [`crate::behavioral::replay`] has for HTTP and
//! [`crate::lsp::transport`] has for processes: one module owns the I/O, and the
//! decisions live outside it where they can be proved.
//!
//! # Read-only is enforced by the driver, not by the guard
//!
//! [`crate::sql::guard`] is a text heuristic and says so in its own refusal.
//! Where an engine can open a connection the *driver* refuses writes on, this
//! layer does that as well, and the two are different facts the user is entitled
//! to tell apart — hence [`ReadOnlyEnforcement`], which is data carried by the
//! connection rather than a sentence hardcoded in the UI.
//!
//! Each engine enforces it a different way and the wording must not be
//! interchangeable, because the three guarantees are not equally strong.
//! SQLite opens a genuinely read-only handle and PostgreSQL has the server hold
//! the session read-only — in both, a write never runs. **SQL Server has no
//! read-only session mode at all**, so the best available is a transaction that
//! is always rolled back: the write executes and is then undone, which is
//! weaker and must say so. [`declared_read_only_enforcement`] is the single
//! place that mapping is written down, and [`expected_enforcement`] is what
//! answers before a connection exists — abstaining to
//! [`ReadOnlyEnforcement::Undetermined`] rather than borrowing an engine's
//! sentence.
//!
//! # Stopping is not cancelling
//!
//! [`StopSignal`] stops *this side* reading rows and drops the connection. The
//! server may still be executing the statement, and on some engines it will run
//! to completion regardless. Nothing here sends a server-side cancel, so nothing
//! here is named as if it did: the outcome is [`StatementOutcome::Stopped`],
//! never "cancelled" and never "aborted".
//!
//! A stop must also be a thing that *happened*. A [`StopHandle`] that is simply
//! dropped is not one, and a row loop must not read the channel closing as a
//! user action — see the `StopWait` rule in [`sqlite`]. And a connect has a
//! deadline ([`DEFAULT_CONNECT_TIMEOUT`]) because nothing watches the stop
//! signal until the row loop starts: without it, a stop aimed at a query that
//! is still connecting reports success while nothing stops. Running out of that
//! deadline is [`ErrorStage::ConnectTimeout`], which is neither a refusal nor a
//! stop.
//!
//! Layout:
//!
//! - [`caps`] — the row and byte ceilings, the per-cell copy bounds, and the
//!   arithmetic that reports a cap.
//! - [`value`] — the mechanical relabelling from a rendered cell to a wire cell.
//! - [`sqlite`] — the SQLite driver, over `sqlx`.
//! - [`mssql`] — the SQL Server driver, over `tiberius`. The engine with the
//!   **weakest** read-only guarantee: it has no read-only session mode, so
//!   every statement runs in a transaction that is always rolled back.
//!
//! A new engine is one new file beside [`sqlite`], one `pub mod` line, one arm
//! in [`for_engine`] and one arm in [`declared_read_only_enforcement`]. The two
//! mappings are `match`es on [`SqlEngine`], so an engine cannot be added
//! without both being answered.

pub mod caps;
pub mod mssql;
pub mod postgres;
pub mod sqlite;
pub mod value;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::sql::dsn::{redact, SqlEngine};
use crate::sql::model::{SqlEvent, SqlResultSet};

pub use caps::{CapTracker, Flow, Limits, MAX_BLOB_COPY_BYTES};

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

/// The receiving half a row loop watches. `true` means *stop reading*.
///
/// Deliberately not called a cancellation token: see the module docs.
pub type StopSignal = watch::Receiver<bool>;

/// The sending half, held by [`crate::sql::session`].
pub type StopHandle = watch::Sender<bool>;

/// Make a fresh, un-signalled stop channel.
pub fn stop_channel() -> (StopHandle, StopSignal) {
    watch::channel(false)
}

/// How long a driver waits for a connection to open before giving up.
///
/// A connect with no deadline is not merely slow: a stop aimed at a query that
/// is still connecting reports [`crate::sql::session::StopOutcome::Signalled`]
/// while nothing stops, because the row loop that watches the stop signal has
/// not started yet. This is what bounds that window, and running out of it is
/// its own failure — [`ErrorStage::ConnectTimeout`] — never a user stop.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Read-only enforcement
// ---------------------------------------------------------------------------

/// How — or whether — read-only is being enforced on an open connection.
///
/// Six answers, not two, because the three engines this console speaks to
/// enforce read-only three genuinely different ways and one of them is much
/// weaker than the other two:
///
/// | Engine | Mechanism | Does a write run? |
/// |---|---|---|
/// | SQLite | the handle is opened `SQLITE_OPEN_READONLY` | no |
/// | PostgreSQL | the *server* holds the session read-only | no |
/// | SQL Server | **there is no read-only session mode**; each statement runs in a transaction that is always rolled back | **yes, then it is undone** |
///
/// The wording lives here rather than in the frontend so an engine cannot
/// quietly inherit a stronger sentence than it has earned — and SQL Server is
/// exactly the engine that would, since it is the one with the weakest
/// guarantee and the one users reach for most. A rollback is not a sandbox: the
/// statement executes, and anything it does *outside* the transaction — a
/// command shell, a linked server, an autonomous procedure — is not undone.
///
/// [`Undetermined`](ReadOnlyEnforcement::Undetermined) is the abstain arm. A
/// profile whose engine was never determined has no driver behind it, so which
/// of the others applies is not yet knowable, and answering with any of them
/// would be a guess — the stronger guess being the dangerous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyEnforcement {
    /// The connection was opened in a mode the driver's own handle refuses
    /// writes in. For SQLite that is `SQLITE_OPEN_READONLY`.
    Driver,
    /// The server was told to hold this session read-only and refuses a write
    /// itself. For PostgreSQL that is
    /// `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` /
    /// `default_transaction_read_only`.
    ///
    /// A driver may report this **only after the setting was accepted**. If it
    /// could not be applied, the connect must fail: silently continuing as
    /// [`GuardOnly`](ReadOnlyEnforcement::GuardOnly) would downgrade a promise
    /// the user already consented to without telling them.
    ServerSession,
    /// The engine has no read-only mode at all, so every statement is run
    /// inside a transaction that is always rolled back. The write *executes*
    /// and is then undone. This is SQL Server, and it is weaker than the two
    /// above — see the type docs.
    RolledBackTransaction,
    /// Nothing on the connection is enforcing anything, so
    /// [`crate::sql::guard`]'s text classification is the only thing between
    /// the user and a write.
    GuardOnly,
    /// Writes are allowed on this connection; nothing is enforcing read-only.
    NotEnforced,
    /// Writes are not allowed, but which mechanism (if any) is behind that is
    /// not known — typically because the engine itself was never determined,
    /// so there is no driver to ask. Never a stand-in for one of the others.
    Undetermined,
}

/// How strong an enforcement is, without naming a mechanism.
///
/// Exists so a caller can pick a tone without matching on six variants — and
/// so the one collapse that would matter cannot happen quietly: *the write
/// never runs* and *the write runs and is undone* are separate arms
/// ([`Refused`](EnforcementStrength::Refused) and
/// [`Undone`](EnforcementStrength::Undone)), and no ordering is defined between
/// them, because ranking them would invite treating one as "nearly" the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementStrength {
    /// The write is refused before it runs.
    Refused,
    /// The write runs and is then reverted. Side effects outside the
    /// transaction survive.
    Undone,
    /// Only the text guard stands in the way.
    HeuristicOnly,
    /// Nothing at all is enforcing read-only.
    Nothing,
    /// It is not known which of the above applies.
    Unknown,
}

impl ReadOnlyEnforcement {
    /// The short sentence the UI shows. Data on the driver, never a literal in
    /// a view.
    ///
    /// The phrase "before it runs" is reserved for the two mechanisms that
    /// genuinely refuse a write, and a test pins that.
    pub fn label(self) -> &'static str {
        match self {
            ReadOnlyEnforcement::Driver => "Opened read-only by the driver",
            ReadOnlyEnforcement::ServerSession => "The server is holding this session read-only",
            ReadOnlyEnforcement::RolledBackTransaction => {
                "Every statement is rolled back — not a sandbox"
            }
            ReadOnlyEnforcement::GuardOnly => {
                "Read-only by text check only — this connection can write"
            }
            ReadOnlyEnforcement::NotEnforced => "Writes are allowed on this connection",
            ReadOnlyEnforcement::Undetermined => "Read-only — enforcement not determined",
        }
    }

    /// The sentence behind the label: what the mechanism actually is, and what
    /// it does not cover.
    pub fn detail(self) -> &'static str {
        match self {
            ReadOnlyEnforcement::Driver => {
                "The database handle was opened in the engine's own read-only mode, so the engine \
                 rejects a write before it runs. This does not depend on the guard classifying \
                 your SQL correctly."
            }
            ReadOnlyEnforcement::ServerSession => {
                "The session was set read-only on the server, so the server rejects a write \
                 before it runs. This does not depend on the guard classifying your SQL \
                 correctly."
            }
            ReadOnlyEnforcement::RolledBackTransaction => {
                "This engine has no read-only mode for a session, so each statement runs inside a \
                 transaction that is always rolled back. A write therefore executes and is then \
                 undone rather than being stopped, and this is not a sandbox: anything that acts \
                 outside the transaction — a command shell, a linked server, an autonomous \
                 procedure — is not undone by the rollback."
            }
            ReadOnlyEnforcement::GuardOnly => {
                "Nothing on this connection is enforcing anything, so a heuristic over the SQL \
                 text is the only thing between you and a write. It abstains rather than \
                 guessing, but it is not a guarantee."
            }
            ReadOnlyEnforcement::NotEnforced => {
                "Nothing is enforcing read-only: writes were allowed on this connection, and the \
                 guard lets a recognised write through."
            }
            ReadOnlyEnforcement::Undetermined => {
                "Writes are not allowed, but it is not known what — if anything — the connection \
                 itself would do about one. Nothing here may be read as a guarantee."
            }
        }
    }

    /// The tone, without a mechanism. See [`EnforcementStrength`].
    pub fn strength(self) -> EnforcementStrength {
        match self {
            ReadOnlyEnforcement::Driver | ReadOnlyEnforcement::ServerSession => {
                EnforcementStrength::Refused
            }
            ReadOnlyEnforcement::RolledBackTransaction => EnforcementStrength::Undone,
            ReadOnlyEnforcement::GuardOnly => EnforcementStrength::HeuristicOnly,
            ReadOnlyEnforcement::NotEnforced => EnforcementStrength::Nothing,
            ReadOnlyEnforcement::Undetermined => EnforcementStrength::Unknown,
        }
    }

    /// Whether the write is stopped before the server executes it.
    ///
    /// **Not** the same question as [`is_driver_enforced`](Self::is_driver_enforced),
    /// and deliberately false for [`RolledBackTransaction`](ReadOnlyEnforcement::RolledBackTransaction):
    /// there the statement runs.
    pub fn refuses_writes_before_they_run(self) -> bool {
        self.strength() == EnforcementStrength::Refused
    }

    /// The narrow fact that the *handle* was opened read-only — SQLite's
    /// mechanism and nobody else's.
    ///
    /// A server-side session setting is just as strong and is still a different
    /// thing, so it is false here; ask
    /// [`refuses_writes_before_they_run`](Self::refuses_writes_before_they_run)
    /// for the strength question.
    pub fn is_driver_enforced(self) -> bool {
        matches!(self, ReadOnlyEnforcement::Driver)
    }
}

/// Every engine the console models.
///
/// Here rather than on [`SqlEngine`] because it exists for the total mappings
/// below and their tests. The mappings themselves are `match`es, so adding an
/// engine is a compile error rather than a silent default; this array only has
/// to be kept in step with the enum, and the tests over it would notice a new
/// engine that had been given the wrong wording.
pub const ENGINES: [SqlEngine; 3] = [SqlEngine::Sqlite, SqlEngine::Postgres, SqlEngine::SqlServer];

/// What an engine's driver enforces when writes are **not** allowed.
///
/// The single place each engine's real guarantee is written down, so a driver
/// cannot invent a stronger one and the UI does not have to know the mechanism.
/// A driver may only carry this forward onto a live connection once it has
/// actually applied it — see [`ReadOnlyEnforcement::ServerSession`].
pub fn declared_read_only_enforcement(engine: SqlEngine) -> ReadOnlyEnforcement {
    match engine {
        SqlEngine::Sqlite => ReadOnlyEnforcement::Driver,
        SqlEngine::Postgres => ReadOnlyEnforcement::ServerSession,
        SqlEngine::SqlServer => ReadOnlyEnforcement::RolledBackTransaction,
    }
}

/// What a connection *would* carry, answerable before anything is opened.
///
/// `None` for the engine is not a missing input to be defaulted: it is the
/// case where the engine could not be determined, and it yields
/// [`ReadOnlyEnforcement::Undetermined`] rather than the weakest or the
/// strongest of the real answers.
pub fn expected_enforcement(
    engine: Option<SqlEngine>,
    writes_allowed: bool,
) -> ReadOnlyEnforcement {
    if writes_allowed {
        return ReadOnlyEnforcement::NotEnforced;
    }
    match engine {
        Some(engine) => declared_read_only_enforcement(engine),
        None => ReadOnlyEnforcement::Undetermined,
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Where a failure happened. Kept apart because "the connection never opened"
/// and "the statement errored" are different situations for the reader, and
/// [`SqlEvent::Failed`] carries a `statement_index` for only one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorStage {
    /// Before anything was sent: a bad connection string, or a handle that
    /// would not open.
    Connect,
    /// The connection did not open within its deadline. Nothing was sent and
    /// no statement ran.
    ConnectTimeout,
    /// The statement was rejected or failed at the server.
    Execute,
}

/// A driver failure, already redacted.
///
/// Constructed **only** through [`DriverError::new`], which runs the text
/// through [`crate::sql::dsn::redact`]. Driver errors routinely embed the whole
/// connection string, and this type is the last place that can stop one before
/// it becomes a `SqlEvent::Failed` the frontend renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverError {
    pub stage: ErrorStage,
    pub message: String,
}

impl DriverError {
    pub fn new(stage: ErrorStage, message: impl AsRef<str>) -> Self {
        Self {
            stage,
            message: redact(message.as_ref()),
        }
    }

    pub fn connect(message: impl AsRef<str>) -> Self {
        Self::new(ErrorStage::Connect, message)
    }

    pub fn execute(message: impl AsRef<str>) -> Self {
        Self::new(ErrorStage::Execute, message)
    }

    /// The connection did not open in time.
    pub fn connect_timed_out(after: Duration) -> Self {
        Self::new(
            ErrorStage::ConnectTimeout,
            format!(
                "The connection did not open within {}. Nothing was sent, and no statement ran.",
                describe_duration(after)
            ),
        )
    }

    /// The wire event for this failure. `statement_index` is `None` for a
    /// connect-stage failure, which is why it is not defaulted to `0`.
    pub fn to_event(&self, statement_index: Option<usize>) -> SqlEvent {
        SqlEvent::Failed {
            statement_index: match self.stage {
                ErrorStage::Connect | ErrorStage::ConnectTimeout => None,
                ErrorStage::Execute => statement_index,
            },
            message: self.message.clone(),
        }
    }
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DriverError {}

pub type DriverResult<T> = Result<T, DriverError>;

/// A duration as a person reads it. Sub-second deadlines keep their
/// milliseconds; anything longer is rounded to a tenth of a second.
pub(crate) fn describe_duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.1} s", d.as_secs_f64())
    }
}

/// A boxed future, so the driver traits stay object-safe. `async fn` in a trait
/// is not `dyn`-compatible on this toolchain, and the whole point of the seam is
/// that the caller holds a `dyn SqlDriver` without knowing which engine it is.
pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = DriverResult<T>> + Send + 'a>>;

/// Run a driver connect under a deadline.
///
/// The one place the timeout outcome is built, so no driver can spell it as an
/// ordinary connect failure. Two distinct answers stay distinct: an engine that
/// *refused* the handle (bad path, bad credentials, read-only file) is
/// [`ErrorStage::Connect`], and one that never answered is
/// [`ErrorStage::ConnectTimeout`]. Neither is a user stop — a stop is an `Ok`
/// outcome on a statement that ran, and there is no statement here.
///
/// The pending connect is dropped when the deadline passes. That is what
/// abandons it; whether the engine notices promptly is the engine’s business,
/// and this deliberately does not wait to find out.
pub async fn within_connect_deadline<T, F>(deadline: Duration, connect: F) -> DriverResult<T>
where
    F: Future<Output = DriverResult<T>>,
{
    match tokio::time::timeout(deadline, connect).await {
        Ok(result) => result,
        Err(_) => Err(DriverError::connect_timed_out(deadline)),
    }
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// What the caller asks a driver to open.
///
/// `dsn` travels in one direction only and is never stored on the connection,
/// logged, or echoed into an error — see the subsystem docs.
#[derive(Debug, Clone)]
pub struct ConnectSpec {
    pub dsn: String,
    /// The connection's writes-allowed setting. `false` asks the driver to open
    /// a handle that will itself refuse a write, where the engine has one.
    pub writes_allowed: bool,
}

/// How one statement ended.
///
/// Two terminal states here, and a failure is the `Err` arm rather than a third
/// variant, so *finished*, *stopped* and *failed* can never be read as one
/// another. A stopped statement still carries the rows that did arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementOutcome {
    /// The statement ran to the end of its rows (or to a cap).
    Finished(SqlResultSet),
    /// The caller asked to stop. Streaming ended and the connection is dropped;
    /// the server may still be executing. Not a server-side cancel.
    Stopped(SqlResultSet),
}

impl StatementOutcome {
    pub fn result(&self) -> &SqlResultSet {
        match self {
            StatementOutcome::Finished(r) | StatementOutcome::Stopped(r) => r,
        }
    }

    pub fn was_stopped(&self) -> bool {
        matches!(self, StatementOutcome::Stopped(_))
    }
}

/// Opens connections for one engine.
pub trait SqlDriver: Send + Sync {
    fn engine(&self) -> SqlEngine;

    /// What [`ReadOnlyEnforcement`] a connection opened with this setting would
    /// carry. Answerable without opening anything, so the UI can say what it is
    /// about to do.
    fn read_only_enforcement(&self, writes_allowed: bool) -> ReadOnlyEnforcement;

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> DriverFuture<'a, Box<dyn SqlConnection>>;
}

/// One open connection. Not `Sync`: a statement borrows it mutably.
pub trait SqlConnection: Send {
    fn engine(&self) -> SqlEngine;

    /// What is actually enforcing read-only on *this* handle.
    fn read_only_enforcement(&self) -> ReadOnlyEnforcement;

    /// Run one statement, streaming rows to `events` as they are read.
    ///
    /// `sql` is a **single** statement; splitting a script is the caller's job,
    /// which is why `statement_index` is passed in rather than counted here.
    /// The guard has already run — this layer does not classify SQL.
    fn run_statement<'a>(
        &'a mut self,
        sql: &'a str,
        statement_index: usize,
        limits: Limits,
        events: &'a mpsc::Sender<SqlEvent>,
        stop: StopSignal,
    ) -> DriverFuture<'a, StatementOutcome>;
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// The driver for an engine, or [`None`] when this build has none.
///
/// The one place an engine is mapped to a driver. It lives here rather than in
/// `src-tauri` so it can be tested: a `#[tauri::command]` module cannot be, and
/// "registered under the wrong engine" is exactly the mistake that would open a
/// SQLite file for a PostgreSQL DSN.
///
/// [`None`] is a real answer — "no driver in this build" is not "the engine
/// could not be determined" and not "the connection failed", and the callers
/// keep all three apart.
pub fn for_engine(engine: SqlEngine) -> Option<Box<dyn SqlDriver>> {
    match engine {
        SqlEngine::Sqlite => Some(Box::new(sqlite::SqliteDriver::new())),
        // Whatever an arm returns must satisfy the two registry tests:
        // `engine()` matches the arm, and `read_only_enforcement(false)` equals
        // `declared_read_only_enforcement(engine)`.
        SqlEngine::Postgres => Some(Box::new(postgres::PostgresDriver::new())),
        SqlEngine::SqlServer => Some(Box::new(mssql::MssqlDriver::new())),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
