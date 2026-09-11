//! Which saved Redis connections an agent may see, and what it is told.
//!
//! The Redis counterpart of [`crate::mcp::expose`], with the same rules: an
//! [`AgentConnection`] is a narrow view with **no field that could carry a
//! secret or a path**, the stored id is replaced by the opaque
//! [`crate::mcp::expose::connection_handle`] (reused, not re-derived), the store
//! is re-read every call so revoking exposure is immediate, and `--workspace`
//! scopes the listing without being a boundary. There is no engine field.

use std::path::Path;

use serde::Serialize;

use super::answer::McpRefusal;
use crate::mcp::expose::connection_handle;
use crate::redis::store::{self, RedisConnection, RedisConnectionsFile};

/// Everything an agent is told about a Redis connection: a handle and a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnection {
    /// The opaque handle a tool call names — not the stored id.
    pub id: String,
    /// The label, filtered to carry no path.
    pub name: String,
}

fn shown_name(name: &str) -> String {
    name.split(" · ")
        .map(|segment| segment.rsplit(['/', '\\']).next().unwrap_or(segment).trim())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl AgentConnection {
    pub fn of(connection: &RedisConnection) -> Self {
        Self {
            id: connection_handle(&connection.id),
            name: shown_name(&connection.name),
        }
    }
}

/// What a scoped listing found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedListing {
    pub connections: Vec<AgentConnection>,
    /// Exposed connections the scope left out (`0` when unscoped). Reported, not
    /// silently dropped.
    pub scoped_out: usize,
}

/// Every exposed connection in a store file, scoped for the listing.
pub fn listing(file: &RedisConnectionsFile, workspace: Option<&Path>) -> ExposedListing {
    let exposed: Vec<&RedisConnection> = file
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
    let in_scope: Vec<&&RedisConnection> = exposed
        .iter()
        .filter(|c| c.workspace_root.as_deref() == Some(root))
        .collect();
    ExposedListing {
        scoped_out: exposed.len() - in_scope.len(),
        connections: in_scope.iter().map(|c| AgentConnection::of(c)).collect(),
    }
}

/// Read the store and list what an agent may see now. Re-reads every call.
pub fn exposed_now(path: &Path, workspace: Option<&Path>) -> ExposedListing {
    listing(&store::load(path), workspace)
}

/// Why a listing is empty: nothing saved, or nothing exposed.
pub fn empty_listing_refusal(file: &RedisConnectionsFile) -> McpRefusal {
    if file.connections.is_empty() {
        McpRefusal::NoConnectionsSaved
    } else {
        McpRefusal::NoConnectionExposed {
            total_saved: file.connections.len(),
        }
    }
}

/// Resolve a selector (handle, id, or name) to one exposed connection, or refuse.
/// Ambiguous exposed names are refused, never resolved. Searches the whole store
/// so *not found* and *not exposed* stay different answers.
pub fn find_exposed<'a>(
    file: &'a RedisConnectionsFile,
    selector: &str,
) -> Result<&'a RedisConnection, McpRefusal> {
    let needle = selector.trim();
    if needle.is_empty() {
        return Err(McpRefusal::ConnectionNotFound {
            selector: selector.to_string(),
        });
    }

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

    let by_name: Vec<&RedisConnection> = file
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
            let exposed: Vec<&&RedisConnection> =
                several.iter().filter(|c| c.expose_to_agents).collect();
            match exposed.as_slice() {
                [] => Err(McpRefusal::ConnectionNotExposed {
                    selector: selector.to_string(),
                }),
                [only] => Ok(only),
                many => Err(McpRefusal::ConnectionAmbiguous {
                    selector: selector.to_string(),
                    // Handles, not raw ids — a discovered id embeds a path.
                    candidates: many.iter().map(|c| connection_handle(&c.id)).collect(),
                }),
            }
        }
    }
}

fn admit<'a>(
    connection: &'a RedisConnection,
    selector: &str,
) -> Result<&'a RedisConnection, McpRefusal> {
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
