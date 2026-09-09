//! Writing the browser MCP server into an agent's configuration.
//!
//! **Nothing about where MCP servers live is decided here.** That question was
//! answered, measured and tested in [`crate::mcp::install`], and this module
//! reuses every part of it: [`config_target`] for the three files,
//! [`mcp_merge`] for the pointer-based JSON merge into `.mcp.json` and
//! `~/.claude.json`, [`codex_toml`] for the `toml_edit` merge that preserves a
//! file Codex owns, and [`crate::intents::providers::apply_writes_atomically`]
//! for the write itself. A second merge implementation would be a second set of
//! bugs in somebody's 122 KB `~/.claude.json`.
//!
//! What is this module's own is the three things that differ:
//!
//! 1. **The entry.** [`browser_mcp_entry`] — a `command` and an `args` array,
//!    running this executable's `mcp-browser` subcommand.
//! 2. **No `--workspace`.** The SQL server takes one because a connection
//!    listing can sensibly be scoped to a repository. A browser panel belongs to
//!    a *window*, not a repository, and there is exactly one panel per
//!    application — so scoping it to a workspace would be a flag that reads like
//!    a boundary and is not one. Pinned by
//!    `no_scope_writes_a_workspace_argument_because_there_is_nothing_to_scope`.
//! 3. **The caveats.** They are not the SQL ones with a word changed: what an
//!    agent gains here is *the page the user is looking at*, and the sentence
//!    that says so has to be in front of the user before the file is written.

use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use super::argv;
use super::serve::SERVER_NAME;
use crate::intents::providers::{InstallPlan, InstallScope, PlannedWrite};
use crate::intents::ProviderId;
use crate::mcp::install::{codex_toml, config_target, mcp_merge, McpConfigTarget, CLAUDE_POINTER};

/// The entry as it appears in a Claude Code configuration.
///
/// `command` and `args` are **separate fields**, never a quoted command string:
/// nothing downstream re-splits them, so an executable path containing a space,
/// a quote or a `&` is safe. The same reasoning as
/// [`crate::mcp::install::plan`]'s own `the_exe_path_is_a_field_and_never_quoted_into_an_args_string`.
pub fn browser_mcp_entry(exe: &str) -> Value {
    json!({ "command": exe, "args": entry_args() })
}

/// The argument vector, read back through the same constants
/// [`super::argv::parse_mcp_browser_args`] parses, so the writer and the reader
/// cannot drift.
///
/// Exactly one argument, the subcommand — the marker flag
/// [`super::argv::MARKER`] is **not** written. It is an accepted *alternative*
/// spelling, not a second thing this line carries: `is_mcp_browser_invocation`
/// answers true for either alone, so a human who edits this line down to just
/// the marker still starts the server. Pinned by
/// `the_entry_is_a_command_field_and_an_args_array`.
fn entry_args() -> Vec<String> {
    vec![argv::SUBCOMMAND.to_string()]
}

/// Where the browser server is installed for `provider`, if anywhere. Project
/// scope wins over user scope, matching the other installers' precedence.
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
    let command = exe_path();
    let args = entry_args();

    let (path, content, merges_existing) = match &target {
        McpConfigTarget::ClaudeProject(path) | McpConfigTarget::ClaudeUser(path) => {
            let (content, merges) = mcp_merge::merged_text(
                path,
                CLAUDE_POINTER,
                SERVER_NAME,
                &browser_mcp_entry(&command),
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
/// Removes only our own named entry; every other server the user configured
/// survives. A file holding no entry of ours yields **zero** writes, so the UI
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

/// This copy of the application. The installed entry runs **this executable**
/// with its `mcp-browser` subcommand rather than a shipped script — the fourth
/// self-dispatch mode.
fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "code-basics".to_string())
}

/// The sentence that is this feature's real boundary, stated in **every**
/// scope's caveats verbatim and pinned by a test.
///
/// It must not be softened into "be careful". What is on the other side of a
/// permission click is not a database the user created and named: it is
/// whatever page happens to be open, in their own logged-in session, which may
/// be an authenticated admin console, a password manager, or a support ticket
/// full of somebody else's data.
pub const PAGE_ACCESS_NOTE: &str =
    "An agent allowed to read the browser panel can read whatever page is open in it, in your own \
     logged-in session — including an authenticated one. Nothing is readable until you grant \
     permission for that specific page in the panel, the permission is not remembered, and it \
     ends the moment the page changes.";

/// What every install shares: the panel is the control, not this file.
const CONSENT_NOTE: &str = "Installing this grants nothing by itself. Every read and every \
     state-changing call needs your permission for the page currently open, granted in the \
     browser panel; the state-changing tools need a second, stronger permission.";

/// What the local pipe is, described as the speed bump it is rather than as
/// isolation.
///
/// The user is entitled to know the shape of the exposure before it exists: any
/// program running as this user can find the pipe, because the token that
/// nominally protects it sits in a file that same user can read. What it cannot
/// do is grant itself permission.
const PIPE_NOTE: &str = "While a browser panel is open, this application listens on a local named \
     pipe that only your own Windows account can open. That keeps other users out and nothing \
     else: another program running as you could connect to it. It still cannot read or change any \
     page without you clicking Allow in the panel, and the panel names the program that asked.";

/// There is no JavaScript evaluation tool, and the user should be told that in
/// the same breath as being told what the tools can do.
const NO_EVAL_NOTE: &str = "There is no tool that runs JavaScript in the page. The agent can read \
     the page's text and element outline, and click or type into one element at a time.";

/// What to warn the user about before writing, per provider and scope.
pub fn caveats_for(provider: ProviderId, scope: InstallScope) -> Vec<String> {
    let mut caveats = match (provider, scope) {
        (ProviderId::ClaudeCode, InstallScope::Project) => vec![
            "This writes .mcp.json at the repository root, which is committed and shared with \
             everyone who clones it."
                .to_string(),
            "The command written into .mcp.json is the full path to this copy of code-basics. \
             Teammates who do not have it installed at the same path will get an MCP server that \
             fails to start."
                .to_string(),
            "Claude Code asks you to approve a project .mcp.json the first time it sees one. \
             Until you accept, the server is configured but not loaded — the decision is \
             recorded per project in ~/.claude.json."
                .to_string(),
        ],
        (ProviderId::ClaudeCode, InstallScope::User) => vec![
            "This writes ~/.claude.json, which applies to every repository you open.".to_string(),
            "Close Claude Code before installing: this rewrites a file it is holding open, and a \
             session running at the time may overwrite the change."
                .to_string(),
        ],
        (ProviderId::Codex, InstallScope::User) => vec![
            "This writes $CODEX_HOME/config.toml, which applies to every repository you open. \
             Comments, formatting and every other setting in that file are preserved."
                .to_string(),
        ],
        _ => Vec::new(),
    };

    caveats.push(CONSENT_NOTE.to_string());
    caveats.push(NO_EVAL_NOTE.to_string());
    caveats.push(PIPE_NOTE.to_string());
    // Last, because it is the one the user must not scroll past.
    caveats.push(PAGE_ACCESS_NOTE.to_string());
    caveats
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
