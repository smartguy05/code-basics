//! First-open setup: preview and apply the combined agent-hook install (intent
//! capture for every detected agent + the quality gate) in one step.
//!
//! Decisions live in [`cb_core::setup`]; this wrapper resolves the workspace,
//! applies the plan through the shared backup-then-write path, and does the same
//! post-install steps `enable_intent_capture` does (make the shell hooks
//! executable, create the intents directory).

use cb_core::intents::providers::{self, InstallPlan, InstallScope};
use cb_core::intents::{self, guard, whyhook};
use cb_core::setup;
use tauri::State;

use crate::state::AppState;

/// Exactly what a first-open "set up everything" would write, at `scope`.
/// Touches nothing — this is what the setup dialog renders for confirmation.
#[tauri::command]
pub async fn setup_install_plan(
    state: State<'_, AppState>,
    scope: InstallScope,
) -> Result<InstallPlan, String> {
    let root = state.workspace_root()?;
    setup::setup_plan(&root, scope, &providers::all(), None).map_err(|e| format!("{e:#}"))
}

/// Apply a confirmed first-open setup: write the combined plan, then make the
/// commit guard and durable-why hooks executable and ensure the intents
/// directory exists (so the recorder has somewhere to write).
#[tauri::command]
pub async fn install_setup(state: State<'_, AppState>, scope: InstallScope) -> Result<(), String> {
    let root = state.workspace_root()?;

    let plan =
        setup::setup_plan(&root, scope, &providers::all(), None).map_err(|e| format!("{e:#}"))?;
    providers::apply_writes(&plan.writes).map_err(|e| format!("{e:#}"))?;

    // The shell hooks (git pre-commit / post-commit) must be executable on unix;
    // a no-op on Windows. Mirrors enable_intent_capture.
    if let Some(hook) = guard::hook_path(&root) {
        if guard::is_installed(&hook) {
            guard::ensure_executable(&hook).map_err(|e| format!("{e:#}"))?;
        }
    }
    if let Some(hook) = whyhook::hook_path(&root) {
        if whyhook::is_installed(&hook) {
            whyhook::ensure_executable(&hook).map_err(|e| format!("{e:#}"))?;
        }
    }

    // The recorder refuses to write into a workspace that never opted in, so the
    // directory has to exist before the next edit lands.
    std::fs::create_dir_all(intents::intents_dir(&root))
        .map_err(|e| format!("failed to prepare the intent directory: {e}"))?;

    Ok(())
}
