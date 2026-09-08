//! Is this process an MCP server, or the application?
//!
//! The third self-dispatch mode, beside [`crate::intents::hook`]'s
//! `record-intent` and [`crate::qgate`]'s `quality-gate`: the installed MCP
//! entry runs **this executable** rather than a shipped script, so there is no
//! second artifact to keep in step and no interpreter to depend on being
//! present.
//!
//! Acceptance is the same either-or as the other two, for the same stated
//! reason: the command line lives in a config file the user shares with their
//! team (`.mcp.json`, `~/.claude.json`, `~/.codex/config.toml`), so a
//! hand-edited line that kept only the subcommand, or only the marker, must
//! still start the server. The marker is read back through the same constant
//! that writes it, so the two cannot drift.
//!
//! # `--workspace` is not a security boundary
//!
//! It scopes the connection **listing**, and nothing else. The connection store
//! is user-global by design (see [`crate::sql::store`]), and consent is
//! per-connection: `expose_to_agents` is what decides whether an agent may see
//! a connection at all. A connection excluded from a scoped listing can still
//! be named by id, and that is deliberate — a flag in a config file the user
//! shares with their team is not a place to put an access decision. Say so in
//! any documentation of this flag.

/// The subcommand the installed MCP entry runs.
pub const SUBCOMMAND: &str = "mcp-sql";

/// The marker flag the installed entry also carries. Namespaced like the
/// others so no two modes can be confused for one another.
pub const MARKER: &str = "code-basics-mcp-sql";

/// Did the command line ask for the SQL MCP server rather than the application?
pub fn is_mcp_sql_invocation(args: &[String]) -> bool {
    let marker = format!("--{MARKER}");
    args.iter().any(|arg| arg == SUBCOMMAND || *arg == marker)
}

/// What an `mcp-sql` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSqlInvocation {
    /// The workspace the entry was installed for, if it named one. [`None`] is
    /// an unscoped install and lists every exposed connection. **Not** a
    /// security boundary — see the module docs.
    pub workspace: Option<String>,
}

/// Read an `mcp-sql` command line. [`None`] means this is not an MCP
/// invocation and there is nothing to do — an ordinary application launch.
pub fn parse_mcp_sql_args(args: &[String]) -> Option<McpSqlInvocation> {
    if !is_mcp_sql_invocation(args) {
        return None;
    }
    Some(McpSqlInvocation {
        workspace: flag(args, "--workspace").filter(|w| !w.is_empty()),
    })
}

/// Read `--name value` from the command line; first occurrence wins.
fn flag(args: &[String], name: &str) -> Option<String> {
    let position = args.iter().position(|arg| arg == name)?;
    args.get(position + 1).cloned()
}

#[cfg(test)]
#[path = "argv_tests.rs"]
mod tests;
