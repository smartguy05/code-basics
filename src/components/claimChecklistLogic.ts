import type { BehavioralReport, UnfulfilledClaim } from "../ipc/types";

/**
 * The per-claim checklist that replaces the aggregate scorecard line plus the
 * flat unfulfilled list in the intent view.
 *
 * Each declared intent becomes one row carrying its own state, so a reviewer
 * reads "did the agent do what it said" claim by claim rather than as a tally:
 *
 *  - `unmatched`   — no changed hunk evidences the claim ({@link IntentReview.unfulfilled}).
 *  - `evidenced`   — an accepted span matches the claim ({@link IntentReview.evidenced}).
 *  - `corroborated`— an evidenced claim whose intent card *also* produced an
 *    observable runtime delta in the before/after run.
 *
 * The abstain rule the rest of the intent code follows holds here too: a claim
 * is only promoted to `corroborated` when there is concrete runtime evidence
 * (a matching card that actually carries deltas). A missing behavioral run, a
 * card with no deltas, or no matching card at all all leave the claim at
 * `evidenced` rather than inventing corroboration.
 */
export type ClaimState = "unmatched" | "evidenced" | "corroborated";

/** One row of the checklist. */
export interface ClaimRow {
  /** The declared label's text. */
  label: string;
  /** Files in this diff the claim's turn touched. */
  paths: string[];
  state: ClaimState;
}

/**
 * The intent card id for one claim: `intent:{turnId}:{label}`.
 *
 * Built from the fields, never by parsing an existing id string — a label may
 * itself contain a colon, so splitting an id string apart would be wrong.
 * `git/grouping.rs` composes the card id the same way, which is what lets this
 * key line up with {@link CardBehavior.groupId}.
 */
function cardId(claim: UnfulfilledClaim): string {
  return `intent:${claim.turnId}:${claim.label}`;
}

/**
 * The checklist rows for the intent view.
 *
 * Sorted with the most actionable first — `unmatched`, then `evidenced`, then
 * `corroborated` — and stable within each state, so rows keep the order the
 * backend reported them in.
 */
export function claimRows(
  evidenced: UnfulfilledClaim[],
  unfulfilled: UnfulfilledClaim[],
  behavioral?: BehavioralReport | null,
): ClaimRow[] {
  // Card id → its behavioral attribution, so an evidenced claim can find any
  // runtime delta attributed to its own card in O(1).
  const byGroup = new Map(
    (behavioral?.attributions ?? []).map((card) => [card.groupId, card]),
  );

  const unmatchedRows: ClaimRow[] = unfulfilled.map((claim) => ({
    label: claim.label,
    paths: claim.paths,
    state: "unmatched",
  }));

  const evidencedRows: ClaimRow[] = evidenced.map((claim) => {
    const card = byGroup.get(cardId(claim));
    // Corroborated only with concrete runtime evidence: a matching card that
    // actually carries at least one delta. A card with no deltas is not
    // corroboration — abstain to plain evidenced.
    const corroborated = card != null && card.deltas.length > 0;
    return {
      label: claim.label,
      paths: claim.paths,
      state: corroborated ? "corroborated" : "evidenced",
    };
  });

  const rank: Record<ClaimState, number> = {
    unmatched: 0,
    evidenced: 1,
    corroborated: 2,
  };
  // Array.prototype.sort is stable, so equal-rank rows keep their input order.
  return [...unmatchedRows, ...evidencedRows].sort(
    (a, b) => rank[a.state] - rank[b.state],
  );
}

/**
 * A one-line summary of the checklist, twin of `scorecardLine` — a reading like
 * `3 claims · 1 corroborated · 1 evidenced · 1 unmatched`. `null` when there is
 * nothing declared, so the caption simply does not render.
 */
export function claimChecklistCaption(rows: ClaimRow[]): string | null {
  if (rows.length === 0) return null;
  const count = (state: ClaimState) => rows.filter((r) => r.state === state).length;
  return [
    `${rows.length} claim${rows.length === 1 ? "" : "s"}`,
    `${count("corroborated")} corroborated`,
    `${count("evidenced")} evidenced`,
    `${count("unmatched")} unmatched`,
  ].join(" · ");
}
