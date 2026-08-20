//! Installing the quality-gate Stop hook — the same method the intent hooks use.
//!
//! A [`InstallPlan`](crate::intents::providers::InstallPlan) of
//! [`PlannedWrite`](crate::intents::providers::PlannedWrite)s is computed here
//! without touching disk, previewed in the setup panel, then applied by
//! [`apply_writes`](crate::intents::providers::apply_writes) (backup-then-write).
//! The only file written is Claude Code's `settings.json`, merged surgically by
//! [`settings_merge`](crate::intents::providers::settings_merge) so the gate's
//! `Stop` entry sits alongside the intent recorder's without either removing the
//! other — they are told apart by their markers.

use std::path::Path;

use anyhow::Result;
use serde_json::{json, Map, Value};

use crate::intents::providers::{
    claude_code, settings_merge, InstallPlan, InstallScope, PlannedWrite,
};
use crate::intents::ProviderId;

/// Present in the gate's command line, so its settings entry can be recognised
/// again without depending on its exact text. Distinct from the recorder's
/// `code-basics-intent` so the two coexist in the same `Stop` array.
pub const MARKER: &str = "code-basics-qgate";

/// The single lifecycle event the gate registers for.
const EVENTS: &[&str] = &["Stop"];

/// How long the gate may run. `pnpm typecheck` over a large project is not
/// instant, so this is generous next to the recorder's 5s.
const TIMEOUT_SECS: u64 = 180;

/// Where the gate is installed for this workspace, if anywhere. Project wins
/// over user, matching the recorder's status precedence.
///
/// `home` overrides `~/.claude` for tests; production passes `None`.
pub fn status(root: &Path, home: Option<&Path>) -> Option<InstallScope> {
    if settings_merge::is_installed(&claude_code::project_settings_path(root), EVENTS, MARKER) {
        return Some(InstallScope::Project);
    }
    if let Some(user) = user_settings(home) {
        if settings_merge::is_installed(&user, EVENTS, MARKER) {
            return Some(InstallScope::User);
        }
    }
    None
}

/// The user-scope settings path, or `None` when the home directory is unknown.
fn user_settings(home: Option<&Path>) -> Option<std::path::PathBuf> {
    home.map(Path::to_path_buf)
        .or_else(claude_code::claude_home)
        .map(|h| claude_code::user_settings_path(&h))
}

/// Everything installing the gate would do, computed without touching disk.
///
/// `home` overrides `~/.claude` for tests; production passes `None`.
pub fn install_plan(root: &Path, scope: InstallScope, home: Option<&Path>) -> Result<InstallPlan> {
    let path = match scope {
        InstallScope::Project => claude_code::project_settings_path(root),
        InstallScope::User => user_settings(home)
            .ok_or_else(|| anyhow::anyhow!("could not locate the Claude Code home directory"))?,
    };

    // A project-scope hook names its workspace; a user-scope one fires
    // everywhere and resolves the workspace from the payload instead.
    let pin = (scope == InstallScope::Project).then_some(root);
    let (content, merges_existing) = settings_merge::merged_text(&path, &entries_for(pin), MARKER)?;

    Ok(InstallPlan {
        provider: ProviderId::ClaudeCode,
        scope,
        writes: vec![PlannedWrite {
            path,
            content,
            merges_existing,
        }],
        caveats: caveats(scope),
    })
}

/// Merge the gate's `Stop` entry into a settings.json **string**, returning the
/// new text. Used by the combined first-open setup plan to chain the gate merge
/// on top of the intent recorder's settings.json so both land in one write.
pub fn merged_into(existing: &str, workspace: Option<&Path>) -> Result<String> {
    settings_merge::merged_into_text(existing, &entries_for(workspace), MARKER)
}

/// The `Stop` entry as it should appear in the file.
fn entries_for(workspace: Option<&Path>) -> Value {
    let entry = json!({
        "matcher": "",
        "hooks": [ {
            "type": "command",
            "command": command_line(workspace),
            "timeout": TIMEOUT_SECS,
        } ],
    });
    let mut hooks = Map::new();
    hooks.insert("Stop".to_string(), Value::Array(vec![entry]));
    Value::Object(hooks)
}

/// The command the hook runs: this application's `quality-gate` subcommand.
///
/// It invokes the app itself rather than a shipped script — no second artifact,
/// no interpreter dependency — exactly like the recorder. A project-scope hook
/// names its workspace; a user-scope one must not, or it would run against the
/// one repository open at install time forever.
fn command_line(workspace: Option<&Path>) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "code-basics".to_string());

    let mut line = format!("\"{exe}\" {} --{MARKER}", crate::qgate::SUBCOMMAND);
    if let Some(root) = workspace {
        line.push_str(&format!(" --workspace \"{}\"", root.display()));
    }
    line
}

/// What to warn the user about before writing.
fn caveats(scope: InstallScope) -> Vec<String> {
    match scope {
        InstallScope::Project => vec![
            "This writes .claude/settings.json, which is committed and shared with \
             everyone who clones the repository."
                .to_string(),
        ],
        InstallScope::User => vec![
            "A user-level hook runs when any repository's agent turn ends. The gate \
             abstains where the tooling is absent (no `typecheck` script, no \
             Cargo.toml), so it is safe, but it applies to every repository you open."
                .to_string(),
        ],
    }
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
