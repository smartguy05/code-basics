//! Is this process the Redis MCP server, or the application?
//!
//! The seventh self-dispatch mode, beside the SQL/browser/tasks/roslyn MCP
//! servers, the intent recorder and the quality gate. Mirrors
//! [`crate::mcp::argv`] exactly; see it for why acceptance is an either-or and
//! why `--workspace` is a listing scope and **not** a security boundary (consent
//! is per-connection: `expose_to_agents` for reads, `allow_writes` for writes).

/// The subcommand the installed MCP entry runs.
pub const SUBCOMMAND: &str = "mcp-redis";

/// The marker flag the installed entry also carries.
pub const MARKER: &str = "code-basics-mcp-redis";

/// Did the command line ask for the Redis MCP server rather than the application?
pub fn is_mcp_redis_invocation(args: &[String]) -> bool {
    let marker = format!("--{MARKER}");
    args.iter().any(|arg| arg == SUBCOMMAND || *arg == marker)
}

/// What an `mcp-redis` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRedisInvocation {
    /// The workspace the entry was installed for, if any. Not a security
    /// boundary — see the module docs.
    pub workspace: Option<String>,
}

/// Read an `mcp-redis` command line. [`None`] means an ordinary app launch.
pub fn parse_mcp_redis_args(args: &[String]) -> Option<McpRedisInvocation> {
    if !is_mcp_redis_invocation(args) {
        return None;
    }
    Some(McpRedisInvocation {
        workspace: flag(args, "--workspace").filter(|w| !w.is_empty()),
    })
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let position = args.iter().position(|arg| arg == name)?;
    args.get(position + 1).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_subcommand_or_the_marker() {
        assert!(is_mcp_redis_invocation(&["mcp-redis".to_string()]));
        assert!(is_mcp_redis_invocation(&[
            "--code-basics-mcp-redis".to_string()
        ]));
        assert!(!is_mcp_redis_invocation(&["mcp-sql".to_string()]));
    }

    #[test]
    fn reads_the_workspace_scope() {
        let args = vec![
            "mcp-redis".to_string(),
            "--workspace".to_string(),
            "/repo".to_string(),
        ];
        assert_eq!(
            parse_mcp_redis_args(&args).unwrap().workspace.as_deref(),
            Some("/repo")
        );
    }

    #[test]
    fn an_empty_workspace_is_none() {
        let args = vec![
            "mcp-redis".to_string(),
            "--workspace".to_string(),
            "".to_string(),
        ];
        assert_eq!(parse_mcp_redis_args(&args).unwrap().workspace, None);
    }
}
