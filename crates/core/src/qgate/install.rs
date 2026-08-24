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
    claude_code, codex, settings_merge, InstallPlan, InstallScope, PlannedWrite,
};
use crate::intents::ProviderId;

use anyhow::anyhow;
use std::path::PathBuf;

/// Present in the gate's command line, so its settings entry can be recognised
/// again without depending on its exact text. Distinct from the recorder's
/// `code-basics-intent` so the two coexist in the same `Stop` array.
pub const MARKER: &str = "code-basics-qgate";

/// The single lifecycle event the gate registers for.
const EVENTS: &[&str] = &["Stop"];

/// How long the gate may run. `pnpm typecheck` over a large project is not
/// instant, so this is generous next to the recorder's 5s.
const TIMEOUT_SECS: u64 = 180;

/// Where the gate is installed for this workspace, if anywhere, for Claude Code.
/// Project wins over user, matching the recorder's status precedence.
///
/// `home` overrides `~/.claude` for tests; production passes `None`. Thin wrapper
/// over the provider-aware [`status_for`], kept for existing callers.
pub fn status(root: &Path, home: Option<&Path>) -> Option<InstallScope> {
    status_for(ProviderId::ClaudeCode, root, home)
}

/// Where the gate is installed for this workspace, if anywhere, for `provider`.
/// Project wins over user.
///
/// `home` overrides the provider's home directory for tests; production passes
/// `None`.
pub fn status_for(provider: ProviderId, root: &Path, home: Option<&Path>) -> Option<InstallScope> {
    if let Ok(project) = settings_path(provider, root, InstallScope::Project, home) {
        if settings_merge::is_installed(&project, EVENTS, MARKER) {
            return Some(InstallScope::Project);
        }
    }
    if let Ok(user) = settings_path(provider, root, InstallScope::User, home) {
        if settings_merge::is_installed(&user, EVENTS, MARKER) {
            return Some(InstallScope::User);
        }
    }
    None
}

/// The file a `provider`'s gate hook is written into for a given scope.
///
/// The gate's `Stop` entry is identical for both providers — the two differ only
/// in where the file lives (Claude Code nests hooks in `settings.json`; Codex
/// gives them a file of their own) — so a single settings-shaped merge serves
/// both. Exposed so the uninstall path can target the same file the install did.
pub fn settings_path(
    provider: ProviderId,
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<PathBuf> {
    match provider {
        ProviderId::ClaudeCode => match scope {
            InstallScope::Project => Ok(claude_code::project_settings_path(root)),
            InstallScope::User => claude_user_settings(home)
                .ok_or_else(|| anyhow!("could not locate the Claude Code home directory")),
        },
        ProviderId::Codex => match scope {
            InstallScope::Project => Ok(root.join(".codex").join("hooks.json")),
            InstallScope::User => home
                .map(Path::to_path_buf)
                .or_else(codex::codex_home)
                .map(|h| h.join("hooks.json"))
                .ok_or_else(|| anyhow!("could not locate the Codex home directory")),
        },
        ProviderId::User => Err(anyhow!(
            "the quality gate cannot be installed for the user pseudo-provider"
        )),
    }
}

/// The Claude Code user-scope settings path, or `None` when the home directory
/// is unknown.
fn claude_user_settings(home: Option<&Path>) -> Option<PathBuf> {
    home.map(Path::to_path_buf)
        .or_else(claude_code::claude_home)
        .map(|h| claude_code::user_settings_path(&h))
}

/// Everything installing the gate would do for Claude Code, computed without
/// touching disk. Thin wrapper over the provider-aware [`install_plan_for`].
///
/// `home` overrides `~/.claude` for tests; production passes `None`.
pub fn install_plan(root: &Path, scope: InstallScope, home: Option<&Path>) -> Result<InstallPlan> {
    install_plan_for(ProviderId::ClaudeCode, root, scope, home)
}

/// Everything installing the gate would do for `provider`, computed without
/// touching disk.
///
/// The gate itself is provider-agnostic (`quality-gate` takes no `--provider`
/// flag), so the only things that vary by provider are the target path, the
/// plan's `provider` field, and the caveats. `home` overrides the provider's
/// home directory for tests; production passes `None`.
pub fn install_plan_for(
    provider: ProviderId,
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<InstallPlan> {
    let path = settings_path(provider, root, scope, home)?;

    // A project-scope hook names its workspace; a user-scope one fires
    // everywhere and resolves the workspace from the payload instead.
    let pin = (scope == InstallScope::Project).then_some(root);
    let (content, merges_existing) = settings_merge::merged_text(&path, &entries_for(pin), MARKER)?;

    Ok(InstallPlan {
        provider,
        scope,
        writes: vec![PlannedWrite {
            path,
            content,
            merges_existing,
        }],
        caveats: caveats_for(provider, scope, root, home),
    })
}

/// Everything uninstalling the gate would do for Claude Code, computed without
/// touching disk. Thin wrapper over the provider-aware [`uninstall_plan_for`].
///
/// `home` overrides `~/.claude` for tests; production passes `None`.
pub fn uninstall_plan(
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<InstallPlan> {
    uninstall_plan_for(ProviderId::ClaudeCode, root, scope, home)
}

/// Everything uninstalling the gate for `provider` would do, computed without
/// touching disk.
///
/// Targets the same settings file [`install_plan_for`] wrote, and removes only
/// the gate's own marked `Stop` entry via [`settings_merge::plan_removal`]. The
/// intent recorder's `Stop` entry carries a *distinct* marker, so it survives
/// untouched — which is the whole reason the two markers differ. A file that
/// holds no gate entry (or does not exist) yields **zero** writes, so the UI can
/// say "nothing to remove" rather than rewrite an unchanged file.
pub fn uninstall_plan_for(
    provider: ProviderId,
    root: &Path,
    scope: InstallScope,
    home: Option<&Path>,
) -> Result<InstallPlan> {
    let path = settings_path(provider, root, scope, home)?;

    let writes = match settings_merge::plan_removal(&path, EVENTS, MARKER)? {
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

/// What to warn the user about before writing, for `provider`.
fn caveats_for(
    provider: ProviderId,
    scope: InstallScope,
    root: &Path,
    home: Option<&Path>,
) -> Vec<String> {
    match provider {
        ProviderId::ClaudeCode => caveats(scope),
        ProviderId::Codex => codex_caveats(scope, root, home),
        ProviderId::User => Vec::new(),
    }
}

/// What to warn the user about before writing Claude Code's settings.
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

/// What to warn the user about before writing Codex's `hooks.json`.
///
/// Two conditions are Codex-specific and both leave the configuration looking
/// correct while doing nothing, so they are surfaced rather than assumed away:
/// an untrusted project (checked via [`codex::is_trusted_in`]) and Codex's
/// first-run review of a new command hook.
fn codex_caveats(scope: InstallScope, root: &Path, home: Option<&Path>) -> Vec<String> {
    let mut caveats = Vec::new();
    match scope {
        InstallScope::Project => {
            caveats.push(
                "This writes .codex/hooks.json, which is committed and shared with \
                 everyone who clones the repository."
                    .to_string(),
            );
            if !codex::is_trusted_in(home, root) {
                caveats.push(
                    "Codex ignores this repository's .codex/ directory until the project \
                     is trusted. Open it in Codex once and accept the trust prompt."
                        .to_string(),
                );
            }
        }
        InstallScope::User => {
            caveats.push(
                "A user-level hook runs when any repository's agent turn ends. The gate \
                 abstains where the tooling is absent (no `typecheck` script, no \
                 Cargo.toml), so it is safe, but it applies to every repository you open."
                    .to_string(),
            );
        }
    }
    caveats.push("Codex asks you to review a new command hook the first time it runs.".to_string());
    caveats
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
