//! Writing the Build MCP server into an agent's configuration.
//!
//! **Nothing about where MCP servers live is decided here.** That question was
//! answered, measured and tested in [`crate::mcp::install`], and this module reuses
//! every part of it: [`config_target`] for the three files, [`mcp_merge`] for the
//! pointer-based JSON merge into `.mcp.json` and `~/.claude.json`, [`codex_toml`]
//! for the `toml_edit` merge, and
//! [`crate::intents::providers::apply_writes_atomically`] for the write itself.
//!
//! What is this module's own is the three things that differ:
//!
//! 1. **The entry.** [`build_mcp_entry`] — a `command` and an `args` array, running
//!    this executable's `mcp-build` subcommand.
//! 2. **`--workspace` *is* the boundary** (the Tasks/Roslyn pattern). A
//!    **project**-scope install bakes in the one repository this server may build; a
//!    **user**-scope install writes no workspace and answers
//!    [`super::answer::BuildRefusal::NoWorkspace`] until scoped.
//! 3. **The caveats.** What an agent gains here is *running and reading the build of
//!    one repository*, and the sentence that says so has to be in front of the user
//!    before the file is written.

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
/// `command` and `args` are **separate fields**, never a quoted command string.
///
/// `workspace` is the repository this server may build — [`Some`] for a
/// project-scope install, [`None`] for a user-scope one.
pub fn build_mcp_entry(exe: &str, workspace: Option<&Path>) -> Value {
    json!({ "command": exe, "args": entry_args(workspace) })
}

/// The argument vector, read back through the same constant
/// [`super::argv::parse_mcp_build_args`] parses, so the writer and the reader
/// cannot drift.
fn entry_args(workspace: Option<&Path>) -> Vec<String> {
    let mut args = vec![argv::SUBCOMMAND.to_string()];
    if let Some(root) = workspace {
        args.push(argv::WORKSPACE_FLAG.to_string());
        args.push(root.display().to_string());
    }
    args
}

/// Where the Build server is installed for `provider`, if anywhere. Project scope
/// wins over user scope, matching the other installers' precedence.
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

    let workspace = (scope == InstallScope::Project).then_some(root);
    let command = exe_path();
    let args = entry_args(workspace);

    let (path, content, merges_existing) = match &target {
        McpConfigTarget::ClaudeProject(path) | McpConfigTarget::ClaudeUser(path) => {
            let (content, merges) = mcp_merge::merged_text(
                path,
                CLAUDE_POINTER,
                SERVER_NAME,
                &build_mcp_entry(&command, workspace),
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

/// This copy of the application. The installed entry runs **this executable** with
/// its `mcp-build` subcommand rather than a shipped script.
fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "code-basics".to_string())
}

/// The sentence that says what this grant actually is, stated in **every** scope's
/// caveats verbatim and pinned by a test.
pub const ACCESS_NOTE: &str =
    "This server lets an agent build one repository and read the build's diagnostics — its \
     errors, warnings and status, from the same build the editor runs. A build writes compiler \
     output such as bin/ and obj/, but it changes no source file, and there is no tool here that \
     edits code.";

/// What every install shares: the boundary is the workspace.
const SCOPE_NOTE: &str = "The build is scoped to the workspace this entry is installed for and \
     reaches no other repository. The application must be running with that repository open for \
     the tools to answer.";

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
             Until you accept, the server is configured but not loaded — the decision is recorded \
             per project in ~/.claude.json."
                .to_string(),
        ],
        (ProviderId::ClaudeCode, InstallScope::User) => vec![
            "This writes ~/.claude.json, which applies to every repository you open. Because it is \
             user-scoped it names no workspace, so it answers NoWorkspace until you install it \
             scoped to a repository instead."
                .to_string(),
            "Close Claude Code before installing: this rewrites a file it is holding open, and a \
             session running at the time may overwrite the change."
                .to_string(),
        ],
        (ProviderId::Codex, InstallScope::User) => vec![
            "This writes $CODEX_HOME/config.toml, which applies to every repository you open. \
             Comments, formatting and every other setting in that file are preserved. Being \
             user-scoped it names no workspace and answers NoWorkspace until scoped to a \
             repository."
                .to_string(),
        ],
        _ => Vec::new(),
    };

    caveats.push(SCOPE_NOTE.to_string());
    // Last, because it is the one the user must not scroll past.
    caveats.push(ACCESS_NOTE.to_string());
    caveats
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
