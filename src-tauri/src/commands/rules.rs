//! Business-rule-doc command.
//!
//! A thin bridge: every decision — parsing a doc's front matter, the abstain
//! fallbacks, sorting — lives in [`cb_core::rules`] and is tested there. This
//! reads the workspace's `rules/` directory and hands back what loaded plus any
//! file it could not read.

use cb_core::rules::{self, RulesReport};
use tauri::State;

use crate::state::AppState;

/// Every business-rule doc in the open workspace's `.code-basics/rules/`.
///
/// A file that will not read is reported in `warnings` alongside the docs that
/// loaded, never dropped; a missing directory is an empty report, not an error.
#[tauri::command]
pub async fn list_rules(state: State<'_, AppState>) -> Result<RulesReport, String> {
    let root = state.workspace_root()?;
    let (rules, warnings) = rules::load_rules(&rules::rules_dir(&root));
    Ok(RulesReport { rules, warnings })
}
