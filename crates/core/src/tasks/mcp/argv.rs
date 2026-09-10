//! Is this process the Tasks MCP server, or the application?
//!
//! The fifth self-dispatch mode, beside [`crate::intents::hook`]'s
//! `record-intent`, [`crate::qgate`]'s `quality-gate`, [`crate::mcp::argv`]'s
//! `mcp-sql` and the browser server's `mcp-browser`: the installed MCP entry
//! runs **this executable** rather than a shipped script, so there is no second
//! artifact to keep in step and no interpreter to depend on being present.
//!
//! Acceptance is the same either-or as the others, for the same stated reason:
//! the command line lives in a config file the user shares with their team
//! (`.mcp.json`, `~/.claude.json`, `~/.codex/config.toml`), so a hand-edited
//! line that kept only the subcommand, or only the marker, must still start the
//! server. The marker is read back through the same constant that writes it, so
//! the two cannot drift.
//!
//! # `--workspace` *is* the boundary here
//!
//! Unlike the SQL server — where `--workspace` scopes a listing and nothing
//! else — the task store is per-workspace, so this flag names the one
//! `tasks.json` the server reads and writes. An unscoped invocation has no
//! store: every tool answers [`super::answer::McpRefusal::NoWorkspace`] rather
//! than picking a directory. The install step bakes the workspace in, and that
//! is the consent boundary: an agent configured for one repository cannot reach
//! another repository's tasks.

/// The subcommand the installed MCP entry runs.
pub const SUBCOMMAND: &str = "mcp-tasks";

/// The marker flag the installed entry also carries. Namespaced like the
/// others so no two modes can be confused for one another.
pub const MARKER: &str = "code-basics-mcp-tasks";

/// Did the command line ask for the Tasks MCP server rather than the
/// application?
pub fn is_mcp_tasks_invocation(args: &[String]) -> bool {
    let marker = format!("--{MARKER}");
    args.iter().any(|arg| arg == SUBCOMMAND || *arg == marker)
}

/// What an `mcp-tasks` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTasksInvocation {
    /// The workspace the entry was installed for, if it named one. [`None`] is
    /// an unscoped install, which has **no** store to reach — see the module
    /// docs. Unlike the SQL server's flag, this one *is* the boundary.
    pub workspace: Option<String>,
}

/// Read an `mcp-tasks` command line. [`None`] means this is not an MCP
/// invocation and there is nothing to do — an ordinary application launch.
pub fn parse_mcp_tasks_args(args: &[String]) -> Option<McpTasksInvocation> {
    if !is_mcp_tasks_invocation(args) {
        return None;
    }
    Some(McpTasksInvocation {
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
