//! Forcing read-only, in the one place a statement becomes a plan.
//!
//! [`agent_plan`] has **no `writes_allowed` parameter**. Not a defaulted one —
//! an absent one, so there is no value any caller could pass and no argument a
//! model could put in a tool call. [`crate::sql::plan::execution_plan`] still
//! takes the parameter, because the human console legitimately passes the
//! consent the user gave it; this path passes `false` and is the only thing
//! standing between the two.
//!
//! There are three layers, and each is here because the one above it can be
//! wrong:
//!
//! 1. `expose_to_agents` decides whether the connection is reachable at all
//!    ([`crate::mcp::expose`]).
//! 2. The read-only guard classifies the text, with `writes_allowed = false`.
//!    A statement it does not positively recognise as a read is refused, and
//!    `allow_writes` on the connection does not lift that here — it is not
//!    consulted.
//! 3. [`agent_connect_spec`] opens the handle with `writes_allowed: false`, so
//!    the *engine's own* mechanism fires: `SQLITE_OPEN_READONLY`, a read-only
//!    PostgreSQL session, a rolled-back SQL Server transaction. The guard is a
//!    text heuristic and says so; this layer is not.
//!
//! The third layer is the weakest for exactly one engine, and that is what
//! `sql.read_only_enforcement` exists to say out loud — see
//! [`crate::mcp::render`].

use super::answer::{restate_for_agent, McpRefusal, AGENT_NOTE};
use crate::sql::driver::{expected_enforcement, ConnectSpec, ReadOnlyEnforcement};
use crate::sql::dsn::SqlEngine;
use crate::sql::guard::{self, Verdict};
use crate::sql::plan::{self, Plan, Refusal};
use crate::sql::store::SqlConnection;

/// Decide whether an agent's statement may be sent.
///
/// The resulting [`Plan`] always carries `writes_allowed: false`; there is no
/// input to this function that could produce anything else.
pub fn agent_plan(connection: &SqlConnection, sql: &str) -> Result<Plan, McpRefusal> {
    match plan::execution_plan(connection, sql, false) {
        Ok(plan) => Ok(plan),
        Err(refusal) => Err(translate(connection, sql, refusal)),
    }
}

/// The connect spec for this path. `writes_allowed` is fixed, for the same
/// reason [`agent_plan`] takes no parameter.
pub fn agent_connect_spec(dsn: String) -> ConnectSpec {
    ConnectSpec {
        dsn,
        writes_allowed: false,
    }
}

/// What a connection opened by this path would be enforcing, answerable before
/// anything is opened. Always the writes-disallowed answer.
pub fn agent_enforcement(engine: Option<SqlEngine>) -> ReadOnlyEnforcement {
    expected_enforcement(engine, false)
}

/// Turn the console's refusal into an agent's.
///
/// The guard's own finding is kept — only the guard should be describing what
/// it found — and the console's *advice* is replaced, because it tells the
/// reader to turn writes on for the connection and on this path that would
/// change nothing. See [`crate::mcp::answer::AGENT_NOTE`].
fn translate(connection: &SqlConnection, sql: &str, refusal: Refusal) -> McpRefusal {
    match refusal {
        Refusal::EngineUnknown => McpRefusal::EngineUndetermined {
            connection: connection.id.clone(),
        },
        Refusal::EngineUnsupported { engine } => McpRefusal::EngineUnsupported { engine },
        Refusal::Guard { reason } => {
            // A recognised write and an unrecognised statement are different
            // answers on the console and stay different here. Re-asking the
            // guard rather than parsing its sentence: the words are prose and
            // the verdict is data.
            let write_kind = connection.engine.and_then(|engine| {
                match guard::classify(sql, plan::guard_engine(engine)) {
                    Verdict::Write { kind } => Some(kind),
                    _ => None,
                }
            });
            match write_kind {
                Some(kind) => McpRefusal::WriteRefusedUnconditionally {
                    kind: kind.to_string(),
                    // The console's finding is dropped here and only here,
                    // because this variant states the finding itself and the
                    // console's version of it ends "writes are off for this
                    // connection", which reads as a setting worth changing.
                    reason: AGENT_NOTE.to_string(),
                },
                None => McpRefusal::GuardRefused {
                    reason: restate_for_agent(&reason),
                },
            }
        }
    }
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;
