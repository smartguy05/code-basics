//! Is this process the browser MCP server, or the application?
//!
//! The **fourth** self-dispatch mode, beside [`crate::intents::hook`]'s
//! `record-intent`, [`crate::qgate`]'s `quality-gate` and [`crate::mcp::argv`]'s
//! `mcp-sql`, and it is here for the same three reasons: the installed MCP entry
//! runs *this executable*, so there is no second artifact to keep in step, no
//! interpreter to depend on, and no version skew between the server and the app
//! it was installed from.
//!
//! Acceptance is the same either-or as the other three, for the stated reason
//! that the command line lives in a config file the user shares with their team
//! (`.mcp.json`, `~/.claude.json`, `~/.codex/config.toml`): a hand-edited line
//! that kept only the subcommand, or only the marker, must still start the
//! server. The marker is read back through the same constant that writes it.
//!
//! # `--instance` is a disambiguator, not a permission
//!
//! It names a process id when several windows have a browser panel open, and
//! that is **all** it does. It grants nothing: the pid it names must still be
//! live, still speak this protocol, still have the plugin on and a panel open,
//! and every read and write still needs the user's consent in that window. See
//! [`super::instances::choose_instance`], which narrows on it before deciding
//! anything, and [`super::consent`], which is the actual control.
//!
//! An install never writes it — there is nothing sensible to write, since a pid
//! does not survive a restart — so in practice it is set by a user or an agent
//! answering an [`super::instances::InstanceError::Ambiguous`] refusal.

/// The subcommand the installed MCP entry runs.
pub const SUBCOMMAND: &str = "mcp-browser";

/// An accepted alternative spelling of [`SUBCOMMAND`], namespaced like the
/// other three modes so no two can be confused for one another.
///
/// The installer does **not** write it ([`super::install`] emits the subcommand
/// alone); it exists so a hand-edited command line spelling the mode as a flag
/// still starts the server rather than opening a window.
pub const MARKER: &str = "code-basics-mcp-browser";

/// The flag that names one window when several are open.
pub const INSTANCE_FLAG: &str = "--instance";

/// Did the command line ask for the browser MCP server rather than the
/// application?
pub fn is_mcp_browser_invocation(args: &[String]) -> bool {
    let marker = format!("--{MARKER}");
    args.iter().any(|arg| arg == SUBCOMMAND || *arg == marker)
}

/// What an `mcp-browser` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpBrowserInvocation {
    /// The process id from `--instance`, if it named one. [`None`] means *use
    /// the single running application*, and several running applications are
    /// then a refusal rather than a choice.
    pub instance: Option<String>,
}

/// Read an `mcp-browser` command line. [`None`] means this is not a browser MCP
/// invocation and there is nothing to do — an ordinary application launch.
pub fn parse_mcp_browser_args(args: &[String]) -> Option<McpBrowserInvocation> {
    if !is_mcp_browser_invocation(args) {
        return None;
    }
    Some(McpBrowserInvocation {
        instance: flag(args, INSTANCE_FLAG).filter(|value| !value.trim().is_empty()),
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
