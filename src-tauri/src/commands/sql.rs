//! The SQL console's bridge: list and save connection profiles, discover the
//! ones a workspace already mentions, test one, and run a statement.
//!
//! Thin, like the rest of this layer. Everything that decides anything — how a
//! connection string is redacted, whether a statement is a read, what a cap
//! means, how a stop is recorded — is in [`cb_core::sql`], which is tested with
//! no database software installed. What is left here is resolving state,
//! cloning a handle, and awaiting; plus the handful of decisions the bridge
//! itself has to make, each extracted to a free function below and tested.
//!
//! # Nothing here sends a connection string toward the frontend
//!
//! Rule 3 of the subsystem docs — *a connection string crosses IPC in one
//! direction only* — is enforced at this layer, because this layer is where the
//! wire types are chosen. So no command returns a
//! [`cb_core::sql::store::SqlConnection`]: it can hold a
//! [`SecretSource::Literal`], which is a password. Every read-back goes through
//! [`redact_connection`], whose [`SqlSecretView`] has no variant that could
//! carry one — a literal is described only by [`dsn::display_form`], the
//! redacted view, and the *references* describe themselves by naming a file and
//! a key.
//!
//! Errors are the other leak, and the more likely one: a driver failure
//! routinely embeds the whole DSN. [`cb_core::sql::driver::DriverError`] already
//! redacts on construction; every message this module builds is run through
//! [`dsn::redact`] again on the way out, because the cost of the second pass is
//! nothing and the cost of missing one is permanent.
//!
//! # What moved to `cb-core`, and why it had to
//!
//! [`cb_core::sql::plan`] holds `Plan`, `Refusal` and `execution_plan`;
//! [`cb_core::sql::catalog`] holds the explorer's queries, its row readers and
//! the wire types this module re-exports under their old names. They are not
//! there merely because a command body must not decide anything: the MCP server
//! is a **separate process with no `AppState`**, so it cannot call any
//! `#[tauri::command]`, and anything the human console and an agent must agree
//! about has to sit where both can reach it. What stays here is the wrapping
//! that only this layer can do — [`refusal_event`] turns a refusal into a
//! redacted [`SqlEvent`]; the words themselves come from [`Refusal::sentence`],
//! so the two surfaces cannot describe one refusal differently.
//!
//! # Consent is its own verb — twice
//!
//! [`sql_set_allow_writes`] exists so that turning off the read-only guard is a
//! thing the user does deliberately, and [`upsert`] is what makes that true
//! rather than decorative: a save **ignores** the incoming `allow_writes` and
//! keeps whatever is stored (a new profile starts at `false`). Otherwise a
//! rename — or any UI round-trip that posted the profile back — could raise
//! consent silently, which is exactly what a separate verb is supposed to
//! prevent.
//!
//! [`sql_set_expose_to_agents`] is the second, and the stronger: it decides
//! whether an agent may see a connection **at all**. [`upsert`] guards it the
//! same way — `expose_to_agents` is forced to `false` for a new profile and
//! left untouched on an update — and the two flags are orthogonal, because the
//! agent path forces `writes_allowed: false` regardless of `allow_writes`. No
//! combination of them lets an agent write.
//!
//! # One statement, index 0
//!
//! [`cb_core::sql::driver::SqlConnection::run_statement`] takes a *single*
//! statement and says splitting a script is the caller's job. This module does
//! not split one. There is no splitter in `cb-core`, and the obvious one — cut
//! on `;` — is precisely the guessing this subsystem refuses: a semicolon
//! inside a string literal, a dollar-quoted body or a `BEGIN … END` block would
//! cut a statement in half and send the pieces. So the submitted text is
//! guarded as one batch (which [`cb_core::sql::guard`] handles: it parses every
//! statement in it) and run as one, at `statement_index` 0. When a real
//! splitter lands in `cb-core`, this is the one place that changes.
//!
//! # "I connected" is not "this is a database"
//!
//! [`sql_test_connection`] opens a handle **and then reads a page**, because
//! for SQLite the first is no evidence of the second: `sqlite3` defers its
//! header check until something reads, and `select sqlite_version()` is a pure
//! function that never does — so a profile pointing at a `README.md` used to
//! connect, answer with a version, and be reported as a working connection.
//! [`probe_query`] is the read, and [`SqlTestOutcome::NotADatabase`] is the
//! answer it can give. The connect stage and the probe stage also speak
//! different vocabularies, which is why there are two classifiers: the words
//! `file is not a database` belong only to the second.
//!
//! # Nothing is computed and dropped
//!
//! [`Plan::note`] is the guard's sentence for a statement that is not a plain
//! read — including an *allowed* write, which still says what it is. It is
//! delivered by [`notice_event`]. A value derived and then discarded is a claim
//! the code does not keep, and this one is the sentence the user most needs.

use std::path::PathBuf;

use cb_core::mcp::answer::ConnectionStatusKind;
use cb_core::sql::catalog;
use cb_core::sql::discover::{self, Discovery, DiscoveryOptions};
use cb_core::sql::dotenv::EnvValue;
use cb_core::sql::driver::{ConnectSpec, DriverError, Limits, SqlDriver, StatementOutcome};
use cb_core::sql::dsn::{self, SqlConnectionDisplay, SqlEngine};
use cb_core::sql::model::{SqlEvent, SqlResultSet, SqlValue};
use cb_core::sql::plan::{engine_name, execution_plan, Plan, Refusal};
use cb_core::sql::session::{RegisterError, SqlSessions, StopOutcome};
use cb_core::sql::store::{
    self, SecretSource, SqlConnection as StoredConnection, SqlConnectionsFile,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// The explorer's wire types, which now live in `cb-core`
// ---------------------------------------------------------------------------

// Re-exported under the names the frontend already reads, so the emitted JSON
// is byte-identical and `src/ipc/types.ts` needs no edit. They moved to
// [`cb_core::sql::catalog`] because the MCP server is a separate process with
// no `AppState` and cannot call a command to obtain them.
pub use cb_core::sql::catalog::SqlColumn as SqlColumnView;
pub use cb_core::sql::catalog::SqlObject as SqlObjectView;
// Part of `SqlObjectView`'s public shape even where this crate does not name
// it, so it is re-exported alongside.
#[allow(unused_imports)]
pub use cb_core::sql::catalog::SqlObjectKind;

// ---------------------------------------------------------------------------
// The redacted view of a saved profile
// ---------------------------------------------------------------------------

/// Where a saved profile's connection string comes from, in the form the
/// frontend is allowed to see.
///
/// Deliberately **not** [`SecretSource`], which has a variant holding a
/// password. The shape is the same for the three reference variants — a file
/// and a key is not a secret, and naming it is what lets the UI say where a
/// connection is defined — and the literal is replaced by its redacted
/// description. There is no variant here that can carry a connection string, so
/// this rule cannot be broken by a later field being added to the wrong place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SqlSecretView {
    /// The user typed a string and asked for it to be remembered. Described,
    /// never quoted.
    Literal {
        display: SqlConnectionDisplay,
    },
    AppSettings {
        path: PathBuf,
        key: String,
    },
    UserSecrets {
        project: PathBuf,
        key: String,
    },
    DotEnv {
        path: PathBuf,
        key: String,
    },
}

/// One saved connection, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SqlConnectionView {
    pub id: String,
    pub name: String,
    /// [`None`] means *not determined or ambiguous*, never a default.
    pub engine: Option<SqlEngine>,
    pub secret: SqlSecretView,
    /// Whether the stored profile holds a secret **value** rather than a
    /// reference to one — read from
    /// [`SecretSource::holds_a_secret`], the single place that rule lives, so
    /// the UI can warn without re-deriving it.
    pub holds_a_secret: bool,
    pub workspace_root: Option<PathBuf>,
    pub allow_writes: bool,
    /// Whether an agent may see this connection at all, through the MCP server.
    ///
    /// A separate fact from `allow_writes` and never derived from it: the agent
    /// path forces `writes_allowed: false` regardless, so this answers only
    /// *may an agent read here*. It crosses so the picker can badge it and
    /// offer the toggle.
    pub expose_to_agents: bool,
    /// Whether the user typed this name. The picker derives a composite label
    /// from the reference when it did not, and shows `name` verbatim when it
    /// did — so this has to cross, or the derivation would reappear on a
    /// renamed connection the moment the list is re-read.
    pub user_named: bool,
    pub created_at_ms: u64,
    pub last_used_ms: Option<u64>,
}

/// Strip a stored profile down to what may cross toward the frontend.
///
/// The one seam where a stored secret could escape, so it is a free function
/// with a test that serialises a profile holding a password and asserts the
/// password is nowhere in the JSON.
pub(crate) fn redact_connection(stored: &StoredConnection) -> SqlConnectionView {
    let secret = match &stored.secret {
        SecretSource::Literal { connection_string } => SqlSecretView::Literal {
            display: dsn::display_form(connection_string),
        },
        SecretSource::AppSettings { path, key } => SqlSecretView::AppSettings {
            path: path.clone(),
            key: key.clone(),
        },
        SecretSource::UserSecrets { project, key } => SqlSecretView::UserSecrets {
            project: project.clone(),
            key: key.clone(),
        },
        SecretSource::DotEnv { path, key } => SqlSecretView::DotEnv {
            path: path.clone(),
            key: key.clone(),
        },
    };
    SqlConnectionView {
        id: stored.id.clone(),
        name: stored.name.clone(),
        engine: stored.engine,
        secret,
        holds_a_secret: stored.secret.holds_a_secret(),
        workspace_root: stored.workspace_root.clone(),
        allow_writes: stored.allow_writes,
        expose_to_agents: stored.expose_to_agents,
        user_named: stored.user_named,
        created_at_ms: stored.created_at_ms,
        last_used_ms: stored.last_used_ms,
    }
}

fn views(file: &SqlConnectionsFile) -> Vec<SqlConnectionView> {
    file.connections.iter().map(redact_connection).collect()
}

// ---------------------------------------------------------------------------
// Store mutations
// ---------------------------------------------------------------------------

/// Add or replace a profile, **without** letting the payload change consent.
///
/// See the module docs: `allow_writes` is only ever moved by
/// [`sql_set_allow_writes`]. An update also keeps the stored `created_at_ms`
/// and `last_used_ms`, which belong to the record rather than to the form the
/// user just submitted.
pub(crate) fn upsert(file: &mut SqlConnectionsFile, incoming: StoredConnection) {
    match file.connections.iter_mut().find(|c| c.id == incoming.id) {
        Some(existing) => {
            // A name the user typed outranks the one the payload carries, for
            // the same reason consent does: re-adopting a discovered connection
            // (a rescan, a re-save from the form) must not silently undo a
            // rename. `user_named` is likewise only ever set by [`rename`].
            if !existing.user_named {
                existing.name = incoming.name;
            }
            existing.engine = incoming.engine;
            existing.secret = incoming.secret;
            existing.workspace_root = incoming.workspace_root;
        }
        None => {
            let mut fresh = incoming;
            fresh.allow_writes = false;
            // Agent exposure is consent too, and a stronger one: it is the
            // whole of what an agent may see. A new profile therefore starts
            // unexposed whatever the payload claims, exactly as `allow_writes`
            // does — and the `Some` arm says nothing about it at all, so no
            // save or rename round-trip can raise it either.
            fresh.expose_to_agents = false;
            fresh.user_named = false;
            file.connections.push(fresh);
        }
    }
}

/// Forget a profile. Returns whether one was there — "it was already gone" is
/// not the same answer as "it has been removed".
pub(crate) fn remove(file: &mut SqlConnectionsFile, id: &str) -> bool {
    let before = file.connections.len();
    file.connections.retain(|c| c.id != id);
    file.connections.len() != before
}

/// The rename action, and the only thing that sets `user_named`.
///
/// Its own verb rather than a round-trip through [`upsert`], and not for tidiness:
/// a [`SqlConnectionView`] carries a **redacted** secret, so a caller holding one
/// cannot rebuild the [`StoredConnection`] an upsert needs — a rename posted that
/// way would replace a stored password with its own display form and break the
/// connection. Taking only the two fields a rename is about makes that impossible
/// rather than merely unlikely.
///
/// `name` is stored as given. The cleaning and refusal rule lives at the one
/// frontend seam (`acceptedConnectionName`), so a name that reaches here has
/// already been accepted; the store's job is to keep it.
pub(crate) fn rename(file: &mut SqlConnectionsFile, id: &str, name: String) -> bool {
    match file.connections.iter_mut().find(|c| c.id == id) {
        Some(entry) => {
            entry.name = name;
            entry.user_named = true;
            true
        }
        None => false,
    }
}

/// The consent action, and the only thing that moves `allow_writes`.
pub(crate) fn set_allow_writes(file: &mut SqlConnectionsFile, id: &str, allow: bool) -> bool {
    match file.connections.iter_mut().find(|c| c.id == id) {
        Some(entry) => {
            entry.allow_writes = allow;
            true
        }
        None => false,
    }
}

/// The agent-exposure action, and the only thing that moves
/// `expose_to_agents`.
///
/// Its own verb for the same reason [`set_allow_writes`] is one, and the reason
/// is sharper here: exposure is what an agent may see *at all*, and burying it
/// in [`sql_save_connection`] would let a form round-trip hand a database to an
/// agent without the user ever saying so.
pub(crate) fn set_expose_to_agents(file: &mut SqlConnectionsFile, id: &str, expose: bool) -> bool {
    match file.connections.iter_mut().find(|c| c.id == id) {
        Some(entry) => {
            entry.expose_to_agents = expose;
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Engines and drivers
// ---------------------------------------------------------------------------

/// The driver for an engine, or [`None`] when this build has none.
///
/// A one-line delegation on purpose: the registry lives in `cb_core::sql::driver`
/// where it is unit-testable, and this crate's command modules decide nothing.
/// "No driver in this build" is a different answer from "the engine could not be
/// determined", and [`SqlTestOutcome`] and [`Refusal`] keep them apart rather
/// than reporting both as a failure to connect.
pub(crate) fn driver_for(engine: SqlEngine) -> Option<Box<dyn SqlDriver>> {
    cb_core::sql::driver::for_engine(engine)
}

// ---------------------------------------------------------------------------
// Resolving the secret
// ---------------------------------------------------------------------------

/// Re-read the connection string a profile points at.
///
/// `Err` is a sentence naming what could not be read; it never quotes what was
/// read, and it is redacted on the way out anyway.
pub(crate) fn resolve_dsn(source: &SecretSource) -> Result<String, String> {
    match discover::read_value(source, &DiscoveryOptions::default()) {
        Ok(EnvValue::Literal { text }) if text.trim().is_empty() => {
            Err("the configured connection string is empty".to_string())
        }
        Ok(EnvValue::Literal { text }) => Ok(text),
        // The value is still a `${...}` reference: there is nothing to connect
        // *to* yet, which is not the same as a connection that failed.
        Ok(EnvValue::Unresolved { reason, .. }) => Err(dsn::redact(&reason)),
        Err(why) => Err(dsn::redact(&why)),
    }
}

// ---------------------------------------------------------------------------
// The execution plan, which now lives in `cb-core`
// ---------------------------------------------------------------------------

/// The wire event for a refusal.
///
/// The *words* live in [`Refusal::sentence`], in `cb-core`, so the console and
/// an agent cannot describe one refusal two different ways. Only the wrapping
/// is here, because only this layer has [`SqlEvent`] and the redacting
/// constructor every failure message must pass through.
///
/// Only the guard's refusal is a [`SqlEvent::Refused`] — that variant means
/// *the guard refused, so nothing was sent to the database*. An engine that
/// could not be resolved never reached the guard, so it is a
/// [`SqlEvent::Failed`] with `statement_index: None`, the shape that means
/// "before any statement ran".
pub(crate) fn refusal_event(refusal: &Refusal) -> SqlEvent {
    match refusal {
        Refusal::Guard { reason } => SqlEvent::Refused {
            statement_index: 0,
            reason: reason.clone(),
        },
        // Through [`failed_event`] and never around it: that is the one
        // constructor this module's docs promise, and it is where the
        // redaction lives.
        other => failed_event(None, &other.sentence()),
    }
}

// ---------------------------------------------------------------------------
// Testing a connection
// ---------------------------------------------------------------------------

/// What a connection test found.
///
/// A variant enum and not a boolean, for the reason
/// [`cb_core::lsp::model::Availability`] is one: a wrong password, an
/// unreachable host, a TLS handshake that failed, a timeout, a secret that
/// could not be read and an engine nobody determined are six different things
/// the user does six different things about. "Could not connect" would send
/// them to check the network for a typo in a password.
///
/// [`SqlTestOutcome::Failed`] is the abstention: a driver message this build
/// does not recognise must not be filed under whichever category looks closest.
///
/// Two of the variants are here because *opening a handle* is not *finding a
/// database*, and SQLite proves it — `sqlite3` defers its header check to the
/// first page read, so a connection to a `README.md` opens perfectly and
/// answers `select sqlite_version()`, a pure function that reads no page, with
/// a version number. [`SqlTestOutcome::NotADatabase`] is what a probe that
/// really reads a page reports, and [`SqlTestOutcome::CannotOpenFile`] is the
/// separate answer for a path that would not open at all — which used to be
/// filed as [`SqlTestOutcome::Unreachable`], sending the reader to check a
/// network over a wrong path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SqlTestOutcome {
    /// The handle opened. `server_version` is [`None`] when this build has no
    /// version query for the engine, or the server answered with something that
    /// is not a version — never a placeholder string that would read as one.
    Ok {
        server_version: Option<String>,
    },
    AuthFailed {
        message: String,
    },
    /// A host that could not be reached. **Network only** — a database *file*
    /// that would not open is [`SqlTestOutcome::CannotOpenFile`].
    Unreachable {
        message: String,
    },
    /// The connection string named a database file the driver could not open.
    /// It may not exist, it may be a directory, or it may not be readable:
    /// SQLite reports all three as `unable to open database file` and this
    /// build does not guess which of them it was.
    CannotOpenFile {
        message: String,
    },
    /// The handle opened, and what is behind it is not a database this build
    /// can read — a file that is not one at all, or an encrypted one.
    ///
    /// Distinct from [`SqlTestOutcome::Ok`] for the reason the whole enum
    /// exists: SQLite opens any file and only fails when a page is read, so "I
    /// connected" and "this is a database" are two facts, and reporting the
    /// first as the second is wrong about the only question the user asked.
    NotADatabase {
        message: String,
    },
    TlsFailed {
        message: String,
    },
    /// `after_ms` is [`Some`] when *this* app's timeout bit, so it can say how
    /// long it waited, and [`None`] when the driver itself reported a timeout —
    /// whose duration this layer does not know and will not invent.
    Timeout {
        after_ms: Option<u64>,
    },
    /// The profile's engine was never determined. Nothing was attempted.
    EngineUnknown,
    /// The engine is known and this build has no driver for it. Distinct from
    /// [`SqlTestOutcome::EngineUnknown`]: one is answered by choosing an
    /// engine, the other by waiting for a release.
    EngineUnsupported {
        engine: SqlEngine,
    },
    /// The connection string could not be re-read from where the profile says
    /// it lives, or it is still an unexpanded reference. Nothing was attempted.
    SecretUnresolved {
        reason: String,
    },
    /// The attempt failed and this build does not recognise the reason. The
    /// driver's own redacted words, unclassified.
    Failed {
        message: String,
    },
}

/// The message-free counterpart of an outcome, for the MCP server.
///
/// Seven of the variants above carry a `message`, and every one of those
/// messages came from a database driver. That is right for the human console —
/// it is the user's own machine and their own database — and wrong for an agent,
/// which will paste what it is told into a transcript, a commit message and a
/// model provider's logs. `dsn::redact` is a keyword heuristic, so this mapping
/// does not filter the words: it **drops them**, and what crosses is a variant.
///
/// The mapping is an exhaustive `match` with no catch-all arm, so a new outcome
/// fails to compile here rather than being quietly filed under
/// [`ConnectionStatusKind::Failed`] — which would be the one arm that could
/// carry a message and does not.
///
/// This lives in the bridge rather than in `cb-core` because
/// [`SqlTestOutcome`] does: the classification needs the driver's words, and
/// only this layer ever has them.
/// Reached from the MCP runner ([`crate::mcp_sql`]) as well as this module's
/// own tests: the mapping is what a future variant has to pass through, and it
/// fails to compile rather than leaking a message.
pub(crate) fn connection_status_kind(outcome: &SqlTestOutcome) -> ConnectionStatusKind {
    match outcome {
        SqlTestOutcome::Ok { .. } => ConnectionStatusKind::Ok,
        SqlTestOutcome::AuthFailed { .. } => ConnectionStatusKind::AuthFailed,
        SqlTestOutcome::Unreachable { .. } => ConnectionStatusKind::Unreachable,
        SqlTestOutcome::CannotOpenFile { .. } => ConnectionStatusKind::CannotOpenFile,
        SqlTestOutcome::NotADatabase { .. } => ConnectionStatusKind::NotADatabase,
        SqlTestOutcome::TlsFailed { .. } => ConnectionStatusKind::TlsFailed,
        SqlTestOutcome::Timeout { .. } => ConnectionStatusKind::Timeout,
        SqlTestOutcome::EngineUnknown => ConnectionStatusKind::EngineUnknown,
        SqlTestOutcome::EngineUnsupported { .. } => ConnectionStatusKind::EngineUnsupported,
        SqlTestOutcome::SecretUnresolved { .. } => ConnectionStatusKind::SecretUnresolved,
        SqlTestOutcome::Failed { .. } => ConnectionStatusKind::Failed,
    }
}

/// How long a connection test waits before reporting a timeout.
pub(crate) const TEST_TIMEOUT_MS: u64 = 10_000;

/// Turn a driver's connect-stage message into an outcome, or abstain.
///
/// Matching is on lowercased substrings, and the order matters: a TLS failure's
/// message often also mentions the host, and an authentication failure often
/// also mentions the connection. The fallback is
/// [`SqlTestOutcome::Failed`] — never [`SqlTestOutcome::Unreachable`], which
/// would send somebody to check a firewall over a message this build simply had
/// no rule for.
///
/// Every message is passed through [`dsn::redact`], because this is the last
/// place before it becomes something the frontend renders.
///
/// # What is deliberately not here
///
/// `file is not a database` and `file is encrypted` used to be listed under
/// [`SqlTestOutcome::AuthFailed`], and they were dead needles: sqlite3 defers
/// its header check to the first page read, so those words never appear at the
/// *connect* stage at all. They belong to [`classify_probe_failure`], which
/// classifies the failure of a statement that actually reads a page.
///
/// The network needles are also checked **before** the file ones, which is not
/// cosmetic: a PostgreSQL unix-socket failure reads `could not connect to
/// server: No such file or directory`, and it is the first half of that
/// sentence that is true of it.
pub(crate) fn classify_connect_failure(message: &str) -> SqlTestOutcome {
    let text = message.to_ascii_lowercase();
    let message = dsn::redact(message);
    let has = |needle: &str| text.contains(needle);

    if has("certificate") || has("tls") || has("ssl") || has("handshake") {
        return SqlTestOutcome::TlsFailed { message };
    }
    if has("login failed")
        || has("authentication failed")
        || has("password authentication")
        || has("access denied")
        || has("not authorized")
        || has("permission denied")
    {
        return SqlTestOutcome::AuthFailed { message };
    }
    if has("timed out") || has("timeout") {
        // The driver reported it, so the duration is the driver's, not ours.
        return SqlTestOutcome::Timeout { after_ms: None };
    }
    if has("connection refused")
        || has("could not connect")
        || has("no such host")
        || has("network is unreachable")
        || has("host is unreachable")
    {
        return SqlTestOutcome::Unreachable { message };
    }
    // A file, not a host. `unable to open database file` is what sqlite3
    // reports for a missing path, a directory and an unreadable file alike; the
    // other two spellings are not produced by the SQLite driver in this build
    // and so are not pinned by a test — see `classify_probe_failure`.
    if has("unable to open database file") || has("no such file") || has("cannot find the file") {
        return SqlTestOutcome::CannotOpenFile { message };
    }
    SqlTestOutcome::Failed { message }
}

/// Turn a **probe**-stage message into an outcome, or abstain.
///
/// The probe is the statement that proves there is a database behind the handle
/// (see [`probe_query`]), so its failures are a different vocabulary from the
/// connect stage's: this is where `file is not a database` actually appears.
///
/// The fallback is [`SqlTestOutcome::Failed`] and **not**
/// [`SqlTestOutcome::Ok`]: the probe exists to confirm something, and a probe
/// that errored for a reason this build has no rule for has confirmed nothing.
pub(crate) fn classify_probe_failure(message: &str) -> SqlTestOutcome {
    let text = message.to_ascii_lowercase();
    let message = dsn::redact(message);

    if text.contains("file is not a database") || text.contains("file is encrypted") {
        return SqlTestOutcome::NotADatabase { message };
    }
    SqlTestOutcome::Failed { message }
}

/// The statement a connection test runs to prove the handle really opened a
/// database.
///
/// **Not the version query, and that distinction is the whole point.**
/// `select sqlite_version()` is a pure SQL function: it reads no page, so it
/// answers happily on a handle opened over a `README.md`, and a test built on
/// it reported `Ok { server_version: "3.46.0" }` for a text file. sqlite3 only
/// checks the file header when something reads a page, and `sqlite_master` is
/// that read.
///
/// [`None`] means *this engine needs no such statement*: for a client/server
/// engine the connect handshake has already reached a real database, and there
/// is no equivalent of a file that opens and turns out not to be one.
pub(crate) fn probe_query(engine: SqlEngine) -> Option<&'static str> {
    match engine {
        // A read, so it runs on the read-only handle the profile asks for, and
        // it is true of an empty database (it answers 0) as well as a full one.
        SqlEngine::Sqlite => Some("select count(*) from sqlite_master"),
        SqlEngine::SqlServer | SqlEngine::Postgres => None,
    }
}

/// The statement that asks an engine its version, when this build knows one.
///
/// [`None`] rather than a guess: a version query is engine-specific SQL, and
/// running the wrong one produces an error the user would read as a broken
/// connection.
pub(crate) fn version_query(engine: SqlEngine) -> Option<&'static str> {
    match engine {
        SqlEngine::Sqlite => Some("select sqlite_version()"),
        SqlEngine::SqlServer | SqlEngine::Postgres => None,
    }
}

/// The version out of a version query's result, or [`None`].
///
/// Only a value the driver actually rendered as text or a number counts. A
/// `NULL`, a cell this build could not decode, or an empty result set is *no
/// version reported* — which is what [`SqlTestOutcome::Ok`] carries as
/// [`None`], and is a different statement from a version of `""`.
pub(crate) fn server_version_from(result: &SqlResultSet) -> Option<String> {
    match result.rows.first().and_then(|row| row.first()) {
        Some(SqlValue::Text { text, .. }) | Some(SqlValue::Number { text }) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

/// What a stop request did, on the wire.
///
/// Mirrors [`StopOutcome`], which is not a serialisable type. Three answers:
/// stopping something that has already finished is a different fact from
/// stopping something that was running, and reporting both as success would
/// make a race look like a working feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SqlStopOutcome {
    Signalled,
    AlreadyStopping,
    NotFound,
}

pub(crate) fn stop_outcome(outcome: StopOutcome) -> SqlStopOutcome {
    match outcome {
        StopOutcome::Signalled => SqlStopOutcome::Signalled,
        StopOutcome::AlreadyStopping => SqlStopOutcome::AlreadyStopping,
        StopOutcome::NotFound => SqlStopOutcome::NotFound,
    }
}

/// Stop one query, found by its id alone.
///
/// The registry is keyed by connection *then* query, because a connection is
/// what gets closed and a query is what gets stopped; the frontend addresses a
/// running statement by the query id it minted, so this finds the connection
/// first. A query id nobody is running is [`SqlStopOutcome::NotFound`], the same
/// answer the registry would give — not an error, because a Stop click racing a
/// statement that just finished is ordinary.
pub(crate) fn stop_query(sessions: &SqlSessions, query_id: &str) -> SqlStopOutcome {
    for connection in sessions.connections() {
        if sessions.is_running(&connection, query_id) {
            return stop_outcome(sessions.stop(&connection, query_id));
        }
    }
    SqlStopOutcome::NotFound
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// A failure event, redacted.
///
/// The one constructor for [`SqlEvent::Failed`] in this module, so no call site
/// can build one out of un-redacted text.
pub(crate) fn failed_event(statement_index: Option<usize>, message: &str) -> SqlEvent {
    SqlEvent::Failed {
        statement_index,
        message: dsn::redact(message),
    }
}

/// The guard's note about how a planned statement will be run, as the event
/// that delivers it, or [`None`] for a plain read.
///
/// [`Plan::note`] was computed and then dropped, which made the guard's most
/// important sentence — *this is a write, and writes are allowed on this
/// connection* — invisible to the only person entitled to read it. A value
/// derived and discarded is a claim the code does not keep.
///
/// A [`SqlEvent::Notice`] and not a `Failed` or a `Refused`: the statement is
/// about to run, and both of those would say that it is not.
pub(crate) fn notice_event(plan: &Plan) -> Option<SqlEvent> {
    plan.note.as_ref().map(|note| SqlEvent::Notice {
        statement_index: 0,
        message: dsn::redact(note),
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Every saved connection, redacted. Takes no `AppState`: the store is
/// user-global, like [`crate::commands::notes`].
#[tauri::command]
pub async fn sql_list_connections() -> Result<Vec<SqlConnectionView>, String> {
    Ok(views(&store::load(&store::sql_connections_path())))
}

/// The connections a workspace mentions. Reads files; connects to nothing and
/// saves nothing.
#[tauri::command]
pub async fn sql_discover(root: String) -> Result<Discovery, String> {
    let path = PathBuf::from(&root);
    // Rescan means a fresh walk: the open workspace snapshot cannot know about
    // a project added since the tab opened.
    let workspace = cb_core::workspace::workspace_from_dir(&path).map_err(|e| format!("{e:#}"))?;
    Ok(discover::discover(&workspace, &DiscoveryOptions::default()))
}

/// Add or update a saved connection, and hand back the redacted list.
///
/// The incoming `allowWrites` is **ignored** — see [`upsert`] and the module
/// docs. Consent moves only through [`sql_set_allow_writes`].
#[tauri::command]
pub async fn sql_save_connection(
    connection: StoredConnection,
) -> Result<Vec<SqlConnectionView>, String> {
    let path = store::sql_connections_path();
    let mut file = store::load(&path);
    upsert(&mut file, connection);
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

/// Forget a saved connection.
#[tauri::command]
pub async fn sql_delete_connection(id: String) -> Result<Vec<SqlConnectionView>, String> {
    let path = store::sql_connections_path();
    let mut file = store::load(&path);
    if !remove(&mut file, &id) {
        return Err(format!("no connection named {id}"));
    }
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

/// Rename a saved connection.
///
/// Hands back the whole redacted list, as every other mutation here does, so the
/// picker replaces its state from one answer rather than patching a row.
#[tauri::command]
pub async fn sql_rename_connection(
    id: String,
    name: String,
) -> Result<Vec<SqlConnectionView>, String> {
    let path = store::sql_connections_path();
    let mut file = store::load(&path);
    if !rename(&mut file, &id, name) {
        return Err(format!("no connection named {id}"));
    }
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

/// Allow or disallow writes on one connection.
///
/// Its own verb on purpose: this is the consent action, and burying it inside
/// [`sql_save_connection`] would let a form round-trip turn the read-only guard
/// off without the user ever saying so.
#[tauri::command]
pub async fn sql_set_allow_writes(
    id: String,
    allow_writes: bool,
) -> Result<Vec<SqlConnectionView>, String> {
    let path = store::sql_connections_path();
    let mut file = store::load(&path);
    if !set_allow_writes(&mut file, &id, allow_writes) {
        return Err(format!("no connection named {id}"));
    }
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

/// Expose or un-expose one connection to agents through the MCP server.
///
/// Its own verb on purpose, mirroring [`sql_set_allow_writes`]. The two flags
/// are orthogonal: the agent path forces `writes_allowed: false` regardless of
/// `allow_writes`, so no combination of them lets an agent write. What this one
/// grants is *reading*, and an agent with read access can read anything that
/// login can read — including credentials the database itself stores. Expose
/// only connections whose login you would give a colleague read access to.
///
/// Un-exposing takes effect on the next call: the MCP server re-reads the store
/// per request and caches no connection.
#[tauri::command]
pub async fn sql_set_expose_to_agents(
    id: String,
    expose_to_agents: bool,
) -> Result<Vec<SqlConnectionView>, String> {
    let path = store::sql_connections_path();
    let mut file = store::load(&path);
    if !set_expose_to_agents(&mut file, &id, expose_to_agents) {
        return Err(format!("no connection named {id}"));
    }
    store::save(&path, &file).map_err(|e| format!("{e:#}"))?;
    Ok(views(&file))
}

/// Run `probe` under one deadline, and report a bite as a timeout that knows
/// how long it waited.
///
/// Generic over the future on purpose: that is what guarantees the deadline
/// covers **the whole** attempt rather than a stage of it. The bug it closes is
/// that the timeout used to wrap only `driver.connect(..)`, so a server that
/// accepted a connection and then never answered the probe was reported as
/// anything but [`SqlTestOutcome::Timeout`] — the app waited as long as the
/// driver did, then named the wrong cause.
pub(crate) async fn within_timeout<F>(timeout_ms: u64, probe: F) -> SqlTestOutcome
where
    F: std::future::Future<Output = SqlTestOutcome>,
{
    let deadline = std::time::Duration::from_millis(timeout_ms);
    match tokio::time::timeout(deadline, probe).await {
        // This app's own timeout bit, so it can say how long it waited.
        Err(_) => SqlTestOutcome::Timeout {
            after_ms: Some(timeout_ms),
        },
        Ok(outcome) => outcome,
    }
}

/// Test one profile: resolve its secret, open a handle, prove a database is
/// behind it, and ask its version — the whole of it under `timeout_ms`.
///
/// Takes the profile rather than an id so the decision is reachable from a
/// test; the command does the lookup. See the crate docs: a
/// `#[tauri::command]` body cannot be called by any test, so nothing that
/// decides anything may live in one.
pub(crate) async fn test_outcome(connection: &StoredConnection, timeout_ms: u64) -> SqlTestOutcome {
    let Some(engine) = connection.engine else {
        return SqlTestOutcome::EngineUnknown;
    };
    let Some(driver) = driver_for(engine) else {
        return SqlTestOutcome::EngineUnsupported { engine };
    };
    let dsn_value = match resolve_dsn(&connection.secret) {
        Ok(value) => value,
        Err(reason) => return SqlTestOutcome::SecretUnresolved { reason },
    };

    let spec = ConnectSpec {
        dsn: dsn_value,
        writes_allowed: connection.allow_writes,
    };
    within_timeout(timeout_ms, probe_connection(driver.as_ref(), &spec, engine)).await
}

/// Open a handle, confirm a database is behind it, and read its version.
///
/// The order carries the rule: **the probe decides the outcome and the version
/// decorates it.** A handle that opened is not yet a working connection —
/// sqlite3 opens any file — so [`probe_query`] runs first and its failure is
/// the answer. The version query is then best effort, exactly as before: a
/// version this build could not read is `Ok { server_version: None }`, never a
/// failed test.
async fn probe_connection(
    driver: &dyn SqlDriver,
    spec: &ConnectSpec,
    engine: SqlEngine,
) -> SqlTestOutcome {
    let mut opened = match driver.connect(spec).await {
        Ok(connection) => connection,
        Err(error) => return classify_connect_failure(&error.message),
    };

    if let Some(query) = probe_query(engine) {
        if let Err(error) = run_discarding_rows(opened.as_mut(), query).await {
            return classify_probe_failure(&error.message);
        }
    }

    let server_version = match version_query(engine) {
        None => None,
        Some(query) => run_discarding_rows(opened.as_mut(), query)
            .await
            .ok()
            .and_then(|outcome| server_version_from(outcome.result())),
    };
    SqlTestOutcome::Ok { server_version }
}

/// Run one statement on an open connection for its outcome alone.
///
/// The streamed events are drained and discarded — the row loop streams, and a
/// full channel would block it forever — so the caller reads the assembled
/// result off the returned outcome.
pub(crate) async fn run_discarding_rows(
    connection: &mut dyn cb_core::sql::driver::SqlConnection,
    sql: &str,
) -> Result<StatementOutcome, DriverError> {
    let (tx, mut rx) = mpsc::channel(64);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let (_stop_handle, stop) = cb_core::sql::driver::stop_channel();
    let outcome = connection
        .run_statement(sql, 0, Limits::default(), &tx, stop)
        .await;
    drop(tx);
    let _ = drain.await;
    outcome
}

/// Open a connection, prove it is a database, ask its version, and close it.
///
/// Opened with the profile's own `allow_writes`, so the test exercises the
/// handle the user will actually get rather than a stricter one.
#[tauri::command]
pub async fn sql_test_connection(id: String) -> Result<SqlTestOutcome, String> {
    let file = store::load(&store::sql_connections_path());
    let connection = file
        .connections
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("no connection named {id}"))?;
    Ok(test_outcome(connection, TEST_TIMEOUT_MS).await)
}

/// Test a literal connection before it is saved. The temporary profile exists
/// only for this call; no store is read or written, and the result cannot carry
/// the connection string back to the frontend.
#[tauri::command]
pub async fn sql_test_connection_string(
    engine: SqlEngine,
    connection_string: String,
) -> Result<SqlTestOutcome, String> {
    let connection = StoredConnection {
        id: String::new(),
        name: String::new(),
        engine: Some(engine),
        secret: SecretSource::Literal { connection_string },
        workspace_root: None,
        allow_writes: false,
        expose_to_agents: false,
        user_named: false,
        created_at_ms: 0,
        last_used_ms: None,
    };
    Ok(test_outcome(&connection, TEST_TIMEOUT_MS).await)
}

/// The catalog query for one table's columns, or the refusal that stopped it
/// being built.
///
/// A decision, so it is a free function rather than a line inside the command:
/// a schema or table name now arrives from an agent as well as from a row this
/// app itself read out of the catalog, and [`catalog::identifier`] refuses the
/// hazards the `''` doubling underneath it does not close. See that module's
/// docs — refusing is not parameter binding.
pub(crate) fn column_query_for(
    engine: SqlEngine,
    schema: Option<&str>,
    table: &str,
) -> Result<String, String> {
    if let Some(schema) = schema {
        catalog::identifier(schema).map_err(|refusal| refusal.sentence())?;
    }
    catalog::identifier(table).map_err(|refusal| refusal.sentence())?;
    Ok(catalog::column_catalog_query(engine, schema, table))
}

/// List objects for the explorer. Tables are the first supported category;
/// adding another category does not require replacing this command's shape.
#[tauri::command]
pub async fn sql_list_objects(connection_id: String) -> Result<Vec<SqlObjectView>, String> {
    let file = store::load(&store::sql_connections_path());
    let connection = file
        .connections
        .iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| format!("no connection named {connection_id}"))?;
    let engine = connection
        .engine
        .ok_or_else(|| "the connection's engine has not been determined".to_string())?;
    let driver = driver_for(engine)
        .ok_or_else(|| format!("this build has no driver for {}", engine_name(engine)))?;
    let dsn = resolve_dsn(&connection.secret)?;
    let mut live = driver
        .connect(&ConnectSpec {
            dsn,
            writes_allowed: false,
        })
        .await
        .map_err(|error| dsn::redact(&error.message))?;
    let outcome = run_discarding_rows(live.as_mut(), catalog::object_catalog_query(engine))
        .await
        .map_err(|error| dsn::redact(&error.message))?;
    catalog::table_objects(outcome.result())
}

/// Lazily load a table's columns when its explorer node is opened.
#[tauri::command]
pub async fn sql_list_columns(
    connection_id: String,
    schema: Option<String>,
    table: String,
) -> Result<Vec<SqlColumnView>, String> {
    let file = store::load(&store::sql_connections_path());
    let connection = file
        .connections
        .iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| format!("no connection named {connection_id}"))?;
    let engine = connection
        .engine
        .ok_or_else(|| "the connection's engine has not been determined".to_string())?;
    let driver = driver_for(engine)
        .ok_or_else(|| format!("this build has no driver for {}", engine_name(engine)))?;
    let dsn = resolve_dsn(&connection.secret)?;
    let mut live = driver
        .connect(&ConnectSpec {
            dsn,
            writes_allowed: false,
        })
        .await
        .map_err(|error| dsn::redact(&error.message))?;
    let query = column_query_for(engine, schema.as_deref(), &table)?;
    let outcome = run_discarding_rows(live.as_mut(), &query)
        .await
        .map_err(|error| dsn::redact(&error.message))?;
    catalog::table_columns(outcome.result())
}

/// Run a statement, streaming its rows to `channel`.
#[tauri::command]
pub async fn sql_execute(
    state: State<'_, AppState>,
    query_id: String,
    connection_id: String,
    sql: String,
    channel: Channel<SqlEvent>,
) -> Result<(), String> {
    let file = store::load(&store::sql_connections_path());
    let connection = file
        .connections
        .iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| format!("no connection named {connection_id}"))?;

    let plan = match execution_plan(connection, &sql, connection.allow_writes) {
        Ok(plan) => plan,
        Err(refusal) => {
            let _ = channel.send(refusal_event(&refusal));
            let _ = channel.send(SqlEvent::Finished { cancelled: false });
            return Ok(());
        }
    };
    // The guard's sentence about an allowed write, delivered rather than
    // dropped: it is the whole point of the wording that the user reads it.
    if let Some(notice) = notice_event(&plan) {
        let _ = channel.send(notice);
    }
    let dsn_value = match resolve_dsn(&connection.secret) {
        Ok(value) => value,
        Err(reason) => {
            let _ = channel.send(failed_event(None, &reason));
            let _ = channel.send(SqlEvent::Finished { cancelled: false });
            return Ok(());
        }
    };

    let stop =
        state
            .sql
            .register(&connection_id, &query_id)
            .map_err(|RegisterError::Duplicate| {
                format!("a query with id {query_id} is already running on this connection")
            })?;

    let (tx, mut rx) = mpsc::channel(512);
    let run = tokio::spawn(run_plan(plan, dsn_value, tx, stop));

    // Drained here rather than in a forwarding task, so every event the driver
    // produced has reached the frontend before `Finished` does. A closed
    // channel — the window went away — stops the sending but not the draining,
    // or the run task would block on a full channel forever.
    let mut open = true;
    while let Some(event) = rx.recv().await {
        if open && channel.send(event).is_err() {
            open = false;
        }
    }

    state.sql.finish(&connection_id, &query_id);

    let cancelled = match run.await {
        Ok(Ok(outcome)) => outcome.was_stopped(),
        Ok(Err(error)) => {
            let _ = channel.send(error.to_event(Some(0)));
            false
        }
        Err(join) => {
            let _ = channel.send(failed_event(Some(0), &join.to_string()));
            false
        }
    };
    let _ = channel.send(SqlEvent::Finished { cancelled });
    Ok(())
}

/// Connect and run the planned statement. Decides nothing: the plan already
/// said what may be sent and with what.
async fn run_plan(
    plan: Plan,
    dsn_value: String,
    events: mpsc::Sender<SqlEvent>,
    stop: cb_core::sql::driver::StopSignal,
) -> Result<StatementOutcome, DriverError> {
    let driver = driver_for(plan.engine)
        .ok_or_else(|| DriverError::connect("this build has no driver for that engine"))?;
    let spec = ConnectSpec {
        dsn: dsn_value,
        writes_allowed: plan.writes_allowed,
    };
    let mut connection = driver.connect(&spec).await?;
    connection
        .run_statement(&plan.sql, 0, Limits::default(), &events, stop)
        .await
}

/// Ask a running statement to stop reading.
///
/// **Not a server-side cancel**: it stops this side reading and drops the
/// connection, and the server may still be executing. The name matches
/// [`SqlEvent::Finished`]'s `cancelled`, which is the user-stopped flag.
#[tauri::command]
pub async fn sql_cancel(
    state: State<'_, AppState>,
    query_id: String,
) -> Result<SqlStopOutcome, String> {
    Ok(stop_query(&state.sql, &query_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_core::sql::model::{SqlColumn, SqlResultSet};

    const PASSWORD: &str = "hunter2-very-secret";

    fn literal(dsn: &str) -> StoredConnection {
        StoredConnection {
            id: "c1".into(),
            name: "Orders".into(),
            engine: Some(SqlEngine::Sqlite),
            secret: SecretSource::Literal {
                connection_string: dsn.into(),
            },
            workspace_root: None,
            allow_writes: false,
            expose_to_agents: false,
            user_named: false,
            created_at_ms: 10,
            last_used_ms: None,
        }
    }

    /// The exact string a unit-variant enum serialises to.
    fn json_str<T: Serialize>(value: &T) -> String {
        serde_json::to_value(value)
            .unwrap()
            .as_str()
            .expect("a unit variant serialises to a string")
            .to_string()
    }

    fn result_with(values: Vec<SqlValue>) -> SqlResultSet {
        SqlResultSet {
            columns: vec![SqlColumn {
                name: "v".into(),
                type_name: None,
            }],
            rows: if values.is_empty() {
                Vec::new()
            } else {
                vec![values]
            },
            row_cap: None,
            rows_affected: None,
            elapsed_ms: 1,
            statement_index: 0,
        }
    }

    // -----------------------------------------------------------------------
    // The catalog query the explorer builds
    // -----------------------------------------------------------------------

    #[test]
    fn an_ordinary_table_builds_its_column_query() {
        let query = column_query_for(SqlEngine::Postgres, Some("sales"), "orders").unwrap();
        assert!(query.contains("'sales'"), "{query}");
        assert!(query.contains("'orders'"), "{query}");
    }

    /// The first layer bites before the doubling underneath it. A name this app
    /// cannot look up safely is refused, not repaired — a repaired name would
    /// return rows about a different object.
    #[test]
    fn a_hazardous_identifier_is_refused_and_the_refusal_says_which_rule() {
        let cases: [(Option<&str>, String, &str); 4] = [
            (None, "orders\\".to_string(), "backslash"),
            (None, "or'ders".to_string(), "single quote"),
            (Some("sales"), "or\0ders".to_string(), "NUL"),
            (None, "or\nders".to_string(), "line break"),
        ];
        for (schema, table, needle) in cases {
            let error = column_query_for(SqlEngine::Sqlite, schema, &table)
                .expect_err("this name must be refused");
            assert!(
                error.to_lowercase().contains(&needle.to_lowercase()),
                "the refusal must name the rule that bit ({needle}): {error}"
            );
        }
    }

    #[test]
    fn an_empty_schema_is_refused_rather_than_matching_every_schema() {
        // `None` means *this engine does not qualify by schema*; `Some("")` is
        // a name nobody has, and interpolating it would silently match nothing
        // while looking like a working lookup.
        assert!(column_query_for(SqlEngine::Postgres, Some(""), "orders").is_err());
        assert!(column_query_for(SqlEngine::Sqlite, None, "orders").is_ok());
    }

    // -----------------------------------------------------------------------
    // Redaction
    // -----------------------------------------------------------------------

    #[test]
    fn a_stored_password_is_nowhere_in_the_view_that_crosses_ipc() {
        let stored = literal(&format!(
            "Server=db.example.com;Database=Orders;User Id=sa;Password={PASSWORD};"
        ));
        let json = serde_json::to_string(&redact_connection(&stored)).unwrap();
        assert!(
            !json.contains(PASSWORD),
            "the connection string reached the frontend: {json}"
        );
        // And the description that replaces it is still useful.
        assert!(json.contains("db.example.com"), "{json}");
    }

    #[test]
    fn a_reference_source_describes_where_it_lives_and_holds_no_secret() {
        let mut stored = literal("Data Source=app.db");
        stored.secret = SecretSource::DotEnv {
            path: PathBuf::from("/repo/.env"),
            key: "DATABASE_URL".into(),
        };
        let view = redact_connection(&stored);
        assert!(!view.holds_a_secret);
        assert_eq!(
            view.secret,
            SqlSecretView::DotEnv {
                path: PathBuf::from("/repo/.env"),
                key: "DATABASE_URL".into(),
            }
        );
    }

    #[test]
    fn a_literal_is_the_only_source_that_reports_holding_a_secret() {
        assert!(redact_connection(&literal("Data Source=app.db")).holds_a_secret);
    }

    // -----------------------------------------------------------------------
    // The store verbs
    // -----------------------------------------------------------------------

    #[test]
    fn a_save_cannot_turn_writes_on() {
        // The whole reason `sql_set_allow_writes` is a separate verb: a form
        // round-trip must not be able to grant consent.
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        assert!(!file.connections[0].allow_writes);

        set_allow_writes(&mut file, "c1", true);
        let mut renamed = literal("Data Source=app.db");
        renamed.name = "Renamed".into();
        renamed.allow_writes = false;
        upsert(&mut file, renamed);

        assert_eq!(file.connections[0].name, "Renamed");
        assert!(
            file.connections[0].allow_writes,
            "a save moved consent, which only sql_set_allow_writes may do"
        );
    }

    #[test]
    fn a_new_profile_starts_with_writes_off_whatever_it_asked_for() {
        let mut file = SqlConnectionsFile::default();
        let mut incoming = literal("Data Source=app.db");
        incoming.allow_writes = true;
        upsert(&mut file, incoming);
        assert!(!file.connections[0].allow_writes);
    }

    #[test]
    fn a_save_cannot_raise_agent_exposure() {
        // Exposure is the stronger of the two consents — it decides whether an
        // agent may read this database at all — so it moves through
        // `sql_set_expose_to_agents` and nowhere else. A form round-trip
        // claiming it must change nothing, in either direction.
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        assert!(!file.connections[0].expose_to_agents);

        let mut claiming = literal("Data Source=app.db");
        claiming.name = "Renamed".into();
        claiming.expose_to_agents = true;
        upsert(&mut file, claiming);

        assert_eq!(file.connections[0].name, "Renamed");
        assert!(
            !file.connections[0].expose_to_agents,
            "a save granted agent exposure, which only sql_set_expose_to_agents may do"
        );

        // And the same in reverse: a payload that happens to say `false` must
        // not revoke a grant the user made, or every save would silently
        // un-expose and the toggle would look broken.
        set_expose_to_agents(&mut file, "c1", true);
        let mut denying = literal("Data Source=app.db");
        denying.expose_to_agents = false;
        upsert(&mut file, denying);
        assert!(
            file.connections[0].expose_to_agents,
            "a save revoked agent exposure, which only sql_set_expose_to_agents may do"
        );
    }

    #[test]
    fn a_new_profile_starts_unexposed_even_if_the_payload_says_otherwise() {
        let mut file = SqlConnectionsFile::default();
        let mut incoming = literal("Data Source=app.db");
        incoming.expose_to_agents = true;
        upsert(&mut file, incoming);
        assert!(
            !file.connections[0].expose_to_agents,
            "a connection the user has just created has not been offered to an agent"
        );
    }

    #[test]
    fn a_rename_is_not_an_exposure_decision() {
        // `rename` takes the two fields a rename is about. Naming a connection
        // says nothing about who may read it, in either direction.
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        assert!(rename(&mut file, "c1", "Orders (prod)".into()));
        assert!(!file.connections[0].expose_to_agents);

        set_expose_to_agents(&mut file, "c1", true);
        assert!(rename(&mut file, "c1", "Orders (prod, read-only)".into()));
        assert!(
            file.connections[0].expose_to_agents,
            "a rename revoked agent exposure"
        );
    }

    #[test]
    fn exposing_an_unknown_id_reports_it_rather_than_inventing_an_entry() {
        let mut file = SqlConnectionsFile::default();
        assert!(!set_expose_to_agents(&mut file, "nope", true));
        assert!(file.connections.is_empty());
    }

    /// The two consents are separate facts, and the picker renders them
    /// separately. Deriving one from the other anywhere would make the MCP
    /// server's forced `writes_allowed: false` look like the only thing
    /// standing between an agent and a write.
    #[test]
    fn the_two_consents_do_not_move_each_other() {
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));

        set_allow_writes(&mut file, "c1", true);
        assert!(!file.connections[0].expose_to_agents);

        set_expose_to_agents(&mut file, "c1", true);
        set_allow_writes(&mut file, "c1", false);
        assert!(file.connections[0].expose_to_agents);

        set_expose_to_agents(&mut file, "c1", false);
        set_allow_writes(&mut file, "c1", true);
        assert!(!file.connections[0].expose_to_agents);
        assert!(file.connections[0].allow_writes);
    }

    #[test]
    fn an_update_keeps_the_record_fields_and_replaces_the_form_fields() {
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        file.connections[0].last_used_ms = Some(99);

        let mut incoming = literal("Data Source=other.db");
        incoming.created_at_ms = 999;
        incoming.last_used_ms = Some(1);
        upsert(&mut file, incoming);

        assert_eq!(file.connections.len(), 1);
        assert_eq!(file.connections[0].created_at_ms, 10);
        assert_eq!(file.connections[0].last_used_ms, Some(99));
        assert_eq!(
            file.connections[0].secret,
            SecretSource::Literal {
                connection_string: "Data Source=other.db".into()
            }
        );
    }

    #[test]
    fn removing_reports_whether_anything_was_there() {
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        assert!(remove(&mut file, "c1"));
        assert!(!remove(&mut file, "c1"));
    }

    #[test]
    fn setting_consent_on_an_unknown_id_reports_it_rather_than_inventing_an_entry() {
        let mut file = SqlConnectionsFile::default();
        assert!(!set_allow_writes(&mut file, "nope", true));
        assert!(file.connections.is_empty());
    }

    #[test]
    fn renaming_records_that_the_user_chose_the_name() {
        // Both halves matter: the name the picker shows, and the flag that stops
        // it composing `project · source · key` over the top of it.
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        assert!(!file.connections[0].user_named);

        assert!(rename(&mut file, "c1", "Shop (live)".into()));
        assert_eq!(file.connections[0].name, "Shop (live)");
        assert!(file.connections[0].user_named);
    }

    #[test]
    fn renaming_an_unknown_id_reports_it_rather_than_inventing_an_entry() {
        let mut file = SqlConnectionsFile::default();
        assert!(!rename(&mut file, "nope", "Shop".into()));
        assert!(file.connections.is_empty());
    }

    #[test]
    fn renaming_touches_nothing_but_the_name() {
        // In particular not the secret: a rename that went through `upsert`
        // would need a whole profile, and the only one a caller holding a
        // redacted view can build has the *display form* where the password was.
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        set_allow_writes(&mut file, "c1", true);

        rename(&mut file, "c1", "Shop".into());

        let entry = &file.connections[0];
        assert_eq!(
            entry.secret,
            SecretSource::Literal {
                connection_string: "Data Source=app.db".into()
            }
        );
        assert!(entry.allow_writes, "a rename is not a consent decision");
        assert_eq!(entry.created_at_ms, 10);
    }

    #[test]
    fn a_save_cannot_undo_a_rename() {
        // Re-adopting a discovered connection — a rescan, or a re-save from the
        // form — carries the *derived* name. Letting it win would revert the
        // rename, and the flag would then disagree with the name it guards.
        let mut file = SqlConnectionsFile::default();
        upsert(&mut file, literal("Data Source=app.db"));
        rename(&mut file, "c1", "Shop (live)".into());

        let mut rediscovered = literal("Data Source=app.db");
        rediscovered.name = "Api · Default · Orders".into();
        rediscovered.user_named = false;
        upsert(&mut file, rediscovered);

        assert_eq!(file.connections[0].name, "Shop (live)");
        assert!(file.connections[0].user_named);
    }

    #[test]
    fn a_save_cannot_claim_the_user_named_a_new_profile() {
        // The same rule as consent: only the rename verb sets this, so a payload
        // asserting it cannot freeze a derived label on a brand-new entry.
        let mut file = SqlConnectionsFile::default();
        let mut incoming = literal("Data Source=app.db");
        incoming.user_named = true;
        upsert(&mut file, incoming);
        assert!(!file.connections[0].user_named);
    }

    // -----------------------------------------------------------------------
    // The execution plan
    // -----------------------------------------------------------------------

    #[test]
    fn a_read_is_planned() {
        let plan = execution_plan(&literal("Data Source=app.db"), "select 1", false).unwrap();
        assert_eq!(plan.engine, SqlEngine::Sqlite);
        assert_eq!(plan.sql, "select 1");
        assert!(!plan.writes_allowed);
        assert!(plan.note.is_none(), "a plain read needs no sentence");
    }

    #[test]
    fn a_write_is_refused_when_consent_is_withheld_and_planned_when_it_is_given() {
        let connection = literal("Data Source=app.db");
        let refusal = execution_plan(&connection, "delete from orders", false).unwrap_err();
        let Refusal::Guard { reason } = refusal else {
            panic!("expected a guard refusal, got {refusal:?}");
        };
        assert!(
            reason.contains("heuristic"),
            "a refusal must name itself as a text heuristic: {reason}"
        );

        let plan = execution_plan(&connection, "delete from orders", true).unwrap();
        assert!(
            plan.note.is_some(),
            "an allowed write must still say what it is"
        );
    }

    #[test]
    fn an_unknown_engine_is_not_a_guard_refusal_and_never_reached_the_database() {
        let mut connection = literal("Data Source=app.db");
        connection.engine = None;
        assert_eq!(
            execution_plan(&connection, "select 1", false).unwrap_err(),
            Refusal::EngineUnknown
        );
    }

    /// Every engine the app can name is now one it can actually open.
    ///
    /// This test used to assert the opposite for SQL Server — that
    /// `execution_plan` refused with [`Refusal::EngineUnsupported`] because
    /// `for_engine` had no arm for it. That refusal path is not gone (it is
    /// still constructed and its event mapping is pinned by
    /// `only_the_guards_refusal_is_a_refused_event`); it is simply no longer
    /// reachable through any real engine, so asserting it here would mean
    /// inventing an engine that does not exist. The invariant worth holding at
    /// this layer is the stronger one: an engine the picker can offer is an
    /// engine that resolves to a driver.
    #[test]
    fn every_engine_the_app_offers_resolves_to_a_driver() {
        for (engine, dsn) in [
            (SqlEngine::Sqlite, "Data Source=app.db"),
            (SqlEngine::Postgres, "postgres://u@h/db"),
            (SqlEngine::SqlServer, "Server=x;Database=y"),
        ] {
            let mut connection = literal(dsn);
            connection.engine = Some(engine);
            let plan = execution_plan(&connection, "select 1", false)
                .unwrap_or_else(|e| panic!("{engine:?} should plan, got {e:?}"));
            assert_eq!(plan.engine, engine);
        }
    }

    #[test]
    fn only_the_guards_refusal_is_a_refused_event() {
        // `Refused` means the guard refused, so nothing was sent. An engine
        // that could not be resolved never reached the guard.
        let refused = refusal_event(&Refusal::Guard {
            reason: "no".into(),
        });
        assert!(matches!(
            refused,
            SqlEvent::Refused {
                statement_index: 0,
                ..
            }
        ));

        for refusal in [
            Refusal::EngineUnknown,
            Refusal::EngineUnsupported {
                engine: SqlEngine::Postgres,
            },
        ] {
            let event = refusal_event(&refusal);
            let SqlEvent::Failed {
                statement_index,
                message,
            } = event
            else {
                panic!("expected a pre-statement failure, got {event:?}");
            };
            assert_eq!(
                statement_index, None,
                "nothing ran, so there is no statement to point at"
            );
            assert!(!message.is_empty());
        }
    }

    /// Which engines this build can actually *open a handle for*.
    ///
    /// Deliberately pinned rather than derived, because the answer is what
    /// separates [`SqlTestOutcome::EngineUnsupported`] ("wait for a release")
    /// from [`SqlTestOutcome::EngineUnknown`] ("pick an engine"), and those
    /// must not collapse into one.
    ///
    /// This began life as an alarm for the opposite state: `mssql.rs` existed
    /// while `for_engine` still returned `None` for it, because two agents
    /// added their arms to the same `match` in parallel and one overwrote the
    /// other — so the driver was written, compiled, and unreachable. The alarm
    /// fired and the arm was wired. It now pins the finished state, and fails
    /// if any engine ever loses its driver the same silent way.
    #[test]
    fn every_engine_has_a_driver_in_this_build() {
        for (engine, since) in [
            (SqlEngine::Sqlite, "phase 1"),
            (SqlEngine::Postgres, "phase 3"),
            (SqlEngine::SqlServer, "phase 2"),
        ] {
            assert!(
                driver_for(engine).is_some(),
                "{engine:?} has shipped since {since} — if this fails, its arm \
                 in `sql::driver::for_engine` was dropped"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Secrets
    // -----------------------------------------------------------------------

    #[test]
    fn a_literal_resolves_to_itself() {
        assert_eq!(
            resolve_dsn(&SecretSource::Literal {
                connection_string: "Data Source=app.db".into()
            })
            .unwrap(),
            "Data Source=app.db"
        );
    }

    #[test]
    fn an_empty_literal_is_unresolved_before_any_driver_is_opened() {
        let error = resolve_dsn(&SecretSource::Literal {
            connection_string: "   ".into(),
        })
        .unwrap_err();
        assert!(error.contains("empty"), "{error}");
    }

    #[test]
    fn an_unexpanded_reference_is_an_error_that_does_not_quote_the_value() {
        let error = resolve_dsn(&SecretSource::Literal {
            connection_string: "Server=${DB_HOST};Password=${DB_PASSWORD}".into(),
        })
        .unwrap_err();
        assert!(!error.contains("DB_PASSWORD"), "{error}");
        assert!(!error.is_empty());
    }

    // -----------------------------------------------------------------------
    // Testing a connection
    // -----------------------------------------------------------------------

    #[test]
    fn an_unrecognised_driver_message_is_not_filed_as_unreachable() {
        // The abstention. Calling this "unreachable" would send somebody to
        // check a firewall over a message this build had no rule for.
        let outcome = classify_connect_failure("disk I/O error while reading page 4");
        assert!(
            matches!(outcome, SqlTestOutcome::Failed { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn the_recognised_connect_failures_stay_apart() {
        assert!(matches!(
            classify_connect_failure("Login failed for user 'sa'."),
            SqlTestOutcome::AuthFailed { .. }
        ));
        // A database file that will not open is not an unreachable host: see
        // `a_file_that_will_not_open_is_a_file_answer_and_not_a_network_one`.
        assert!(matches!(
            classify_connect_failure("connection refused"),
            SqlTestOutcome::Unreachable { .. }
        ));
        assert!(matches!(
            classify_connect_failure("the remote certificate is invalid"),
            SqlTestOutcome::TlsFailed { .. }
        ));
        assert_eq!(
            classify_connect_failure("the connection attempt timed out"),
            SqlTestOutcome::Timeout { after_ms: None }
        );
    }

    #[test]
    fn a_driver_reported_timeout_does_not_invent_a_duration() {
        // Only this app's own timeout knows how long it waited.
        assert_eq!(
            classify_connect_failure("timeout expired"),
            SqlTestOutcome::Timeout { after_ms: None }
        );
    }

    #[test]
    fn a_connect_message_carrying_a_dsn_is_redacted_before_it_becomes_an_outcome() {
        let outcome = classify_connect_failure(&format!(
            "Login failed while connecting with Server=db;Password={PASSWORD};"
        ));
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(!json.contains(PASSWORD), "{json}");
    }

    #[test]
    fn a_version_is_only_reported_when_the_server_gave_one() {
        assert_eq!(
            server_version_from(&result_with(vec![SqlValue::Text {
                text: "3.45.1".into(),
                truncated: false,
            }])),
            Some("3.45.1".to_string())
        );
        // A NULL, an undecodable cell and an empty result set are all "no
        // version reported" — never a placeholder that would read as one.
        assert_eq!(
            server_version_from(&result_with(vec![SqlValue::Null])),
            None
        );
        assert_eq!(
            server_version_from(&result_with(vec![SqlValue::Unavailable {
                reason: "no".into()
            }])),
            None
        );
        assert_eq!(server_version_from(&result_with(Vec::new())), None);
    }

    #[test]
    fn only_engines_with_a_known_version_query_are_asked() {
        assert!(version_query(SqlEngine::Sqlite).is_some());
        assert!(version_query(SqlEngine::SqlServer).is_none());
        assert!(version_query(SqlEngine::Postgres).is_none());
    }

    // -----------------------------------------------------------------------
    // Stopping
    // -----------------------------------------------------------------------

    #[test]
    fn stopping_a_running_query_by_its_id_alone_finds_its_connection() {
        let sessions = SqlSessions::new();
        let _signal = sessions.register("conn-a", "q1").unwrap();
        assert_eq!(stop_query(&sessions, "q1"), SqlStopOutcome::Signalled);
        // A second stop is not a second signal, and not a success.
        assert_eq!(stop_query(&sessions, "q1"), SqlStopOutcome::AlreadyStopping);
    }

    #[test]
    fn stopping_a_query_that_is_not_running_is_not_found_rather_than_an_error() {
        let sessions = SqlSessions::new();
        assert_eq!(stop_query(&sessions, "q1"), SqlStopOutcome::NotFound);
        let _signal = sessions.register("conn-a", "q1").unwrap();
        assert_eq!(stop_query(&sessions, "other"), SqlStopOutcome::NotFound);
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    #[test]
    fn a_failure_event_is_redacted_at_its_only_constructor() {
        let event = failed_event(Some(0), &format!("boom Password={PASSWORD};"));
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains(PASSWORD), "{json}");
    }

    // -----------------------------------------------------------------------
    // Wire keys
    // -----------------------------------------------------------------------

    #[test]
    fn the_view_serialises_with_the_keys_the_ui_reads() {
        let value =
            serde_json::to_value(redact_connection(&literal("Data Source=app.db"))).unwrap();
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "allowWrites",
                "createdAtMs",
                "engine",
                "exposeToAgents",
                "holdsASecret",
                "id",
                "lastUsedMs",
                "name",
                "secret",
                "userNamed",
                "workspaceRoot",
            ]
        );
        // Absent values cross as an explicit null, never as a missing key.
        assert!(value.get("lastUsedMs").unwrap().is_null());
    }

    /// One value of every [`SqlTestOutcome`] variant, paired with the `kind`
    /// string it serialises to.
    ///
    /// Shared by the serialisation test and the TypeScript-mirror test below,
    /// so a variant added to the enum cannot be added to one check and missed
    /// by the other.
    fn every_test_outcome() -> [(SqlTestOutcome, &'static str); 11] {
        [
            (
                SqlTestOutcome::Ok {
                    server_version: None,
                },
                "ok",
            ),
            (
                SqlTestOutcome::AuthFailed {
                    message: "x".into(),
                },
                "authFailed",
            ),
            (
                SqlTestOutcome::Unreachable {
                    message: "x".into(),
                },
                "unreachable",
            ),
            (
                SqlTestOutcome::CannotOpenFile {
                    message: "x".into(),
                },
                "cannotOpenFile",
            ),
            (
                SqlTestOutcome::NotADatabase {
                    message: "x".into(),
                },
                "notADatabase",
            ),
            (
                SqlTestOutcome::TlsFailed {
                    message: "x".into(),
                },
                "tlsFailed",
            ),
            (SqlTestOutcome::Timeout { after_ms: Some(1) }, "timeout"),
            (SqlTestOutcome::EngineUnknown, "engineUnknown"),
            (
                SqlTestOutcome::EngineUnsupported {
                    engine: SqlEngine::Postgres,
                },
                "engineUnsupported",
            ),
            (
                SqlTestOutcome::SecretUnresolved { reason: "x".into() },
                "secretUnresolved",
            ),
            (
                SqlTestOutcome::Failed {
                    message: "x".into(),
                },
                "failed",
            ),
        ]
    }

    #[test]
    fn every_test_outcome_serialises_to_its_exact_kind() {
        for (outcome, kind) in every_test_outcome() {
            let value = serde_json::to_value(&outcome).unwrap();
            assert_eq!(value.get("kind").unwrap().as_str(), Some(kind));
        }
        // The success carries its version key even when there is none.
        let ok = serde_json::to_value(SqlTestOutcome::Ok {
            server_version: None,
        })
        .unwrap();
        assert!(ok.get("serverVersion").unwrap().is_null());
    }

    /// The hand-written TypeScript mirror must describe every outcome this
    /// build can send.
    ///
    /// There is no codegen across this seam — `src/ipc/types.ts` is maintained
    /// by hand — so nothing but a test notices a variant added on one side
    /// alone. `cannotOpenFile` and `notADatabase` were exactly that: added here
    /// when "the handle opened" stopped being read as "this is a database", and
    /// left out of the union, so a frontend switching on it would have met an
    /// undeclared kind in precisely the case those two variants exist to
    /// report. Reading the mirror is cheap; drifting from it is silent.
    #[test]
    fn the_typescript_mirror_declares_every_test_outcome_kind() {
        let ts = include_str!("../../../src/ipc/types.ts");
        let start = ts
            .find("export type SqlTestOutcome")
            .expect("src/ipc/types.ts must declare SqlTestOutcome");
        // The union runs from its own declaration to the next one.
        let rest = &ts[start..];
        let union = match rest["export type".len()..].find("export type") {
            Some(offset) => &rest[.."export type".len() + offset],
            None => rest,
        };
        let missing: Vec<&str> = every_test_outcome()
            .iter()
            .map(|(_, kind)| *kind)
            .filter(|kind| !union.contains(&format!("kind: \"{kind}\"")))
            .collect();
        assert!(
            missing.is_empty(),
            "src/ipc/types.ts does not describe what this build sends: {missing:?}"
        );
    }

    #[test]
    fn the_secret_view_serialises_to_the_kinds_the_ui_switches_on() {
        let kinds: Vec<String> = [
            SqlSecretView::Literal {
                display: dsn::display_form("Data Source=app.db"),
            },
            SqlSecretView::AppSettings {
                path: PathBuf::from("a"),
                key: "k".into(),
            },
            SqlSecretView::UserSecrets {
                project: PathBuf::from("a"),
                key: "k".into(),
            },
            SqlSecretView::DotEnv {
                path: PathBuf::from("a"),
                key: "k".into(),
            },
        ]
        .iter()
        .map(|v| {
            serde_json::to_value(v).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(kinds, ["literal", "appSettings", "userSecrets", "dotEnv"]);
    }

    #[test]
    fn the_core_enumerations_the_frontend_mirrors_serialise_to_their_exact_strings() {
        // Pinned here because `src/ipc/types.ts` hand-mirrors these by hand and
        // nothing in `cb-core` pins them (`SecretSource` and the `model` types
        // are pinned in their own test modules). A rename on the Rust side must
        // fail a test rather than quietly produce a value no `switch` in the UI
        // matches.
        use cb_core::sql::discover::CandidateState;
        use cb_core::sql::dsn::{SqlAuthMode, SqlDisplayConfidence};

        assert_eq!(json_str(&SqlEngine::Postgres), "postgres");
        assert_eq!(json_str(&SqlEngine::SqlServer), "sqlServer");
        assert_eq!(json_str(&SqlEngine::Sqlite), "sqlite");

        assert_eq!(json_str(&SqlAuthMode::Integrated), "integrated");
        assert_eq!(json_str(&SqlAuthMode::Password), "password");
        assert_eq!(json_str(&SqlAuthMode::NoneStated), "noneStated");
        assert_eq!(json_str(&SqlAuthMode::Unknown), "unknown");

        assert_eq!(json_str(&SqlDisplayConfidence::Described), "described");
        assert_eq!(json_str(&SqlDisplayConfidence::Refused), "refused");

        let kinds: Vec<String> = [
            CandidateState::Ready,
            CandidateState::EngineUnknown { reason: "r".into() },
            CandidateState::Unresolved { reason: "r".into() },
        ]
        .iter()
        .map(|state| {
            serde_json::to_value(state).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(kinds, ["ready", "engineUnknown", "unresolved"]);
    }

    #[test]
    fn stop_outcomes_serialise_to_their_exact_strings() {
        assert_eq!(
            serde_json::to_string(&SqlStopOutcome::AlreadyStopping).unwrap(),
            "\"alreadyStopping\""
        );
        assert_eq!(
            serde_json::to_string(&SqlStopOutcome::NotFound).unwrap(),
            "\"notFound\""
        );
    }

    // -----------------------------------------------------------------------
    // Opened is not "is a database"
    // -----------------------------------------------------------------------

    /// A directory of this test's own, removed when the guard drops.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("cb-sql-cmd-{}-{tag}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create the test directory");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A profile pointing at a path, with a `Data Source=` keyword so the test
    /// exercises the same spelling the .NET side of a workspace uses.
    fn profile_for(path: &std::path::Path) -> StoredConnection {
        literal(&format!("Data Source={}", path.display()))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_that_is_not_a_database_is_not_reported_as_a_working_connection() {
        // The reproduction. `select sqlite_version()` is a pure SQL function
        // that reads no page, and sqlite3 defers its header check to the first
        // page read — so pointing a profile at a text file used to answer
        // `Ok { server_version: "3.46.0" }`. "I connected" and "this is a
        // database" are two facts.
        let dir = TempDir::new("not-a-db");
        let path = dir.join("README.md");
        std::fs::write(
            &path,
            "# Not a database\n\nordinary prose, several lines of it\n",
        )
        .unwrap();

        let outcome = test_outcome(&profile_for(&path), TEST_TIMEOUT_MS).await;
        let SqlTestOutcome::NotADatabase { message } = &outcome else {
            panic!("a text file was reported as a database: {outcome:?}");
        };
        assert!(!message.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_database_file_still_tests_ok_and_reports_its_version() {
        // The other half of the same rule: the probe must not turn a perfectly
        // good, empty database into a failure. A zero-length file is a valid
        // SQLite database, and `sqlite_master` answers 0 for it.
        let dir = TempDir::new("empty-db");
        let path = dir.join("empty.db");
        std::fs::File::create(&path).unwrap();

        let outcome = test_outcome(&profile_for(&path), TEST_TIMEOUT_MS).await;
        let SqlTestOutcome::Ok { server_version } = &outcome else {
            panic!("an empty database is a working connection: {outcome:?}");
        };
        let version = server_version
            .as_deref()
            .expect("sqlite reports its version");
        assert!(
            version.starts_with('3'),
            "expected a sqlite3 version, got {version}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_database_file_that_is_not_there_is_not_reported_as_an_unreachable_host() {
        // `unable to open database file` used to land in `Unreachable`, which
        // sends the reader to check a firewall over a wrong path.
        let dir = TempDir::new("missing-db");
        let outcome = test_outcome(&profile_for(&dir.join("nope.db")), TEST_TIMEOUT_MS).await;
        assert!(
            matches!(outcome, SqlTestOutcome::CannotOpenFile { .. }),
            "{outcome:?}"
        );
    }

    // -----------------------------------------------------------------------
    // The deadline covers the whole probe
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn the_deadline_covers_whatever_the_probe_does_not_only_the_connect() {
        // A future that never finishes stands for the server that accepts a
        // connection and then never answers. The timeout used to wrap only
        // `driver.connect(..)`, so this case was reported as anything but a
        // timeout.
        let outcome = within_timeout(5, std::future::pending::<SqlTestOutcome>()).await;
        assert_eq!(outcome, SqlTestOutcome::Timeout { after_ms: Some(5) });
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_probe_that_answers_inside_the_deadline_keeps_its_own_answer() {
        let outcome = within_timeout(
            TEST_TIMEOUT_MS,
            std::future::ready(SqlTestOutcome::NotADatabase {
                message: "file is not a database".into(),
            }),
        )
        .await;
        assert!(
            matches!(outcome, SqlTestOutcome::NotADatabase { .. }),
            "{outcome:?}"
        );
    }

    // Note on what is *not* tested here: `test_outcome` cannot be shown to
    // apply its deadline by passing a tiny one, because `tokio::time::timeout`
    // returns the inner future's answer whenever that future is ready on its
    // first poll, whatever the deadline — and a local SQLite file is. So the
    // deadline is proved generically above, and `test_outcome` is composed to
    // put the whole probe inside it.

    // -----------------------------------------------------------------------
    // Classification, from the strings sqlx really produces
    // -----------------------------------------------------------------------

    /// What sqlx reports when the file a SQLite DSN names cannot be opened —
    /// captured from a real run against a path that does not exist, not
    /// invented.
    const REAL_CANNOT_OPEN: &str = "error returned from database: (code: 14) unable to open \
                                    database file";
    /// What sqlx reports when a page is read from a file that is not a
    /// database. Captured from a real run against a `README.md`.
    const REAL_NOT_A_DATABASE: &str =
        "error returned from database: (code: 26) file is not a database";

    #[test]
    fn a_file_that_will_not_open_is_a_file_answer_and_not_a_network_one() {
        assert!(
            matches!(
                classify_connect_failure(REAL_CANNOT_OPEN),
                SqlTestOutcome::CannotOpenFile { .. }
            ),
            "the reader is sent to check a network over a wrong path"
        );
    }

    #[test]
    fn a_not_a_database_page_read_is_its_own_answer_and_not_a_wrong_password() {
        // `file is not a database` sat in the *connect* classifier's
        // `AuthFailed` list, where it was dead: sqlite3 never says it at the
        // connect stage. It is a probe-stage message, and it means the file is
        // not a database — not that the credentials were wrong.
        assert!(
            matches!(
                classify_probe_failure(REAL_NOT_A_DATABASE),
                SqlTestOutcome::NotADatabase { .. }
            ),
            "{:?}",
            classify_probe_failure(REAL_NOT_A_DATABASE)
        );
        assert!(
            !matches!(
                classify_connect_failure(REAL_NOT_A_DATABASE),
                SqlTestOutcome::AuthFailed { .. }
            ),
            "a dead needle came back to the connect classifier"
        );
    }

    #[test]
    fn an_unrecognised_probe_message_is_not_reported_as_a_working_connection() {
        // The probe exists to confirm something; a probe that errored for a
        // reason this build has no rule for has confirmed nothing. `disk I/O
        // error` is a real sqlite message and this build has no rule for it.
        let outcome = classify_probe_failure("error returned from database: disk I/O error");
        assert!(
            matches!(outcome, SqlTestOutcome::Failed { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_probe_message_carrying_a_dsn_is_redacted_before_it_becomes_an_outcome() {
        let outcome = classify_probe_failure(&format!(
            "file is not a database, opening Server=db;Password={PASSWORD};"
        ));
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(!json.contains(PASSWORD), "{json}");
    }

    #[test]
    fn a_socket_failure_that_names_a_file_is_still_a_network_answer() {
        // PostgreSQL's unix-socket failure reads `could not connect to server:
        // No such file or directory`. Both halves match a needle, and it is the
        // first that is true of it — which is why the network needles are
        // checked before the file ones.
        let outcome = classify_connect_failure(
            "could not connect to server: No such file or directory. Is the server running \
             locally and accepting connections on that socket?",
        );
        assert!(
            matches!(outcome, SqlTestOutcome::Unreachable { .. }),
            "{outcome:?}"
        );
    }

    // -----------------------------------------------------------------------
    // The guard's note reaches the user
    // -----------------------------------------------------------------------

    #[test]
    fn an_allowed_write_delivers_the_sentence_that_says_it_is_a_write() {
        let connection = literal("Data Source=app.db");
        let plan = execution_plan(&connection, "delete from orders", true).unwrap();
        let note = plan.note.clone().expect("an allowed write carries a note");

        let event = notice_event(&plan).expect("the note must be delivered, not dropped");
        let SqlEvent::Notice {
            statement_index,
            message,
        } = event
        else {
            panic!("a note is neither a failure nor a refusal: {event:?}");
        };
        assert_eq!(statement_index, 0);
        assert_eq!(message, note);
    }

    #[test]
    fn a_plain_read_carries_no_note_and_sends_no_notice() {
        let plan = execution_plan(&literal("Data Source=app.db"), "select 1", false).unwrap();
        assert!(notice_event(&plan).is_none());
    }

    #[test]
    fn a_note_is_redacted_on_its_way_out_like_every_other_message() {
        let plan = Plan {
            engine: SqlEngine::Sqlite,
            sql: "select 1".into(),
            writes_allowed: true,
            note: Some(format!("while using Server=db;Password={PASSWORD};")),
        };
        let json = serde_json::to_string(&notice_event(&plan).unwrap()).unwrap();
        assert!(!json.contains(PASSWORD), "{json}");
    }

    // -----------------------------------------------------------------------
    // One constructor for `Failed`
    // -----------------------------------------------------------------------

    #[test]
    fn every_failed_event_in_this_module_is_built_by_its_one_constructor() {
        // The module docs promise that no call site builds a `SqlEvent::Failed`
        // out of un-redacted text, and `Refusal::to_event` used to break that
        // promise by constructing one directly. A future bypass would be just
        // as invisible, so this reads the source and counts.
        // Split so these lines are not themselves matches.
        let needle = concat!("SqlEvent", "::Failed {");
        let only = concat!("    SqlEvent", "::Failed {");
        let source = include_str!("sql.rs");
        // `let` is how the two destructures in these tests are spelled, and a
        // destructure is not a construction.
        let built: Vec<&str> = source
            .lines()
            .filter(|line| line.contains(needle) && !line.contains("let "))
            .collect();
        assert_eq!(
            built,
            vec![only],
            "the only construction may be the one inside `failed_event`"
        );
    }

    // -----------------------------------------------------------------------
    // What may cross to an agent
    // -----------------------------------------------------------------------

    /// One of every `SqlTestOutcome`, with a sentinel in every message field.
    ///
    /// Written out rather than generated, so adding a variant to the enum means
    /// adding it here — and the count assertion below is what makes forgetting
    /// to a failure rather than a silent gap.
    fn every_outcome() -> Vec<SqlTestOutcome> {
        let message = || SENTINEL.to_string();
        vec![
            SqlTestOutcome::Ok {
                server_version: Some(message()),
            },
            SqlTestOutcome::AuthFailed { message: message() },
            SqlTestOutcome::Unreachable { message: message() },
            SqlTestOutcome::CannotOpenFile { message: message() },
            SqlTestOutcome::NotADatabase { message: message() },
            SqlTestOutcome::TlsFailed { message: message() },
            SqlTestOutcome::Timeout { after_ms: Some(10) },
            SqlTestOutcome::EngineUnknown,
            SqlTestOutcome::EngineUnsupported {
                engine: SqlEngine::Postgres,
            },
            SqlTestOutcome::SecretUnresolved { reason: message() },
            SqlTestOutcome::Failed { message: message() },
        ]
    }

    /// A string a driver message would carry that must never reach an agent.
    const SENTINEL: &str = "Server=db;Password=hunter2-SENTINEL";

    #[test]
    fn every_test_outcome_maps_to_an_agent_kind_and_none_carries_its_message() {
        let outcomes = every_outcome();
        assert_eq!(
            outcomes.len(),
            ConnectionStatusKind::ALL.len(),
            "a variant was added to SqlTestOutcome or ConnectionStatusKind and not to the other"
        );

        let mut mapped: Vec<&'static str> = Vec::new();
        for outcome in &outcomes {
            let kind = connection_status_kind(outcome);
            let rendered = cb_core::mcp::render::status(kind);
            assert!(
                !rendered.contains("SENTINEL"),
                "{outcome:?} leaked its message: {rendered}"
            );
            mapped.push(kind.name());
        }

        // Every kind is reachable and no two outcomes share one: a mapping that
        // collapsed two answers would be as wrong as one that leaked a message.
        let mut sorted = mapped.clone();
        sorted.sort_unstable();
        let mut unique = sorted.clone();
        unique.dedup();
        assert_eq!(sorted, unique, "two outcomes map to one agent kind");
        assert_eq!(sorted.len(), ConnectionStatusKind::ALL.len());
    }

    #[test]
    fn an_ok_outcomes_server_version_does_not_cross_either() {
        // `server_version` is not an error message, and it is still the
        // server's own text: it names a product and a build number.
        let kind = connection_status_kind(&SqlTestOutcome::Ok {
            server_version: Some("PostgreSQL 16.2 SENTINEL".into()),
        });
        assert_eq!(kind, ConnectionStatusKind::Ok);
        assert!(!cb_core::mcp::render::status(kind).contains("SENTINEL"));
    }
}
