//! Is this process the Roslyn MCP server, or the application?
//!
//! The **sixth** self-dispatch mode, beside [`crate::intents::hook`]'s
//! `record-intent`, [`crate::qgate`]'s `quality-gate`, [`crate::mcp::argv`]'s
//! `mcp-sql`, the browser server's `mcp-browser` and the Tasks server's
//! `mcp-tasks`, and it is here for the same three reasons: the installed MCP
//! entry runs *this executable*, so there is no second artifact to keep in step,
//! no interpreter to depend on, and no version skew between the server and the
//! app it was installed from.
//!
//! Acceptance is the same either-or as the others, for the stated reason that the
//! command line lives in a config file the user shares with their team
//! (`.mcp.json`, `~/.claude.json`, `~/.codex/config.toml`): a hand-edited line
//! that kept only the subcommand, or only the marker, must still start the
//! server. The marker is read back through the same constant that writes it.
//!
//! # `--workspace` *is* the boundary
//!
//! Unlike the browser's `--instance` (a disambiguator that grants nothing),
//! `--workspace <root>` names the one repository this server may reach — the
//! Tasks pattern. An unscoped invocation reaches no session and every tool
//! answers [`super::answer::RoslynRefusal::NoWorkspace`] rather than guessing a
//! directory. The install step bakes it in, so it is the consent boundary: an
//! agent configured for repo X cannot reach repo Y's semantic model.
//!
//! # `--instance` is a disambiguator, not a permission
//!
//! It names a process id when several windows have the same repository open, and
//! that is **all** it does. See [`super::instances::choose_instance`], which
//! narrows on it before deciding anything.

/// The subcommand the installed MCP entry runs.
pub const SUBCOMMAND: &str = "mcp-roslyn";

/// An accepted alternative spelling of [`SUBCOMMAND`], namespaced like the other
/// modes so no two can be confused for one another.
///
/// The installer does **not** write it ([`super::install`] emits the subcommand
/// alone); it exists so a hand-edited command line spelling the mode as a flag
/// still starts the server rather than opening a window.
pub const MARKER: &str = "code-basics-mcp-roslyn";

/// The flag that names the repository this server may reach — the boundary.
pub const WORKSPACE_FLAG: &str = "--workspace";

/// The flag that names one window when several have the same repository open.
pub const INSTANCE_FLAG: &str = "--instance";

/// Did the command line ask for the Roslyn MCP server rather than the
/// application?
pub fn is_mcp_roslyn_invocation(args: &[String]) -> bool {
    let marker = format!("--{MARKER}");
    args.iter().any(|arg| arg == SUBCOMMAND || *arg == marker)
}

/// What an `mcp-roslyn` command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRoslynInvocation {
    /// The workspace the entry was installed for, if it named one. [`None`] is an
    /// unscoped install, which reaches **no** session — see the module docs.
    pub workspace: Option<String>,
    /// The process id from `--instance`, if it named one. [`None`] means *use the
    /// single application that has the workspace open*, and several such
    /// applications are then a refusal rather than a choice.
    pub instance: Option<String>,
}

/// Read an `mcp-roslyn` command line. [`None`] means this is not a Roslyn MCP
/// invocation and there is nothing to do — an ordinary application launch.
pub fn parse_mcp_roslyn_args(args: &[String]) -> Option<McpRoslynInvocation> {
    if !is_mcp_roslyn_invocation(args) {
        return None;
    }
    Some(McpRoslynInvocation {
        workspace: flag(args, WORKSPACE_FLAG).filter(|value| !value.trim().is_empty()),
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
