//! Whether a statement may be sent, and with what.
//!
//! This module holds decisions that used to live in the SQL console's Tauri
//! bridge. They moved for two reasons, and the second is the load-bearing one:
//!
//! 1. CLAUDE.md's rule that a `#[tauri::command]` body must not decide
//!    anything, because nothing in `src-tauri` can be called by a test.
//! 2. The MCP server is a **separate process with no `AppState`**, so it cannot
//!    call any command. Anything both the human console and an agent must agree
//!    about has to live here, where both can reach it.
//!
//! # One refusal, one sentence
//!
//! [`Refusal::sentence`] is the single phrasing of every refusal. The bridge
//! wraps it in a [`crate::sql::model::SqlEvent`] through its own redacting
//! constructor — that wrapping stays there — but the *words* are here, so the
//! console and an agent can never describe one refusal two different ways.
//!
//! A refusal also never names a supported-engine list it does not derive:
//! [`Refusal::sentence`] asks [`driver::for_engine`] which engines this build
//! can actually open, so a driver that ships cannot be contradicted by a
//! sentence somebody forgot to update.

use super::driver;
use super::dsn::SqlEngine;
use super::guard;
use super::store::SqlConnection;

/// A statement that may be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub engine: SqlEngine,
    pub sql: String,
    /// What the connection's consent says, passed to the driver so it can open
    /// a handle that itself refuses writes where the engine has one.
    pub writes_allowed: bool,
    /// The guard's own sentence for anything that is not a plain read —
    /// including an *allowed* write, which still says what it is. [`None`] for
    /// a read.
    pub note: Option<String>,
}

/// Why a statement will not be sent. Never "blocked": the three reasons are
/// acted on differently, and only the last one reached the guard at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The profile's engine was never determined, so there is no dialect to
    /// parse with and no driver to open. The user picks an engine.
    EngineUnknown,
    /// The engine is known and this build has no driver for it.
    EngineUnsupported { engine: SqlEngine },
    /// The read-only guard refused. `reason` is the guard's own sentence, which
    /// names itself as a text heuristic.
    Guard { reason: String },
}

impl Refusal {
    /// The sentence this refusal is described by, wherever it is described.
    ///
    /// The bridge turns it into an event and an agent turns it into a tool
    /// error; neither rephrases it.
    pub fn sentence(&self) -> String {
        match self {
            Refusal::EngineUnknown => "This connection's engine has not been determined, so \
                 there is no dialect to check the SQL against and no driver to open. Choose an \
                 engine for it and try again."
                .to_string(),
            Refusal::EngineUnsupported { engine } => {
                // Derived, never typed. The sentence this replaced named SQLite
                // as the only supported engine; Postgres and SQL Server drivers
                // shipped afterwards and it went on saying so.
                let supported: Vec<&str> = driver::ENGINES
                    .into_iter()
                    .filter(|engine| driver::for_engine(*engine).is_some())
                    .map(engine_name)
                    .collect();
                if supported.is_empty() {
                    format!(
                        "This build has no driver for {}, and none for any other engine either.",
                        engine_name(*engine)
                    )
                } else {
                    format!(
                        "This build has no driver for {}. It can open: {}.",
                        engine_name(*engine),
                        supported.join(", ")
                    )
                }
            }
            Refusal::Guard { reason } => reason.clone(),
        }
    }
}

/// The engine's name for a sentence a human reads.
pub fn engine_name(engine: SqlEngine) -> &'static str {
    match engine {
        SqlEngine::Sqlite => "SQLite",
        SqlEngine::SqlServer => "SQL Server",
        SqlEngine::Postgres => "PostgreSQL",
    }
}

/// The dialect the guard parses with. A total mapping: a new engine must be
/// given a dialect here, not defaulted into somebody else's.
pub fn guard_engine(engine: SqlEngine) -> guard::Engine {
    match engine {
        SqlEngine::Sqlite => guard::Engine::Sqlite,
        SqlEngine::SqlServer => guard::Engine::SqlServer,
        SqlEngine::Postgres => guard::Engine::Postgres,
    }
}

/// Decide whether a statement may be sent, and with what.
///
/// `allow_writes` is a parameter rather than being read off `connection` so the
/// resolution can be tested against both settings without building two
/// profiles — the caller passes the consent it is entitled to pass. The agent
/// path passes `false` and has no way to pass anything else.
pub fn execution_plan(
    connection: &SqlConnection,
    sql: &str,
    allow_writes: bool,
) -> Result<Plan, Refusal> {
    let Some(engine) = connection.engine else {
        return Err(Refusal::EngineUnknown);
    };
    if driver::for_engine(engine).is_none() {
        return Err(Refusal::EngineUnsupported { engine });
    }

    let decision = guard::guard(sql, guard_engine(engine), allow_writes);
    if !decision.allowed {
        return Err(Refusal::Guard {
            reason: refusal_reason(&decision),
        });
    }
    Ok(Plan {
        engine,
        sql: sql.to_string(),
        writes_allowed: allow_writes,
        note: decision.message,
    })
}

/// The sentence a refusal carries.
///
/// The guard always supplies one; the fallback exists so that a future verdict
/// with no message cannot produce an empty refusal, which would read as
/// "blocked" with no reason — the one thing the guard's own docs forbid. It
/// still carries [`guard::HEURISTIC_NOTE`], so a refusal never claims the
/// database would have stopped the statement.
pub fn refusal_reason(decision: &guard::Decision) -> String {
    decision.message.clone().unwrap_or_else(|| {
        format!(
            "This statement was not recognised as a read. {}",
            guard::HEURISTIC_NOTE
        )
    })
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
