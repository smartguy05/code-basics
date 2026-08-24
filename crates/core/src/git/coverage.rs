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

use crate::git::attribution::{Confidence, FileAttribution};
use crate::git::grouping::{self, IntentGroup};
use crate::git::patch::{FileDiff, LineOrigin};
use crate::intents::{scope_covers, IntentLabel, Intents, LabelSource, ProviderId};

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
    // Reconcile a single bare declared reason onto a single orphan geometry
    // turn, if the diff is that unambiguous. Done on a clone so the caller's
    // parallel `attributions` are untouched, and *before* grouping and the
    // tally so both read the same reconciled spans.
    let mut attributions = attributions.to_vec();
    let bound_bare = bind_bare_orphan(&mut attributions, intents);

    let groups = grouping::group(diffs, &attributions, intents);
    let evidenced_set = evidenced_claims(&attributions);
    let claims = claims_in_play(diffs, &attributions, intents, bound_bare.as_ref());
    let claim_total = claims.len() as u32;

    let mut unfulfilled: Vec<UnfulfilledClaim> = claims
        .into_iter()
        .filter(|claim| !evidenced_set.contains(&(claim.turn_id.clone(), claim.label.clone())))
        .collect();
    // Deterministic, and reads in turn order.
    unfulfilled.sort_by(|a, b| a.turn_id.cmp(&b.turn_id).then(a.label.cmp(&b.label)));

    // Evidenced is whatever is left once the unmatched are removed, so the two
    // can never disagree with the list.
    let evidenced = claim_total - unfulfilled.len() as u32;
    let scorecard = scorecard(diffs, &attributions, &unfulfilled, evidenced);

    IntentReview {
        groups,
        unfulfilled,
        scorecard,
    }
}

/// The claims some accepted span now evidences, keyed by the label's own turn.
///
/// A claim is `(label turn, label text)`, so a cross-turn or bare-bound label
/// evidences the reason that owns it rather than the turn that made the edit.
fn evidenced_claims(attributions: &[FileAttribution]) -> BTreeSet<(String, String)> {
    attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.spans)
        .filter_map(|s| {
            s.label.as_ref().map(|label| {
                let turn = s.label_turn_id.clone().unwrap_or_else(|| s.turn_id.clone());
                (turn, label.clone())
            })
        })
        .collect()
}

/// Bind a single bare declared reason to a single orphan geometry turn.
///
/// The workflow/subagent case where the reason carries no `paths`: if the whole
/// diff has exactly one orphan turn — a turn whose every span is still
/// unlabeled — and exactly one otherwise-unbound declared label with empty
/// `paths`, stamp that label onto the orphan turn's spans and report it so the
/// tally counts it as a claim. Abstains otherwise: two orphan turns or two
/// candidate labels are ambiguous, and a wrong label is worse than no label.
fn bind_bare_orphan(
    attributions: &mut [FileAttribution],
    intents: &Intents,
) -> Option<(String, String)> {
    // Orphan turns: present on some span, but none of their spans is labeled.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut labeled: BTreeSet<String> = BTreeSet::new();
    for span in attributions
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.spans)
    {
        seen.insert(span.turn_id.clone());
        if span.label.is_some() {
            labeled.insert(span.turn_id.clone());
        }
    }
    let orphans: Vec<&String> = seen.difference(&labeled).collect();
    let [orphan] = orphans.as_slice() else {
        return None;
    };
    let orphan = (*orphan).clone();

    // Candidate bare reasons: declared, no paths, from another turn, and not
    // already carried on a span. Bind only when exactly one exists.
    let already = evidenced_claims(attributions);
    let mut candidates = intents.labels.iter().filter(|l| {
        l.source == LabelSource::Declared
            && l.paths.is_empty()
            && l.turn_id != orphan
            && !already.contains(&(l.turn_id.clone(), l.label.clone()))
    });
    let bare = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let bound = (bare.turn_id.clone(), bare.label.clone());

    for span in attributions
        .iter_mut()
        .flat_map(|f| &mut f.hunks)
        .flat_map(|h| &mut h.spans)
    {
        if span.turn_id == orphan && span.label.is_none() {
            span.label = Some(bare.label.clone());
            span.label_source = Some(LabelSource::Declared);
            span.label_turn_id = Some(bare.turn_id.clone());
            // The geometry match is solid, but pinning *this* reason to it rests
            // on count-uniqueness alone — no path scope, no recency tie. So the
            // card is marked low-confidence (a path-scoped or same-turn declared
            // reason stays High): the association is shown, but as a heuristic,
            // not asserted. A wrong label is worse than no label.
            span.confidence = Confidence::Low;
        }
    }

    Some(bound)
}

/// The declared claims in play for this diff, keyed by the label's own turn.
///
/// Three sources, unioned into one map so a reason seen more than one way
/// merges rather than double-counting:
/// 1. same-turn declared reasons joined through the records the diff contains
///    (unchanged behaviour, keyed by the record's own turn);
/// 2. declared, path-scoped reasons whose `paths` cover a file in the diff —
///    this is what puts the workflow case's cross-turn label in play, even when
///    its turn made no edit in this diff;
/// 3. the bare reason the single-orphan pass bound, if any.
fn claims_in_play(
    diffs: &[FileDiff],
    attributions: &[FileAttribution],
    intents: &Intents,
    bound_bare: Option<&(String, String)>,
) -> Vec<UnfulfilledClaim> {
    // Keyed by (turn, label text) so one turn's distinct declared intents stay
    // distinct while the same intent seen across several files merges.
    let mut claims: BTreeMap<(String, String), Claim> = BTreeMap::new();

    {
        let mut add = |turn: &str, label: &IntentLabel, path: &str| {
            claims
                .entry((turn.to_string(), label.label.clone()))
                .or_insert_with(|| Claim {
                    turn_id: turn.to_string(),
                    label: label.label.clone(),
                    provider: label.provider,
                    paths: BTreeSet::new(),
                })
                .paths
                .insert(path.to_string());
        };

        // 1. Same-turn declared reasons, through records the diff contains.
        for diff in diffs {
            for record in intents.for_path(&diff.path) {
                if let Some(label) = intents.label_for(record) {
                    if label.source == LabelSource::Declared {
                        add(&record.turn_id, label, &diff.path);
                    }
                }
            }
        }

        // 2. Declared, path-scoped reasons covering a file in the diff.
        for label in &intents.labels {
            if label.source != LabelSource::Declared || label.paths.is_empty() {
                continue;
            }
            for diff in diffs {
                if label.paths.iter().any(|p| scope_covers(p, &diff.path)) {
                    add(&label.turn_id, label, &diff.path);
                }
            }
        }

        // 3. The bound bare reason, filed under wherever its label now sits.
        if let Some((turn, text)) = bound_bare {
            if let Some(label) = intents
                .labels
                .iter()
                .find(|l| &l.turn_id == turn && &l.label == text)
            {
                for file in attributions {
                    let carries = file.hunks.iter().flat_map(|h| &h.spans).any(|s| {
                        s.label.as_deref() == Some(text.as_str())
                            && s.label_turn_id.as_deref() == Some(turn.as_str())
                    });
                    if carries {
                        add(turn, label, &file.path);
                    }
                }
            }
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
