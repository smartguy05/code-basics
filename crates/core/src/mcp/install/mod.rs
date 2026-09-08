//! Installing the SQL MCP server into an agent's configuration — previewed
//! before anything is written, exactly as the hook installers are.
//!
//! Reuses the machinery that already exists rather than growing a second one:
//! an [`InstallPlan`](crate::intents::providers::InstallPlan) of
//! [`PlannedWrite`](crate::intents::providers::PlannedWrite)s is computed here
//! without touching disk, shown to the user, and applied by
//! [`apply_writes_atomically`](crate::intents::providers::apply_writes_atomically).
//! The shape mirrors [`crate::qgate::install`] name for name.
//!
//! # This is deliberately not an extension of `qgate::install::settings_path`
//!
//! That function answers *where does a provider keep its **hooks***. This one
//! answers *where does a provider keep its **MCP servers***. The answers are
//! different files in every single case — and the failure mode of conflating
//! them is severe, because `.claude/settings.json` is owned by two hook
//! installers whose plans carry full file contents. One function answering two
//! unrelated questions would eventually be changed for one of them and silently
//! break the other, so they stay apart.
//!
//! # Where MCP servers actually live, measured rather than assumed
//!
//! | provider | scope | file | shape |
//! |---|---|---|---|
//! | Claude Code | project | `<root>/.mcp.json` | top-level `mcpServers` |
//! | Claude Code | user | `~/.claude.json` | top-level `mcpServers` |
//! | Codex | user | `$CODEX_HOME/config.toml` | `[mcp_servers.<name>]` |
//!
//! Two of those are surprising enough to be worth naming. `.claude/settings.json`
//! holds only `hooks` in this tree and is **not** where `mcpServers` goes —
//! pinned by `claude_project_scope_is_dot_mcp_json_at_the_root_and_not_dot_claude_settings`.
//! And `~/.claude.json` is a *sibling* of `~/.claude`, not a file inside it.
//!
//! Codex has **no project scope**, and [`config_target`] returns an error
//! naming that rather than inventing `<root>/.codex/config.toml`. A refusal is
//! recoverable; a file that looks installed and is never read is not.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::intents::providers::{claude_code, codex, home_dir, InstallScope};
use crate::intents::ProviderId;

pub mod codex_toml;
pub mod mcp_merge;
pub mod plan;

/// Read a config file this app is about to merge into: `Ok(None)` when it is
/// genuinely absent, `Err` when it exists and could not be read.
///
/// **The one function in this module whose whole purpose is refusing to collapse
/// two answers into one.** `read_to_string(path).ok()` treats a permissions
/// error, a Windows sharing violation and non-UTF-8 bytes exactly as it treats
/// a file that was never there — and every caller here goes on to *write*.
///
/// Getting that wrong is not a cosmetic bug. The absent case legitimately plans
/// a brand-new config, and `merges_existing: false` legitimately tells
/// `providers::apply_writes_atomically` there is nothing to back up. Applied to
/// a file that does exist, those two correct behaviours compose into renaming a
/// four-line document over the user's real config with no `.bak` — and
/// `~/.claude.json` is 122 KB, holds every project's trust decisions, and is
/// rewritten continuously by a running Claude Code, so being briefly unreadable
/// is its ordinary condition rather than a rare accident.
///
/// A refusal here is always recoverable: the user closes the agent and retries.
pub(crate) fn read_existing(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(anyhow!(
            "{} exists but could not be read ({error}), so it was left untouched. \
             If an agent is running, close it and try again.",
            path.display()
        )),
    }
}

/// The name this server is installed under, read back through the server's own
/// constant so an installed entry and the running process cannot disagree.
pub use crate::mcp::serve::SERVER_NAME;

/// The JSON nesting path holding the server map in both Claude Code files.
///
/// A slice rather than a literal because [`mcp_merge`] takes the pointer as a
/// parameter — which is what would let a per-project map inside `~/.claude.json`
/// (`["projects", "<root>", "mcpServers"]`) be added with no new code.
pub const CLAUDE_POINTER: &[&str] = &["mcpServers"];

/// `<root>/.mcp.json` — Claude Code's project-scope MCP file.
const CLAUDE_PROJECT_FILE: &str = ".mcp.json";

/// `~/.claude.json` — Claude Code's user-scope configuration. Note the leading
/// dot and the absence of a directory: this is a sibling of `~/.claude`.
const CLAUDE_USER_FILE: &str = ".claude.json";

/// `$CODEX_HOME/config.toml`.
const CODEX_CONFIG_FILE: &str = "config.toml";

/// Which file an install would write, and in which dialect.
///
/// The variants are not decoration: the two JSON files and the one TOML file
/// need different merges, and an enum makes forgetting one a compile error
/// rather than a silently unhandled case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpConfigTarget {
    /// `<root>/.mcp.json`, committed and shared with the team.
    ClaudeProject(PathBuf),
    /// `~/.claude.json`, rewritten continuously by a running Claude Code.
    ClaudeUser(PathBuf),
    /// `$CODEX_HOME/config.toml`, the global Codex configuration.
    CodexUser(PathBuf),
}

impl McpConfigTarget {
    /// The file itself.
    pub fn path(&self) -> &Path {
        match self {
            Self::ClaudeProject(path) | Self::ClaudeUser(path) | Self::CodexUser(path) => path,
        }
    }
}

/// Where `provider` keeps its MCP servers for `scope`.
///
/// `home` overrides the **provider's home directory** for tests — `~/.claude`
/// for Claude Code, `$CODEX_HOME` for Codex — matching
/// [`crate::qgate::install::settings_path`]'s convention exactly, so the two
/// installers take the same argument and cannot be called with each other's.
/// Production passes `None`.
pub fn config_target(
    provider: ProviderId,
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<McpConfigTarget> {
    match (provider, scope) {
        (ProviderId::ClaudeCode, InstallScope::Project) => Ok(McpConfigTarget::ClaudeProject(
            root.join(CLAUDE_PROJECT_FILE),
        )),
        (ProviderId::ClaudeCode, InstallScope::User) => claude_user_config(home)
            .map(McpConfigTarget::ClaudeUser)
            .ok_or_else(|| anyhow!("could not locate the Claude Code home directory")),
        (ProviderId::Codex, InstallScope::User) => home
            .map(Path::to_path_buf)
            .or_else(codex::codex_home)
            .map(|h| McpConfigTarget::CodexUser(h.join(CODEX_CONFIG_FILE)))
            .ok_or_else(|| anyhow!("could not locate the Codex home directory")),
        (ProviderId::Codex, InstallScope::Project) => Err(anyhow!(
            "Codex reads MCP servers only from $CODEX_HOME/config.toml and has no \
             per-repository equivalent. Install at user scope instead — writing a \
             <root>/.codex/config.toml would look installed and never be read."
        )),
        (ProviderId::User, _) => Err(anyhow!(
            "the user pseudo-provider is not an agent and has no MCP configuration"
        )),
    }
}

/// `~/.claude.json`.
///
/// The `home` override names `~/.claude` (the provider home, as everywhere
/// else), so the user-scope MCP file is its **sibling** and has to be derived
/// rather than joined. With no override, the user's home directory is used
/// directly — `claude_code::claude_home` would append `.claude` only for it to
/// be stripped again.
fn claude_user_config(home: Option<&Path>) -> Option<PathBuf> {
    match home {
        Some(provider_home) => provider_home.parent().map(|p| p.join(CLAUDE_USER_FILE)),
        None => home_dir()
            .or_else(|| claude_code::claude_home().and_then(|h| h.parent().map(Path::to_path_buf)))
            .map(|h| h.join(CLAUDE_USER_FILE)),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
