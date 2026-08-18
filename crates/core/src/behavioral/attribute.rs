//! Mapping observable deltas back to the intent card that plausibly caused them.
//!
//! The evidence is weak — a test's source file is only guessed, a route's
//! handler is not known from an `.http` file, a console line rarely names a
//! path — so this follows the whole stack's rule to the letter: a delta is
//! pinned to a card **only** when its candidate files land inside exactly one
//! [`IntentGroup`]. Zero owners or two-or-more owners ⇒ the delta goes to the
//! unattributed bucket, never onto a card by guess. A delta is never split
//! across cards.

use std::collections::{BTreeMap, HashSet};

use super::{BehavioralDelta, CardBehavior};
use crate::git::attribution::Confidence;
use crate::git::grouping::IntentGroup;

/// Normalise a path for comparison: forward slashes only. Case is left intact —
/// both sides come from the same git/workspace layer and agree.
fn norm_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// The confidence one delta contributes, before the card takes the weakest.
///
/// A test transition is a single run per side — evidence, not proof — so it is
/// capped at [`Confidence::Medium`]; console and HTTP deltas already carry a
/// confidence of their own.
fn delta_confidence(delta: &BehavioralDelta) -> Confidence {
    match delta {
        BehavioralDelta::Test(_) => Confidence::Medium,
        BehavioralDelta::Console(c) => c.confidence,
        BehavioralDelta::Http(h) => h.confidence,
    }
}

/// Files this delta points at, as evidence for attribution.
///
/// * Test — its `files_hint` (filled by the caller from suite/project).
/// * Console — any known group file whose path is named in a changed line.
/// * HTTP — none; a route's handler is not derivable here, so HTTP deltas are
///   left for the unattributed bucket by design.
fn candidate_paths(delta: &BehavioralDelta, known_files: &HashSet<String>) -> HashSet<String> {
    match delta {
        BehavioralDelta::Test(case) => case.files_hint.iter().map(|p| norm_path(p)).collect(),
        BehavioralDelta::Console(console) => {
            let mut hits = HashSet::new();
            for line in console.added_lines.iter().chain(&console.removed_lines) {
                let line = norm_path(line);
                for file in known_files {
                    if line.contains(file.as_str()) {
                        hits.insert(file.clone());
                    }
                }
            }
            hits
        }
        BehavioralDelta::Http(_) => HashSet::new(),
    }
}

/// Attribute each delta to the one card that owns its candidate files, or to
/// the unattributed bucket. Cards are returned sorted by group id.
pub fn attribute_behavioral(
    deltas: Vec<BehavioralDelta>,
    groups: &[IntentGroup],
) -> (Vec<CardBehavior>, Vec<BehavioralDelta>) {
    let group_files: Vec<(String, HashSet<String>)> = groups
        .iter()
        .map(|g| {
            (
                g.id.clone(),
                g.files.iter().map(|f| norm_path(&f.path)).collect(),
            )
        })
        .collect();

    // The union of every group's files, so console lines only scan real paths.
    let known_files: HashSet<String> = group_files
        .iter()
        .flat_map(|(_, files)| files.iter().cloned())
        .collect();

    let mut by_group: BTreeMap<String, Vec<BehavioralDelta>> = BTreeMap::new();
    let mut unattributed: Vec<BehavioralDelta> = Vec::new();

    for delta in deltas {
        let cands = candidate_paths(&delta, &known_files);
        let owners: Vec<&String> = if cands.is_empty() {
            Vec::new()
        } else {
            group_files
                .iter()
                .filter(|(_, files)| !files.is_disjoint(&cands))
                .map(|(id, _)| id)
                .collect()
        };

        match owners.as_slice() {
            [only] => by_group.entry((*only).clone()).or_default().push(delta),
            // Zero owners (unowned) or ≥2 (ambiguous) — never guess.
            _ => unattributed.push(delta),
        }
    }

    let cards = by_group
        .into_iter()
        .map(|(group_id, deltas)| {
            // Weakest-member: a card is never more certain than its shakiest delta.
            let confidence = deltas
                .iter()
                .map(delta_confidence)
                .min()
                .unwrap_or(Confidence::Low);
            CardBehavior {
                group_id,
                deltas,
                confidence,
            }
        })
        .collect();

    (cards, unattributed)
}

#[cfg(test)]
#[path = "attribute_tests.rs"]
mod tests;
