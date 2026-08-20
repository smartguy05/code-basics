//! First-open setup: one combined plan that installs the agent hooks a fresh
//! workspace is offered — intent capture (for every detected agent) **and** the
//! quality gate — in a single preview-and-apply.
//!
//! The one thing this module exists to get right: intent capture and the quality
//! gate both write Claude Code's `settings.json`, and a [`PlannedWrite`] carries
//! the *full* file content computed at plan time. Planning them independently and
//! applying both would make the second overwrite the first. So the gate's `Stop`
//! entry is **chained onto** the intent recorder's settings.json content (via
//! [`qgate::install::merged_into`]) when both target the same file, yielding one
//! write that carries both markers. Every other file (the instruction splice, the
//! commit guard, the durable-why hook, Codex's own hook file) has no conflict and
//! is appended unchanged.

use std::path::Path;

use anyhow::Result;

use crate::intents::providers::{InstallPlan, InstallScope, PlannedWrite, Provider};
use crate::intents::ProviderId;
use crate::qgate;

/// Build the combined install plan without touching disk.
///
/// `providers` are the agents to install intent capture for (only the detected
/// ones are used); production passes [`crate::intents::providers::all`]. `gate_home`
/// overrides `~/.claude` for the quality gate's user-scope path in tests.
pub fn setup_plan(
    root: &Path,
    scope: InstallScope,
    providers: &[Box<dyn Provider>],
    gate_home: Option<&Path>,
) -> Result<InstallPlan> {
    let mut writes: Vec<PlannedWrite> = Vec::new();
    let mut caveats: Vec<String> = Vec::new();

    // Intent capture for every detected agent.
    for provider in providers.iter().filter(|p| p.detected()) {
        let plan = provider.install_plan(root, scope)?;
        writes.extend(plan.writes);
        caveats.extend(plan.caveats);
    }

    // The quality gate. Its single write is Claude Code's settings.json.
    let gate = qgate::install::install_plan(root, scope, gate_home)?;
    let gate_write = gate
        .writes
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("the quality gate produced no write"))?;
    let pin = (scope == InstallScope::Project).then_some(root);

    match writes.iter_mut().find(|w| w.path == gate_write.path) {
        // Already writing that settings.json (intent capture for Claude Code):
        // chain the gate's entry onto its content so both markers land at once.
        // The gate's caveats are dropped here — it is not introducing a new file,
        // and intent capture has already warned about this one, so repeating a
        // near-identical note would only be noise.
        Some(existing) => existing.content = qgate::install::merged_into(&existing.content, pin)?,
        // Not otherwise touched: the gate introduces its own write, so its
        // caveats are the only warning about that file and must be kept.
        None => {
            writes.push(gate_write);
            caveats.extend(gate.caveats);
        }
    }

    Ok(InstallPlan {
        provider: ProviderId::ClaudeCode,
        scope,
        writes,
        caveats: dedup_preserving_order(caveats),
    })
}

/// Drop exact-duplicate caveats while keeping first-seen order.
fn dedup_preserving_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
