//! Which saved connections an agent may see, and what it is told about them.
//!
//! # A third view, narrower than the console's
//!
//! `SqlConnectionView` — the bridge's shape for the picker — carries a
//! `SqlSecretView` naming a **file path and a key**, and its literal variant
//! carries `server` and `database`. All of that is right for a human looking at
//! their own machine and wrong for a party that will paste what it is told into
//! a transcript, a commit message and a model provider's logs.
//!
//! [`AgentConnection`] is therefore a third view rather than a projection of
//! the second: an id, a name and an engine, and **no field that could carry a
//! secret**. That much is structural — there is no field of any type that could
//! hold one, and the only constructor drops everything else.
//!
//! The **path** guarantee is weaker and it is worth being precise about, because
//! an earlier version of this doc overstated it and the test that was supposed
//! to pin it passed vacuously (its fixture hard-coded `id: "conn-1"`, a literal
//! that could not be path-derived). A *discovered* connection's stored id
//! genuinely embeds the file it was found in, so:
//!
//! * the id is replaced by an opaque [`connection_handle`] — structural, since
//!   the real id never crosses;
//! * the name is **filtered** by `shown_name` down to its final path component,
//!   because a bare filename is what makes two connections tellable apart and
//!   is worth keeping.
//!
//! Pinned by `an_agent_connection_has_no_field_that_could_carry_a_secret_or_a_path`,
//! `a_discovered_connections_path_derived_id_does_not_reach_the_agent_verbatim`
//! and `a_shown_name_never_carries_a_path`.
//!
//! # Read per call, never cached
//!
//! [`exposed_now`] re-reads the store on **every** call, and nothing here holds
//! a `static`, a `OnceLock` or a memo. This is what makes revoking exposure
//! immediate: the server process is long-lived, so a cached list — or a cached
//! connection handle — would go on answering for a database the user had
//! already un-exposed, and the user would have no way to tell. Pinned by
//! `exposed_connections_are_read_per_call_and_never_cached`. Do not "optimise"
//! it.
//!
//! # `--workspace` scopes, it does not protect
//!
//! [`exposed_now`] takes an optional workspace and filters the **listing** to
//! connections created under it. That is a convenience, not a boundary: the
//! store is user-global, consent is per-connection, and [`find_exposed`]
//! deliberately searches the whole store so a connection excluded from a scoped
//! listing can still be named by id. See [`crate::mcp::argv`].

use std::path::Path;

use serde::Serialize;

use super::answer::McpRefusal;
use crate::sql::dsn::SqlEngine;
use crate::sql::store::{self, SqlConnection, SqlConnectionsFile};

/// Everything an agent is told about a connection.
///
/// Three fields, and adding a fourth is a decision about secrets rather than a
/// data-modelling convenience — the same rule
/// [`crate::sql::store::SecretSource`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnection {
    /// An opaque handle, which is what a tool call names.
    ///
    /// **Not the stored id.** A connection the app *discovered* rather than one
    /// the user typed carries its origin file in its id —
    /// `appsettings:<workspace-relative path>:<key>`, `dotenv:…`,
    /// `usersecrets:…` (see [`crate::sql::discover`]) — and the store is
    /// user-global, so an agent working in one repository could otherwise read
    /// the layout of every other repository the user has opened. `--workspace`
    /// scoping is documented as a convenience rather than a boundary, so it is
    /// not the thing standing in the way.
    ///
    /// [`connection_handle`] derives this, and [`find_exposed`] resolves it, so
    /// the handle round-trips and an agent can still name what it was shown.
    pub id: String,
    /// The label shown to the agent.
    ///
    /// For a user-named connection this is exactly what they typed. For a
    /// discovered one it is the app's own label, which names the *file* the
    /// connection string was found in (`Api · appsettings.json · …`) — a bare
    /// filename, deliberately kept because it is how a person or an agent tells
    /// two connections apart, and pinned by
    /// `a_shown_name_never_carries_a_path` to be a filename and never a path.
    pub name: String,
    /// [`None`] means the engine was never determined — not a default.
    pub engine: Option<SqlEngine>,
}

/// The opaque handle an agent sees in place of a stored id.
///
/// FNV-1a over the stored id: deterministic across runs (so an agent may cache
/// it), one-way for this purpose, and dependency-free. It is **not** a security
/// hash and does not need to be — it exists to avoid volunteering filesystem
/// layout, not to protect a secret. The real secret never comes near this type.
pub fn connection_handle(id: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("conn-{hash:016x}")
}

/// Strip a discovered label down to something carrying no path.
///
/// `discover` labels are already filename-based, but a label whose origin path
/// could not be made workspace-relative falls back to the absolute path, so this
/// keeps only the final component of any segment that looks like one.
fn shown_name(name: &str) -> String {
    name.split(" · ")
        .map(|segment| segment.rsplit(['/', '\\']).next().unwrap_or(segment).trim())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl AgentConnection {
    /// The narrow view of a saved profile. The only constructor, so no caller
    /// can assemble one with more in it.
    pub fn of(connection: &SqlConnection) -> Self {
        Self {
            id: connection_handle(&connection.id),
            name: shown_name(&connection.name),
            engine: connection.engine,
        }
    }
}

/// What a scoped listing found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedListing {
    /// The connections to show, already scoped.
    pub connections: Vec<AgentConnection>,
    /// How many exposed connections the scope left out. `0` when unscoped.
    /// Reported rather than silently dropped: a listing that is missing an
    /// entry with nothing to say so is the mistake this crate refuses
    /// everywhere else.
    pub scoped_out: usize,
}

/// Every exposed connection in a store file, scoped for the listing.
pub fn listing(file: &SqlConnectionsFile, workspace: Option<&Path>) -> ExposedListing {
    let exposed: Vec<&SqlConnection> = file
        .connections
        .iter()
        .filter(|c| c.expose_to_agents)
        .collect();
    let Some(root) = workspace else {
        return ExposedListing {
            connections: exposed.iter().map(|c| AgentConnection::of(c)).collect(),
            scoped_out: 0,
        };
    };
    let in_scope: Vec<&&SqlConnection> = exposed
        .iter()
        .filter(|c| c.workspace_root.as_deref() == Some(root))
        .collect();
    ExposedListing {
        scoped_out: exposed.len() - in_scope.len(),
        connections: in_scope.iter().map(|c| AgentConnection::of(c)).collect(),
    }
}

/// Read the store and list what an agent may see, **now**.
///
/// Re-reads `path` on every call. See the module docs.
pub fn exposed_now(path: &Path, workspace: Option<&Path>) -> ExposedListing {
    listing(&store::load(path), workspace)
}

/// Why a listing is empty. Two different answers, and the fix differs.
pub fn empty_listing_refusal(file: &SqlConnectionsFile) -> McpRefusal {
    if file.connections.is_empty() {
        McpRefusal::NoConnectionsSaved
    } else {
        McpRefusal::NoConnectionExposed {
            total_saved: file.connections.len(),
        }
    }
}

/// Resolve a selector to exactly one exposed connection, or refuse.
///
/// A selector is an id or a name. An id match is exact and wins outright, since
/// an id is what [`AgentConnection`] hands back and is the caller's way of being
/// unambiguous. A name match is case-insensitive and trimmed, and **several
/// matches are refused rather than resolved**: running a query against a
/// database the caller did not choose is as strong a claim as an arrow in the
/// architecture graph, and gets the same answer.
///
/// The search covers the **whole** store rather than the exposed subset,
/// because "there is no such connection" and "that one is not exposed" are
/// different answers and only one of them is fixed by a toggle.
pub fn find_exposed<'a>(
    file: &'a SqlConnectionsFile,
    selector: &str,
) -> Result<&'a SqlConnection, McpRefusal> {
    let needle = selector.trim();
    if needle.is_empty() {
        return Err(McpRefusal::ConnectionNotFound {
            selector: selector.to_string(),
        });
    }

    // The opaque handle first, because it is what `AgentConnection` hands back
    // and therefore what a well-behaved agent names. The stored id still
    // resolves too: a person reading the store, or a hand-written tool call,
    // should not be told their own id does not exist.
    if let Some(by_handle) = file
        .connections
        .iter()
        .find(|c| connection_handle(&c.id) == needle)
    {
        return admit(by_handle, selector);
    }

    if let Some(by_id) = file.connections.iter().find(|c| c.id == needle) {
        return admit(by_id, selector);
    }

    let by_name: Vec<&SqlConnection> = file
        .connections
        .iter()
        .filter(|c| c.name.trim().eq_ignore_ascii_case(needle))
        .collect();

    match by_name.as_slice() {
        [] => Err(McpRefusal::ConnectionNotFound {
            selector: selector.to_string(),
        }),
        [only] => admit(only, selector),
        several => {
            // Ambiguity is decided among the **exposed** ones: an unexposed
            // profile sharing a name is not a candidate, and naming it here
            // would report the existence of something the user did not share.
            let exposed: Vec<&&SqlConnection> = several
                .iter()
                .filter(|c| c.expose_to_agents)
                .collect::<Vec<_>>();
            match exposed.as_slice() {
                [] => Err(McpRefusal::ConnectionNotExposed {
                    selector: selector.to_string(),
                }),
                [only] => Ok(only),
                many => Err(McpRefusal::ConnectionAmbiguous {
                    selector: selector.to_string(),
                    candidates: many.iter().map(|c| c.id.clone()).collect(),
                }),
            }
        }
    }
}

fn admit<'a>(
    connection: &'a SqlConnection,
    selector: &str,
) -> Result<&'a SqlConnection, McpRefusal> {
    if connection.expose_to_agents {
        Ok(connection)
    } else {
        Err(McpRefusal::ConnectionNotExposed {
            selector: selector.to_string(),
        })
    }
}

#[cfg(test)]
#[path = "expose_tests.rs"]
mod tests;
