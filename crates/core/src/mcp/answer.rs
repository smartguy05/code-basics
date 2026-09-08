//! Every answer this server can give that is not data — and they are all
//! different answers.
//!
//! This is the crate's abstain-rather-than-guess rule applied to a consumer
//! that cannot ask a follow-up question. A human at the SQL console who reads
//! "could not run that" looks at the panel and works out why; a model reads the
//! sentence, believes it, and acts. So the distinctions that matter most are
//! the ones a human would have recovered from unaided:
//!
//! * *no connection is exposed* is not *no connection is saved*. The first is
//!   fixed by a toggle, the second by creating a connection.
//! * *this connection is not exposed* is not *there is no such connection*.
//!   Collapsing them either leaks the existence of connections the user chose
//!   not to share, or hides a toggle that is one click away — which of the two
//!   depends on which way it collapses, and neither is acceptable.
//! * an ambiguous selector is **refused, never resolved**. Running against a
//!   database the caller did not choose is as strong a claim as the
//!   architecture graph's arrows, and gets the same treatment.
//! * a capped result is not a short one, and a row cap is not a byte cap:
//!   raising the row limit would not return more rows.
//!
//! # Nothing here forwards driver text
//!
//! [`crate::sql::driver::DriverError`] is already redacted for the human
//! console, and that is not enough for this surface. `dsn::redact` is a keyword
//! heuristic, and the party on the other end of this transport will paste what
//! it is told into a transcript, a commit message and a model provider's logs.
//! So a connect failure crosses as a [`ConnectionStatusKind`] and a statement
//! failure as an [`crate::sql::driver::ErrorStage`] — a *variant* and a
//! *stage*, with the words dropped entirely rather than filtered.
//!
//! # Every refusal is a tool error, never a protocol error
//!
//! The specification says a client SHOULD hand tool execution errors to the
//! model so it can correct itself, and that is exactly what every variant here
//! is for. JSON-RPC protocol errors are reserved for the two things a model
//! cannot correct by trying again with better arguments: an unknown tool name
//! and arguments that are not the shape the schema declared.

use crate::sql::catalog::IdentifierRefusal;
use crate::sql::driver::{DriverError, ErrorStage};
use crate::sql::dsn::SqlEngine;
use crate::sql::model::{RowCap, RowCapReason};
use crate::sql::plan;

/// What a connection attempt found, **without its message**.
///
/// The message-free counterpart of the bridge's `SqlTestOutcome`. The bridge
/// owns the classification — it is the layer that has the driver's words — and
/// this is what may cross to an agent. The mapping between them is exhaustive
/// and tested there, so a new outcome fails to compile rather than leaking a
/// message through a catch-all arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatusKind {
    /// The handle opened and a page was really read.
    Ok,
    AuthFailed,
    /// A host that could not be reached. Network only.
    Unreachable,
    /// A database *file* that would not open.
    CannotOpenFile,
    /// The handle opened and what is behind it is not a database this build can
    /// read.
    NotADatabase,
    TlsFailed,
    Timeout,
    /// The profile's engine was never determined. Nothing was attempted.
    EngineUnknown,
    /// The engine is known and this build has no driver for it.
    EngineUnsupported,
    /// The connection string could not be re-read from where the profile says
    /// it lives. Nothing was attempted.
    SecretUnresolved,
    /// It failed and this build has no rule for the reason. The abstention:
    /// never filed under whichever category looked closest.
    Failed,
}

impl ConnectionStatusKind {
    /// Every kind, so a caller can enumerate them without matching. Kept in
    /// step with the enum by the bridge's exhaustive mapping test.
    pub const ALL: [ConnectionStatusKind; 11] = [
        ConnectionStatusKind::Ok,
        ConnectionStatusKind::AuthFailed,
        ConnectionStatusKind::Unreachable,
        ConnectionStatusKind::CannotOpenFile,
        ConnectionStatusKind::NotADatabase,
        ConnectionStatusKind::TlsFailed,
        ConnectionStatusKind::Timeout,
        ConnectionStatusKind::EngineUnknown,
        ConnectionStatusKind::EngineUnsupported,
        ConnectionStatusKind::SecretUnresolved,
        ConnectionStatusKind::Failed,
    ];

    /// The wire name. Stable: it is what an agent matches on.
    pub fn name(self) -> &'static str {
        match self {
            ConnectionStatusKind::Ok => "ok",
            ConnectionStatusKind::AuthFailed => "authFailed",
            ConnectionStatusKind::Unreachable => "unreachable",
            ConnectionStatusKind::CannotOpenFile => "cannotOpenFile",
            ConnectionStatusKind::NotADatabase => "notADatabase",
            ConnectionStatusKind::TlsFailed => "tlsFailed",
            ConnectionStatusKind::Timeout => "timeout",
            ConnectionStatusKind::EngineUnknown => "engineUnknown",
            ConnectionStatusKind::EngineUnsupported => "engineUnsupported",
            ConnectionStatusKind::SecretUnresolved => "secretUnresolved",
            ConnectionStatusKind::Failed => "failed",
        }
    }

    /// The sentence, written for a reader that cannot see the panel. It never
    /// quotes the driver, so it is the same sentence whatever the driver said.
    pub fn detail(self) -> &'static str {
        match self {
            ConnectionStatusKind::Ok => {
                "The connection opened and a page was read: this really is a database."
            }
            ConnectionStatusKind::AuthFailed => {
                "The server rejected the credentials. The person who owns this connection has to \
                 fix it; nothing an agent can do will change the outcome."
            }
            ConnectionStatusKind::Unreachable => {
                "The host could not be reached. This is a network answer, not a credentials one."
            }
            ConnectionStatusKind::CannotOpenFile => {
                "The database file could not be opened. It may not exist, it may be a directory, \
                 or it may not be readable; this build does not guess which."
            }
            ConnectionStatusKind::NotADatabase => {
                "A handle opened, but what is behind it is not a database this build can read — \
                 it may not be one at all, or it may be encrypted."
            }
            ConnectionStatusKind::TlsFailed => "The TLS handshake failed.",
            ConnectionStatusKind::Timeout => {
                "The connection did not open within its deadline. Nothing was sent and no \
                 statement ran."
            }
            ConnectionStatusKind::EngineUnknown => {
                "This connection's engine has never been determined, so nothing was attempted."
            }
            ConnectionStatusKind::EngineUnsupported => {
                "This build has no driver for that engine, so nothing was attempted."
            }
            ConnectionStatusKind::SecretUnresolved => {
                "The connection string could not be re-read from where the profile says it \
                 lives, so nothing was attempted."
            }
            ConnectionStatusKind::Failed => {
                "The attempt failed and this build has no rule for the reason. The driver's own \
                 words are deliberately not forwarded here; the person who owns this connection \
                 can see them in the SQL console."
            }
        }
    }
}

/// The note every guard refusal carries **on this path**.
///
/// [`crate::sql::guard::HEURISTIC_NOTE`] ends by advising the reader to *enable
/// writes for this connection*. On the human console that is true and useful.
/// Here it is **false**: [`crate::mcp::execute::agent_plan`] has no parameter a
/// caller could pass, so the writes-allowed setting is not consulted and
/// changing it changes nothing. Repeating the console's sentence would tell a
/// model to ask its user to flip a switch that cannot help, and the model would
/// then report that as the fix.
pub const AGENT_NOTE: &str = "This is a heuristic over the SQL text, not a database-enforced \
sandbox, and it can be wrong in both directions. Turning writes on for this connection would not \
change this answer: this interface forces read-only regardless of that setting, and there is no \
argument that lifts it.";

/// Restate a guard sentence for an agent.
///
/// The guard writes *what it found*, then [`crate::sql::guard::HEURISTIC_NOTE`].
/// This keeps the first half — the words are the guard's, and only the guard
/// should be describing what it found — and replaces the advice with
/// [`AGENT_NOTE`]. A sentence that does not end in the console's note is kept
/// whole and the agent note appended, so a future guard message cannot silently
/// lose it.
pub fn restate_for_agent(guard_sentence: &str) -> String {
    let finding = guard_sentence
        .trim_end()
        .strip_suffix(crate::sql::guard::HEURISTIC_NOTE)
        .unwrap_or(guard_sentence)
        .trim_end();
    if finding.is_empty() {
        return AGENT_NOTE.to_string();
    }
    format!("{finding} {AGENT_NOTE}")
}

/// Why a tool call produced no data.
///
/// Every variant is rendered as an MCP **tool execution error**, so the model
/// sees it and can correct itself. See the module docs for the ones that must
/// never collapse into each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRefusal {
    /// There are no saved connections at all.
    NoConnectionsSaved,
    /// There are saved connections, and none of them is exposed to agents.
    /// `total_saved` is what makes this distinguishable from
    /// [`McpRefusal::NoConnectionsSaved`] for the reader, and it is a **count**
    /// rather than a list: an unexposed connection's name is not this
    /// interface's to reveal.
    NoConnectionExposed { total_saved: usize },
    /// Nothing saved matches the selector, exposed or not.
    ConnectionNotFound { selector: String },
    /// A connection matches, and the user has not exposed it. Distinct from
    /// [`McpRefusal::ConnectionNotFound`] on purpose — the fix is one toggle,
    /// and hiding that behind "not found" would send the reader hunting for a
    /// connection that is right there.
    ConnectionNotExposed { selector: String },
    /// Several exposed connections match. **Refused, never resolved.**
    ConnectionAmbiguous {
        selector: String,
        /// The ids that matched, so the caller can name one exactly. Ids, not
        /// connection details.
        candidates: Vec<String>,
    },
    /// The connection's engine has never been determined, so there is no
    /// dialect to check the SQL against and no driver to open.
    EngineUndetermined { connection: String },
    /// The engine is known and this build has no driver for it. Distinct from
    /// [`McpRefusal::EngineUndetermined`]: one is answered by choosing an
    /// engine, the other by waiting for a release.
    EngineUnsupported { engine: SqlEngine },
    /// The connection string could not be re-read from where the profile says
    /// it lives. The store's own reason names a file and a key, so it is
    /// **replaced** here rather than forwarded.
    SecretUnresolved { connection: String },
    /// The read-only guard did not recognise the statement as a read. Not a
    /// write — the guard keeps those apart and so does this.
    GuardRefused { reason: String },
    /// The guard recognised a write. Its own variant because the answer is
    /// unconditional here: no setting on the connection and no argument to the
    /// tool would let it through.
    WriteRefusedUnconditionally { kind: String, reason: String },
    /// A schema or table name this app will not interpolate. Carries the
    /// catalog's own refusal so the rule that bit is named.
    IdentifierRefused { refusal: IdentifierRefusal },
    /// The connection did not open. Carries a *kind* and no message — see the
    /// module docs.
    ConnectFailed { kind: ConnectionStatusKind },
    /// The statement reached the server and failed. Carries a *stage* and no
    /// message.
    StatementFailed { stage: ErrorStage },
    /// A catalog listing hit a cap, so it is **not** a short list and must not
    /// be read as one. Refused rather than returned, because a partial catalog
    /// answers a different question from the one that was asked.
    CatalogTruncated { cap: RowCap },
    /// A catalog row was not the shape this build reads. Reported, never
    /// skipped: silently dropping it would return a table list that is missing
    /// a table with nothing to say so.
    CatalogRowUnreadable { detail: String },
}

impl McpRefusal {
    /// A short machine-matchable name, so a model can branch without parsing
    /// prose.
    pub fn code(&self) -> &'static str {
        match self {
            McpRefusal::NoConnectionsSaved => "noConnectionsSaved",
            McpRefusal::NoConnectionExposed { .. } => "noConnectionExposed",
            McpRefusal::ConnectionNotFound { .. } => "connectionNotFound",
            McpRefusal::ConnectionNotExposed { .. } => "connectionNotExposed",
            McpRefusal::ConnectionAmbiguous { .. } => "connectionAmbiguous",
            McpRefusal::EngineUndetermined { .. } => "engineUndetermined",
            McpRefusal::EngineUnsupported { .. } => "engineUnsupported",
            McpRefusal::SecretUnresolved { .. } => "secretUnresolved",
            McpRefusal::GuardRefused { .. } => "guardRefused",
            McpRefusal::WriteRefusedUnconditionally { .. } => "writeRefusedUnconditionally",
            McpRefusal::IdentifierRefused { .. } => "identifierRefused",
            McpRefusal::ConnectFailed { .. } => "connectFailed",
            McpRefusal::StatementFailed { .. } => "statementFailed",
            McpRefusal::CatalogTruncated { .. } => "catalogTruncated",
            McpRefusal::CatalogRowUnreadable { .. } => "catalogRowUnreadable",
        }
    }

    /// The sentence the model reads.
    pub fn sentence(&self) -> String {
        match self {
            McpRefusal::NoConnectionsSaved => "There are no saved database connections at all. \
                 One has to be created in the app's SQL console before anything here can answer."
                .to_string(),
            McpRefusal::NoConnectionExposed { total_saved } => format!(
                "None of the {total_saved} saved database connections is exposed to agents. \
                 Exposure is granted per connection in the app's SQL console and is off by \
                 default; nothing here can grant it."
            ),
            McpRefusal::ConnectionNotFound { selector } => format!(
                "No saved connection is called {selector:?}. Use sql.list_connections to see the \
                 ones that are available."
            ),
            McpRefusal::ConnectionNotExposed { selector } => format!(
                "A saved connection matches {selector:?}, and it is not exposed to agents. That \
                 is a per-connection choice made in the app's SQL console; nothing here can \
                 change it."
            ),
            McpRefusal::ConnectionAmbiguous {
                selector,
                candidates,
            } => format!(
                "{selector:?} matches {} exposed connections ({}), so it is refused rather than \
                 resolved — running against a database that was not chosen is not something this \
                 interface will guess at. Name one by its id.",
                candidates.len(),
                candidates.join(", ")
            ),
            McpRefusal::EngineUndetermined { connection } => format!(
                "The engine of connection {connection:?} has never been determined, so there is \
                 no dialect to check the SQL against and no driver to open. It has to be chosen \
                 in the app's SQL console."
            ),
            McpRefusal::EngineUnsupported { engine } => {
                plan::Refusal::EngineUnsupported { engine: *engine }.sentence()
            }
            McpRefusal::SecretUnresolved { connection } => format!(
                "The connection string for {connection:?} could not be read from where its \
                 profile says it lives, so nothing was attempted. The location is deliberately \
                 not named here."
            ),
            McpRefusal::GuardRefused { reason } => reason.clone(),
            McpRefusal::WriteRefusedUnconditionally { kind, reason } => {
                format!("This is a write ({kind}), and this interface is read-only. {reason}")
            }
            McpRefusal::IdentifierRefused { refusal } => refusal.sentence(),
            McpRefusal::ConnectFailed { kind } => format!(
                "The connection did not open ({}). {}",
                kind.name(),
                kind.detail()
            ),
            McpRefusal::StatementFailed { stage } => format!(
                "The statement failed at the {} stage. The database's own message is \
                 deliberately not forwarded to an agent; the person who owns this connection can \
                 read it in the app's SQL console.",
                stage_name(*stage)
            ),
            McpRefusal::CatalogTruncated { cap } => format!(
                "The catalog listing reached its {} of {} rows, so this is not a short list — it \
                 is an incomplete one, and returning it would answer a different question from \
                 the one that was asked.{}",
                cap_name(cap.reason),
                cap.limit,
                match cap.reason {
                    RowCapReason::RowLimit => "",
                    RowCapReason::ByteLimit =>
                        " Raising the row limit would not return more rows: it was the byte \
                         ceiling that bit.",
                }
            ),
            McpRefusal::CatalogRowUnreadable { detail } => format!(
                "A catalog row was not the shape this build reads, so the listing is reported as \
                 unreadable rather than silently missing an entry: {detail}"
            ),
        }
    }
}

/// A connect failure, with the driver's words dropped rather than filtered.
pub fn connect_failed(kind: ConnectionStatusKind) -> McpRefusal {
    McpRefusal::ConnectFailed { kind }
}

/// A statement failure, keeping only where it happened.
///
/// Takes the whole [`DriverError`] rather than a stage, so a call site cannot
/// become the place somebody decides to pass the message along too — there is
/// nowhere here for it to go.
pub fn statement_failed(error: &DriverError) -> McpRefusal {
    McpRefusal::StatementFailed { stage: error.stage }
}

fn stage_name(stage: ErrorStage) -> &'static str {
    match stage {
        ErrorStage::Connect => "connect",
        ErrorStage::ConnectTimeout => "connect (timed out)",
        ErrorStage::Execute => "execute",
    }
}

fn cap_name(reason: RowCapReason) -> &'static str {
    match reason {
        RowCapReason::RowLimit => "row ceiling",
        RowCapReason::ByteLimit => "byte ceiling",
    }
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
