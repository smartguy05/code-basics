//! The testable decision seam between the raw deltas and the wire report.
//!
//! The command in `src-tauri` orchestrates processes and worktrees — none of
//! which a unit test can construct — but the *decisions* it makes (which config
//! to run, how raw deltas become [`BehavioralDelta`] values, how they split into
//! attributed vs unattributed buckets, and what the scorecard tallies) are pure
//! and live here so they can be tested headlessly.
//!
//! # `files_hint` and the safe abstain
//!
//! A test delta is attributed only when its `files_hint` lands inside exactly
//! one intent card ([`super::attribute_behavioral`]). This module leaves
//! `files_hint` **empty**: cheaply mapping a test case to the source file it
//! exercises is not possible from a test report alone (the report names the
//! test, not the code under test), and a *wrong* hint would misattribute a
//! delta to a card that did not cause it. An empty hint sends the delta to the
//! unattributed bucket — the honest bucket — which is the correct abstain. The
//! console signal still attributes by scanning changed lines for known paths.

use std::path::Path;

use super::{
    attribute_behavioral, BehavioralDelta, BehavioralReport, BehavioralScorecard, ConsoleDelta,
    HttpDelta, TestDelta,
};
use crate::git::grouping::IntentGroup;
use crate::model::RunConfig;
use crate::workspace::{self, Workspace};

/// Scan a baseline checkout into a [`Workspace`]. A thin wrapper over
/// [`workspace::scan`] kept here so the command depends on one seam.
pub fn scan_baseline(worktree_path: &Path) -> Result<Workspace, String> {
    workspace::scan(worktree_path).map_err(|e| format!("{e:#}"))
}

/// Find a configuration by id, mirroring `commands::run`'s lookup.
pub fn find_config<'a>(ws: &'a Workspace, config_id: &str) -> Option<&'a RunConfig> {
    ws.configs.iter().find(|c| c.id == config_id)
}

/// Turn the three raw signals into the wire [`BehavioralReport`].
///
/// Pure: it flattens the deltas into [`BehavioralDelta`] values, runs
/// attribution to split them into per-card and unattributed buckets, and
/// tallies the [`BehavioralScorecard`].
///
/// * `outcomes_compared` — how many signals were actually run on both sides: a
///   test suite (0 or 1), the console (0 or 1), plus one per HTTP request.
/// * `deltas` — total observable differences (a test *change* counts; an
///   unchanged console does not).
/// * `attributed` / `unattributed` — the split [`attribute_behavioral`] made.
/// * `abstained` — `warnings.len()`: every refusal (never-ready server,
///   dependency drift, teardown residue) is recorded as a warning, so the count
///   of warnings is the count of things we declined to judge.
pub fn assemble_report(
    tests: Option<TestDelta>,
    console: Option<ConsoleDelta>,
    http: Vec<HttpDelta>,
    groups: &[IntentGroup],
    warnings: Vec<String>,
) -> BehavioralReport {
    // Count what was compared before consuming the pieces.
    let outcomes_compared =
        tests.is_some() as u32 + console.as_ref().map(|_| 1).unwrap_or(0) + http.len() as u32;

    // Flatten each signal to individual observable deltas. A test suite yields
    // one delta per changed case; the console yields at most one; each HTTP
    // request that actually differs yields one.
    let mut flat: Vec<BehavioralDelta> = Vec::new();

    if let Some(td) = &tests {
        for case in &td.cases {
            flat.push(BehavioralDelta::Test(case.clone()));
        }
    }

    // Equal-after-normalising is *no delta*, not a change — the console field is
    // kept (the comparison ran, and stays counted in `outcomes_compared`) but it
    // contributes no `BehavioralDelta`.
    if let Some(c) = &console {
        if c.is_change() {
            flat.push(BehavioralDelta::Console(c.clone()));
        }
    }

    for h in &http {
        if h.is_change() {
            flat.push(BehavioralDelta::Http(h.clone()));
        }
    }

    let total_deltas = flat.len() as u32;

    let (attributions, unattributed) = attribute_behavioral(flat, groups);

    let attributed_deltas: u32 = attributions.iter().map(|c| c.deltas.len() as u32).sum();
    let unattributed_deltas = unattributed.len() as u32;

    let scorecard = BehavioralScorecard {
        outcomes_compared,
        deltas: total_deltas,
        attributed_deltas,
        unattributed_deltas,
        abstained: warnings.len() as u32,
    };

    BehavioralReport {
        tests,
        console,
        http,
        attributions,
        unattributed,
        scorecard,
        warnings,
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
