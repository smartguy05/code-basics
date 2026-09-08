//! The install, the uninstall, the status and the caveats — mirroring
//! [`crate::qgate::install`] name for name so the two installers read alike.
//!
//! One difference from the gate is deliberate and worth keeping. The gate bakes
//! its executable into a single command **string** and quotes it by hand, which
//! is the shape a shell-style hook line requires. An MCP entry has a `command`
//! field and a separate `args` array, so nothing is ever quoted and no path
//! containing a space, a quote or a `&` can be re-split by anything downstream.
//! Pinned by `the_exe_path_is_a_field_and_never_quoted_into_an_args_string`.

use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use super::{codex_toml, config_target, mcp_merge, McpConfigTarget, CLAUDE_POINTER, SERVER_NAME};
use crate::intents::providers::{InstallPlan, InstallScope, PlannedWrite};
use crate::intents::ProviderId;
use crate::mcp::argv;

/// Where the server is installed for `provider`, if anywhere. Project wins over
/// user, matching the hook installers' precedence.
///
/// `home` overrides the provider's home directory for tests; production passes
/// `None`.
pub fn status_for(provider: ProviderId, root: &Path, home: Option<&Path>) -> Option<InstallScope> {
    for scope in [InstallScope::Project, InstallScope::User] {
        if let Ok(target) = config_target(provider, root, scope, home) {
            let installed = match &target {
                McpConfigTarget::ClaudeProject(path) | McpConfigTarget::ClaudeUser(path) => {
                    mcp_merge::is_installed(path, CLAUDE_POINTER, SERVER_NAME)
                }
                McpConfigTarget::CodexUser(path) => codex_toml::is_installed(path, SERVER_NAME),
            };
            if installed {
                return Some(scope);
            }
        }
    }
    None
}

/// Everything installing the server would do, computed without touching disk.
pub fn install_plan_for(
    provider: ProviderId,
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<InstallPlan> {
    let target = config_target(provider, root, scope, home)?;

    // A project-scope entry names its workspace, so the listing is scoped to the
    // repository it was installed for; a user-scope entry fires everywhere and
    // must not name one, or it would list a single repository's connections
    // forever. `--workspace` scopes the LISTING ONLY and is not a security
    // boundary — see `crate::mcp::argv`.
    let workspace = (scope == InstallScope::Project).then_some(root);
    let command = exe_path();
    let args = entry_args(workspace);

    let (path, content, merges_existing) = match &target {
        McpConfigTarget::ClaudeProject(path) | McpConfigTarget::ClaudeUser(path) => {
            let (content, merges) = mcp_merge::merged_text(
                path,
                CLAUDE_POINTER,
                SERVER_NAME,
                &json_entry(&command, &args),
            )?;
            (path.clone(), content, merges)
        }
        McpConfigTarget::CodexUser(path) => {
            let (content, merges) = codex_toml::merged_text(path, SERVER_NAME, &command, &args)?;
            (path.clone(), content, merges)
        }
    };

    Ok(InstallPlan {
        provider,
        scope,
        writes: vec![PlannedWrite {
            path,
            content,
            merges_existing,
        }],
        caveats: caveats_for(provider, scope),
    })
}

/// Everything uninstalling the server would do, computed without touching disk.
///
/// Targets the same file [`install_plan_for`] wrote and removes only our own
/// named entry; every other server the user configured survives. A file that
/// holds no entry of ours (or does not exist) yields **zero** writes, so the UI
/// can say "nothing to remove" rather than rewrite an unchanged file.
pub fn uninstall_plan_for(
    provider: ProviderId,
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<InstallPlan> {
    let target = config_target(provider, root, scope, home)?;

    let (path, removed) = match &target {
        McpConfigTarget::ClaudeProject(path) | McpConfigTarget::ClaudeUser(path) => (
            path.clone(),
            mcp_merge::plan_removal(path, CLAUDE_POINTER, SERVER_NAME)?,
        ),
        McpConfigTarget::CodexUser(path) => {
            (path.clone(), codex_toml::plan_removal(path, SERVER_NAME)?)
        }
    };

    let writes = match removed {
        Some(content) => vec![PlannedWrite {
            path,
            content,
            merges_existing: true,
        }],
        None => Vec::new(),
    };

    Ok(InstallPlan {
        provider,
        scope,
        writes,
        caveats: Vec::new(),
    })
}

/// The entry as it appears in a Claude Code configuration.
fn json_entry(command: &str, args: &[String]) -> Value {
    json!({ "command": command, "args": args })
}

/// This copy of the application. The installed entry runs **this executable**
/// with its `mcp-sql` subcommand rather than a shipped script — the third
/// self-dispatch mode, exactly as `record-intent` and `quality-gate` are.
fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "code-basics".to_string())
}

/// The argument vector, read back through the same constants the server parses
/// so the writer and the reader cannot drift.
fn entry_args(workspace: Option<&Path>) -> Vec<String> {
    let mut args = vec![argv::SUBCOMMAND.to_string()];
    if let Some(root) = workspace {
        args.push("--workspace".to_string());
        args.push(root.display().to_string());
    }
    args
}

/// The sentence that is this feature's real security boundary.
///
/// Stated in **every** scope's caveats, verbatim, and pinned by a test. An
/// agent granted read access to a database can read whatever that login can
/// read — including credentials the database itself stores, which no amount of
/// care in [`crate::mcp`] removes. It is inherent in the grant, so the grant is
/// where it must be said, and it must not be softened into "be careful".
pub const READ_ACCESS_NOTE: &str =
    "An agent with read access can read anything that login can read, including whatever \
     credentials the database itself stores. Expose only connections whose login you would \
     give a colleague read access to.";

/// What every install shares: nothing is reachable until it is exposed, and the
/// exposure is per connection.
const CONSENT_NOTE: &str = "No connection is reachable until you switch on \"expose to agents\" \
     for it individually, and every call is forced read-only.";

/// What `--workspace` does and, more importantly, does not do.
const SCOPE_NOTE: &str = "The --workspace argument scopes the connection listing only. It is not \
     a security boundary: a connection left out of a scoped listing can still be named by id, \
     and consent stays per connection.";

/// What to warn the user about before writing, per provider and scope.
pub fn caveats_for(provider: ProviderId, scope: InstallScope) -> Vec<String> {
    let mut caveats = match (provider, scope) {
        (ProviderId::ClaudeCode, InstallScope::Project) => vec![
            "This writes .mcp.json at the repository root, which is committed and shared \
             with everyone who clones it."
                .to_string(),
            // NOT `qgate::install::SHARED_EXE_PATH_NOTE`, though that constant's
            // doc comment invites sharing. Its words are "The **hook** command
            // … will silently get no **gate**", and nothing here is a hook and
            // no gate is involved — a teammate's failure mode is Claude Code
            // being unable to start the server. Sharing a sentence that names
            // the wrong feature is not consistency, it is a wrong warning, so
            // this is the same *rule* stated for this feature rather than the
            // same *string*.
            "The command written into .mcp.json is the full path to this copy of \
             code-basics. Teammates who do not have it installed at the same path \
             will get an MCP server that fails to start."
                .to_string(),
            // Correction 3, stated rather than assumed: a project-scope server is
            // correct-but-inert until it is approved, and Claude Code records that
            // decision in ~/.claude.json (enabledMcpjsonServers/disabledMcpjsonServers)
            // rather than in the file being written here.
            "Claude Code asks you to approve a project .mcp.json the first time it sees \
             one. Until you accept, the server is configured but not loaded — the \
             decision is recorded per project in ~/.claude.json."
                .to_string(),
        ],
        (ProviderId::ClaudeCode, InstallScope::User) => vec![
            "This writes ~/.claude.json, which applies to every repository you open.".to_string(),
            "Close Claude Code before installing: this rewrites a file it is holding open, \
             and a session running at the time may overwrite the change."
                .to_string(),
        ],
        // No `.codex/` trust caveat here. That one gates whether Codex reads a
        // *repository's* .codex/ directory; this writes the global configuration,
        // which is read regardless.
        (ProviderId::Codex, InstallScope::User) => vec![
            "This writes $CODEX_HOME/config.toml, which applies to every repository you \
             open. Comments, formatting and every other setting in that file are \
             preserved."
                .to_string(),
        ],
        _ => Vec::new(),
    };

    caveats.push(CONSENT_NOTE.to_string());
    if scope == InstallScope::Project {
        caveats.push(SCOPE_NOTE.to_string());
    }
    caveats.push(READ_ACCESS_NOTE.to_string());
    caveats
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
