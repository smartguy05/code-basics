use super::*;
use crate::intents::providers::InstallScope;
use crate::intents::ProviderId;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(r"C:\work\repo")
}

/// Correction 1, pinned: Claude Code's **project** MCP scope is `<root>/.mcp.json`
/// at the repository root. It is emphatically *not* `.claude/settings.json`,
/// which in this tree holds only `hooks` — writing there would produce a file
/// that looks installed and does nothing, and would collide with the two hook
/// installers that own it.
#[test]
fn claude_project_scope_is_dot_mcp_json_at_the_root_and_not_dot_claude_settings() {
    let target = config_target(ProviderId::ClaudeCode, &root(), InstallScope::Project, None)
        .expect("claude project scope resolves");

    assert_eq!(
        target,
        McpConfigTarget::ClaudeProject(root().join(".mcp.json"))
    );

    let path = target.path().to_string_lossy().replace('\\', "/");
    assert!(path.ends_with("/.mcp.json"), "{path}");
    assert!(!path.contains("/.claude/"), "{path}");
    assert!(!path.contains("settings.json"), "{path}");
}

/// The user-scope file is `~/.claude.json` — a *sibling* of `~/.claude`, not a
/// file inside it. The `home` override names the provider home exactly as
/// `qgate::install` does, so the sibling has to be derived rather than joined.
#[test]
fn claude_user_scope_is_the_claude_json_sibling_of_the_claude_home() {
    let home = PathBuf::from(r"C:\Users\me\.claude");
    let target = config_target(
        ProviderId::ClaudeCode,
        &root(),
        InstallScope::User,
        Some(&home),
    )
    .expect("claude user scope resolves");

    assert_eq!(
        target,
        McpConfigTarget::ClaudeUser(PathBuf::from(r"C:\Users\me\.claude.json"))
    );
}

#[test]
fn codex_user_scope_is_config_toml_in_the_codex_home() {
    let home = PathBuf::from(r"C:\Users\me\.codex");
    let target = config_target(ProviderId::Codex, &root(), InstallScope::User, Some(&home))
        .expect("resolves");

    assert_eq!(target, McpConfigTarget::CodexUser(home.join("config.toml")));
}

/// Codex reads MCP servers only from `$CODEX_HOME/config.toml`. Inventing a
/// per-repository `<root>/.codex/config.toml` would write a file that looks
/// installed and is never read. A refusal is recoverable; a silent no-op is not.
#[test]
fn codex_has_no_project_scope_and_says_why_rather_than_inventing_a_path() {
    let err = config_target(ProviderId::Codex, &root(), InstallScope::Project, None)
        .expect_err("codex project scope must be refused");
    let message = err.to_string().to_lowercase();

    assert!(message.contains("codex"), "{message}");
    assert!(
        message.contains("config.toml") || message.contains("codex_home"),
        "the refusal must name where Codex does read MCP servers: {message}"
    );
}

#[test]
fn the_user_pseudo_provider_has_no_mcp_configuration() {
    assert!(config_target(ProviderId::User, &root(), InstallScope::User, None).is_err());
}

/// The name is read back through the server's own constant so an install and
/// the running server can never disagree about what was installed.
#[test]
fn the_installed_name_is_the_servers_own_name() {
    assert_eq!(SERVER_NAME, crate::mcp::serve::SERVER_NAME);
}
