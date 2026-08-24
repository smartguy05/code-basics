//! Erosion-detector command.
//!
//! Like the intent grouping, the scan is recomputed on every call rather than
//! cached: it is cheap regex over a diff libgit2 already produced, and a cached
//! result would have to be invalidated by every edit, stage and revert.

use cb_core::erosion::{self, ErosionReport};
use cb_core::git::{ComparisonMode, Repo};
use tauri::State;

use crate::state::AppState;

/// Scan the working tree for changes that quietly weaken the codebase.
///
/// Rules whose TOML would not parse (loader) or whose regex would not compile
/// (scan) are reported together in `warnings`, never dropped.
#[tauri::command]
pub async fn erosion_scan(
    state: State<'_, AppState>,
    mode: ComparisonMode,
) -> Result<ErosionReport, String> {
    let root = state.workspace_root()?;
    let repo = Repo::open(&root).map_err(|e| format!("{e:#}"))?;
    let diffs = repo.diff_all(mode).map_err(|e| format!("{e:#}"))?;

    let (rules, mut warnings) = erosion::all_rules(&root);
    let mut report = erosion::scan_diffs(&diffs, &rules);

    warnings.append(&mut report.warnings);
    report.warnings = warnings;
    Ok(report)
}
