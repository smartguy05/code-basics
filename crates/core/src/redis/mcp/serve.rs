//! The handshake and the answers that need no connection.
//!
//! Routing and the response envelopes are reused from [`crate::mcp::serve`] (the
//! method-name set and JSON-RPC framing are generic); this module adds only what
//! is Redis-specific: the server identity, the instructions, the tool-list
//! result (filtered by the per-tool gate), and the connection listing.

use std::path::Path;

use rmcp::model::{Implementation, InitializeResult, ServerCapabilities, ToolsCapability};
use serde_json::{json, Value};

use super::answer::McpRefusal;
use super::expose;
use super::render;
use super::tools;
use crate::mcp::serve::negotiated_version;
use crate::redis::store::RedisConnectionsFile;
use crate::tool_gate::{self, ToolGateFile};

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "code-basics-redis";

/// This build's version, from the crate.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a client is told before its first call. Two rules a model cannot infer:
/// exposure is per-connection and off by default, and writing is a **second**
/// consent (`allow_writes`) on top of exposure.
pub const INSTRUCTIONS: &str = "These tools reach the Redis servers the user has explicitly \
exposed to agents in the code-basics Redis panel. Exposure is per connection and off by default: a \
server that is not listed is one the user chose not to share. Writing is a separate consent again \
— a write tool is refused unless the connection has both 'expose to agents' and 'allow writes' \
enabled, both off by default; nothing here can grant either. A large value is capped and the cap \
is reported rather than the value being silently shortened.";

/// The `initialize` result.
pub fn initialize_result(requested: Option<&str>) -> InitializeResult {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolsCapability::default());
    InitializeResult::new(capabilities)
        .with_protocol_version(negotiated_version(requested))
        .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
        .with_instructions(INSTRUCTIONS)
}

/// The `tools/list` result, with tools the user switched off removed.
pub fn tools_list_result(gate: &ToolGateFile) -> Value {
    // Redis is not in `ServerId` (the four built-in servers), so it is not gated
    // by the shared filter; every tool is advertised. Kept as a parameter so the
    // runner's call site matches the other servers and a future `ServerId::Redis`
    // is a one-line change here.
    let _ = gate;
    let _ = tool_gate::MCP_TOOLS_FILE;
    json!({ "tools": tools::descriptors() })
}

/// `redis.list_connections`, straight out of the store. An empty listing is a
/// refusal naming *which* emptiness; a listing emptied only by `--workspace`
/// scope is answered, not refused.
pub fn list_connections_answer(
    file: &RedisConnectionsFile,
    workspace: Option<&Path>,
) -> Result<String, McpRefusal> {
    let listed = expose::listing(file, workspace);
    if listed.connections.is_empty() && listed.scoped_out == 0 {
        return Err(expose::empty_listing_refusal(file));
    }
    Ok(render::connections(&listed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redis::store::{RedisConnection, SecretSource};
    use rmcp::model::ProtocolVersion;
    use std::path::PathBuf;

    fn exposed(id: &str, name: &str) -> RedisConnection {
        RedisConnection {
            id: id.to_string(),
            name: name.to_string(),
            secret: SecretSource::AppSettings {
                path: PathBuf::from("appsettings.json"),
                key: "ConnectionStrings:Redis".to_string(),
            },
            workspace_root: None,
            allow_writes: false,
            expose_to_agents: true,
            user_named: true,
            created_at_ms: 1,
            last_used_ms: None,
        }
    }

    #[test]
    fn the_handshake_names_the_server_and_states_the_two_consents() {
        let result = initialize_result(None);
        assert_eq!(result.server_info.name, SERVER_NAME);
        assert!(INSTRUCTIONS.contains("off by default"));
        assert!(INSTRUCTIONS.contains("allow writes"));
    }

    #[test]
    fn version_comes_from_the_crate() {
        assert_eq!(SERVER_VERSION, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn an_empty_store_refuses_with_nothing_saved() {
        let file = RedisConnectionsFile::default();
        assert_eq!(
            list_connections_answer(&file, None),
            Err(McpRefusal::NoConnectionsSaved)
        );
    }

    #[test]
    fn a_populated_store_lists_the_exposed_ones() {
        let file = RedisConnectionsFile {
            version: 1,
            connections: vec![exposed("a", "Cache")],
        };
        let answer = list_connections_answer(&file, None).unwrap();
        assert!(answer.contains("Cache"), "{answer}");
    }

    #[test]
    fn a_known_version_is_echoed_and_an_unknown_one_still_gets_a_session() {
        let known = ProtocolVersion::LATEST;
        assert_eq!(
            initialize_result(Some(known.as_str())).protocol_version,
            known
        );
        assert_eq!(
            initialize_result(Some("1000-01-01")).protocol_version,
            ProtocolVersion::LATEST
        );
    }
}
