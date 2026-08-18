//! Auditing the agent's own story about its work.
//!
//! [`crate::git::grouping`] answers "what changed, and why, where the why is
//! known". This module answers the two questions that grouping leaves implicit,
//! and which are the ones a reviewer signing off on agent-written code actually
//! needs:
//!
//! * **What did nothing account for?** Grouping already surfaces those hunks as
//!   `Other`; the sort in `grouping::group` now leads with them.
//! * **What did the agent claim it did that no change shows?** — an *unfulfilled
//!   claim*. A declared intent whose turn edited a file in this diff, but whose
//!   recorded text is nowhere in it.
//!
//! # This pass does no matching of its own
//!
//! Attribution already matched recorded edits onto the diff, **by content**,
//! against thresholds tuned in [`crate::git::attribution`]. The reverse pass
//! reads that result: a turn is *evidenced* when some accepted span carries its
//! id, and nothing here can be stricter or looser than the one matcher that
//! produced those spans. There is exactly one notion of "matched".
//!
//! # A wrong label is worse than no label
//!
//! The same rule the whole intent stack is built against. Only a label the
//! agent *declared* becomes a claim — a sentence mined from prose is already a
//! guess, and calling it unfulfilled would assert more than is known. An
//! unmatched claim is reported as "no matching change in this diff", never as
//! undone: the edit may be committed, later overwritten, hand-reverted, or
//! transformed past the matcher's normalisation ladder.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git::attribution::FileAttribution;
use crate::git::grouping::{self, IntentGroup};
use crate::git::patch::{FileDiff, LineOrigin};
use crate::intents::{Intents, LabelSource, ProviderId};

/// A declared intent for which no changed hunk shows matching content.
///
/// Not an accusation. See the module docs: the change may have landed and moved
/// beyond the matcher's reach. Reported as unmatched, never as undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnfulfilledClaim {
    pub turn_id: String,
    /// The declared label's text.
    pub label: String,
    pub provider: ProviderId,
    /// Files in this diff the claim's turn touched, so the reviewer knows where
    /// to look. Ascending.
    pub paths: Vec<String>,
}

/// The per-turn tally shown above the cards.
///
/// The direct answer to "did the agent do what it told me it did": how many
/// stated intents there were, how many are evidenced by a change, and how much
/// of the diff nothing accounts for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scorecard {
    /// Declared intents whose turn edited a file in this diff.
    pub claims: u32,
    /// Claims with at least one accepted span anywhere in the diff.
    pub evidenced: u32,
    /// Claims with no accepted span — always equal to `unfulfilled.len()`.
    pub unmatched: u32,
    /// Changed hunks across the tree (a hunk with at least one non-context line).
    pub hunks: u32,
    /// Hunks with at least one attributed span.
    pub attributed_hunks: u32,
    /// Changed lines across the tree that no record claimed.
    pub unattributed_lines: u32,
}

/// Grouping plus the two coverage failures and the aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IntentReview {
    pub groups: Vec<IntentGroup>,
    pub unfulfilled: Vec<UnfulfilledClaim>,
    pub scorecard: Scorecard,
}

/// Build the cards, find the unfulfilled claims, and tally the scorecard.
///
/// `diffs` and `attributions` are parallel: same scan, same order.
pub fn review(
    diffs: &[FileDiff],
    attributions: &[FileAttribution],
    intents: &Intents,
) -> IntentReview {
    let groups = grouping::group(diffs, attributions);
    let evidenced_turns = evidenced_turns(attributions);
    let claims = claims_in_play(diffs, intents);
    let claim_total = claims.len() as u32;

    let mut unfulfilled: Vec<UnfulfilledClaim> = claims
        .into_iter()
        .filter(|claim| !evidenced_turns.contains(&claim.turn_id))
        .collect();
    // Deterministic, and reads in turn order.
    unfulfilled.sort_by(|a, b| a.turn_id.cmp(&b.turn_id).then(a.label.cmp(&b.label)));

    // Evidenced is whatever is left once the unmatched are removed, so the two
    // can never disagree with the list.
    let evidenced = claim_total - unfulfilled.len() as u32;
    let scorecard = scorecard(diffs, attributions, &unfulfilled, evidenced);

    IntentReview {
        groups,
        unfulfilled,
        scorecard,
    }
}

/// Every turn that attribution accepted a span for.
fn evidenced_turns(attributions: &[FileAttribution]) -> BTreeSet<String> {
    attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.spans)
        .map(|s| s.turn_id.clone())
        .collect()
}

/// The declared claims whose turn edited a file in this diff.
///
/// Built from the records the diff actually contains, joined to their covering
/// label the same way the forward pass does (`Intents::label_for`). A turn
/// whose files are absent from the diff never appears — it is not "these
/// changes".
fn claims_in_play(diffs: &[FileDiff], intents: &Intents) -> Vec<UnfulfilledClaim> {
    // Keyed by (turn, label text) so one turn's distinct declared intents stay
    // distinct while the same intent seen across several files merges.
    let mut claims: BTreeMap<(String, String), Claim> = BTreeMap::new();

    for diff in diffs {
        for record in intents.for_path(&diff.path) {
            let Some(label) = intents.label_for(record) else {
                continue;
            };
            if label.source != LabelSource::Declared {
                continue;
            }
            let claim = claims
                .entry((record.turn_id.clone(), label.label.clone()))
                .or_insert_with(|| Claim {
                    turn_id: record.turn_id.clone(),
                    label: label.label.clone(),
                    provider: label.provider,
                    paths: BTreeSet::new(),
                });
            claim.paths.insert(diff.path.clone());
        }
    }

    claims
        .into_values()
        .map(|c| UnfulfilledClaim {
            turn_id: c.turn_id,
            label: c.label,
            provider: c.provider,
            paths: c.paths.into_iter().collect(),
        })
        .collect()
}

struct Claim {
    turn_id: String,
    label: String,
    provider: ProviderId,
    paths: BTreeSet<String>,
}

fn scorecard(
    diffs: &[FileDiff],
    attributions: &[FileAttribution],
    unfulfilled: &[UnfulfilledClaim],
    evidenced: u32,
) -> Scorecard {
    let unmatched = unfulfilled.len() as u32;

    let hunks = diffs
        .iter()
        .flat_map(|d| &d.hunks)
        .filter(|h| h.lines.iter().any(|l| l.origin != LineOrigin::Context))
        .count() as u32;

    let attributed_hunks = attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .filter(|h| !h.spans.is_empty())
        .count() as u32;

    let unattributed_lines = attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .map(|h| h.unattributed_lines)
        .sum();

    Scorecard {
        claims: evidenced + unmatched,
        evidenced,
        unmatched,
        hunks,
        attributed_hunks,
        unattributed_lines,
    }
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
