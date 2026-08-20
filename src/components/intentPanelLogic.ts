import type {
  ComparisonMode,
  ErosionCategory,
  ErosionFlag,
  FileChange,
  IntentGroup,
  ProviderStatus,
  RejectSummary,
  Scorecard,
  UnfulfilledClaim,
} from "../ipc/types";

/**
 * Whether a file's changes are in the index, so an intent card can show what is
 * staged without leaving the Intent view.
 *
 * `git status` reports the two halves independently, so a file can be both
 * staged and unstaged at once (some hunks staged, others not). That third state
 * is kept distinct rather than collapsed into "staged": staging a card is only
 * fully done when nothing of it is left unstaged.
 */
export type StagedState = "staged" | "partial" | "none";

/** The staged state of one path, from the working-tree status. */
export function stagedState(path: string, files: FileChange[]): StagedState {
  const file = files.find((f) => f.path === path);
  if (!file || file.staged == null) return "none";
  return file.unstaged == null ? "staged" : "partial";
}

/**
 * The staged state of a whole card, folded from its files.
 *
 * "staged" only when every file is fully staged; "none" when not one is; and
 * "partial" for everything between — including a card whose files are each fully
 * staged or fully not, since it is still half-done as a unit.
 */
export function groupStagedState(
  paths: string[],
  files: FileChange[],
): StagedState {
  if (paths.length === 0) return "none";
  const states = paths.map((p) => stagedState(p, files));
  if (states.every((s) => s === "staged")) return "staged";
  if (states.every((s) => s === "none")) return "none";
  return "partial";
}

/**
 * Deciding what to say when nothing here is agent-stated.
 *
 * Every group being inferred looks identical whatever the cause — capture was
 * never turned on, records exist but were never imported, or capture is on and
 * simply has nothing to say about these particular edits. The panel used to
 * show the same silent "0 stated" badge for all three, which leaves the one
 * user who could fix it with no way to know that. This picks the one true
 * explanation and the actions that follow from it.
 *
 * The abstain rule the rest of the intent code follows applies here too: a
 * banner appears only when there is something concrete to say. No groups means
 * no diff to explain, and a single stated group means capture is demonstrably
 * working — both stay quiet.
 */
export type IntentDataHint =
  | { kind: "none" }
  | {
      kind: "hint";
      /** Which of the three situations this is; the component may style on it. */
      reason:
        | "captureOffWithSessions"
        | "captureOffNoSessions"
        | "capturingButNothingMatched";
      /** The sentence to show. */
      text: string;
      /** Past sessions across every provider — the count on the import button. */
      sessions: number;
      /** Offer "Enable capture": only when nothing is capturing yet. */
      canEnable: boolean;
      /** Offer "Import past sessions": only when there is something to import. */
      canImport: boolean;
      /**
       * Anything the providers reported standing in the way — a user-level hook
       * pinned to another workspace, an untrusted Codex project. These are the
       * cases where "capture is off" is not the whole truth, so they ride along
       * with the banner rather than staying buried in the collapsed setup pane.
       */
      caveats: string[];
    };

/**
 * True when the card is an intent whose file is scoped by two or more declared
 * reasons and none could be bound uniquely — so the reasons are shown as
 * candidates rather than one being guessed.
 */
export function isAmbiguousIntent(group: IntentGroup): boolean {
  return group.kind === "intent" && (group.candidates?.length ?? 0) > 0;
}

/** The candidate reasons to list under an ambiguous card; empty otherwise. */
export function cardCandidates(group: IntentGroup): string[] {
  return isAmbiguousIntent(group) ? group.candidates! : [];
}

/**
 * The text shown on the card headline. A single stated intent shows its reason;
 * an ambiguous one shows a marker (the reasons themselves render as a list
 * below); every other kind already carries a derived title in `label`.
 */
export function cardHeadline(group: IntentGroup): string {
  return isAmbiguousIntent(group) ? "Ambiguous intent" : group.label;
}

/**
 * The card's hover tooltip. A single stated intent shows its own text (the
 * headline is ellipsis-truncated, so hover is where the full intent is read); an
 * ambiguous one lists every candidate reason; every other kind is a location, so
 * it keeps its explanatory KIND_TITLE sentence.
 */
export function cardTitle(group: IntentGroup, kindTitle: string): string {
  if (isAmbiguousIntent(group)) {
    return `Two or more declared reasons scope this file:\n- ${group
      .candidates!.join("\n- ")}`;
  }
  return group.kind === "intent" ? group.label : kindTitle;
}

/** Decide the banner above the group list. */
export function intentDataHint(
  groups: IntentGroup[],
  providers: ProviderStatus[],
): IntentDataHint {
  if (groups.length === 0) return { kind: "none" };
  if (groups.some((g) => g.kind === "intent")) return { kind: "none" };

  const capturing = providers.some((p) => p.capture != null);
  const sessions = providers.reduce((sum, p) => sum + p.sessions, 0);
  const caveats = [...new Set(providers.flatMap((p) => p.caveats ?? []))];

  if (capturing) {
    return {
      kind: "hint",
      reason: "capturingButNothingMatched",
      text:
        "Capture is on, but nothing here matches a recorded intent — the " +
        "records may be from another branch, or these edits may predate them.",
      sessions,
      canEnable: false,
      canImport: sessions > 0,
      caveats,
    };
  }

  if (sessions > 0) {
    return {
      kind: "hint",
      reason: "captureOffWithSessions",
      text:
        "Nothing here is agent-stated: capture is off, so these edits were " +
        `never recorded. ${sessions} past session${sessions === 1 ? "" : "s"} ` +
        "for this workspace can be imported now.",
      sessions,
      canEnable: true,
      canImport: true,
      caveats,
    };
  }

  return {
    kind: "hint",
    reason: "captureOffNoSessions",
    text:
      "Nothing here is agent-stated: capture is off, so nothing is being " +
      "recorded for this workspace and no past sessions were found.",
    sessions: 0,
    canEnable: true,
    canImport: false,
    caveats,
  };
}

/**
 * The one-line scorecard above the cards — the direct answer to "did the agent
 * do what it told me it did".
 *
 * A reading like `4 claims · 3 evidenced · 1 unmatched · 41 hunks · 6
 * unattributed`. Reads sensibly at zero (`0 claims`), so it can show even when
 * capture is off and nothing was stated.
 */
export function scorecardLine(sc: Scorecard): string {
  return [
    `${sc.claims} claim${sc.claims === 1 ? "" : "s"}`,
    `${sc.evidenced} evidenced`,
    `${sc.unmatched} unmatched`,
    `${sc.hunks} hunk${sc.hunks === 1 ? "" : "s"}`,
    `${sc.unattributedLines} unattributed`,
  ].join(" · ");
}

/**
 * A gentle nudge to review the scope of a diff, derived purely from the
 * Scorecard and the groups already computed — no new backend signal.
 *
 * Two things suggest a diff has drifted past what the agent set out to do:
 * hunks grouped as "other" (a turn made them but stated no reason, or nothing
 * could be attributed to them at all) and changed lines the scorecard could tie
 * to no recorded intent. Neither is wrong on its own — refactors, generated
 * files and pre-agent edits all land here honestly — so this is INFORMATIONAL,
 * a prompt to glance at scope, never a claim that anything is out of place.
 *
 * The abstain rule the rest of this file follows is sharp here, because a false
 * nag trains the reader to ignore a true one:
 *  - Below {@link MIN_TOTAL_LINES} changed lines the diff is too small to be
 *    worth a second look, so it stays silent even with an unattributed line.
 *  - "notice" needs one signal present: enough unexplained groups, or a
 *    meaningful unattributed share of the diff.
 *  - "high" needs BOTH signals substantial at once — one large signal alone is
 *    still only a notice.
 */
const MIN_TOTAL_LINES = 40;
const UNEXPLAINED_NOTICE = 2;
const UNEXPLAINED_HIGH = 4;
const UNATTRIBUTED_SHARE_NOTICE = 0.4;
const UNATTRIBUTED_SHARE_HIGH = 0.6;

export function scopeCreep(
  scorecard: Scorecard,
  groups: IntentGroup[],
): { level: "notice" | "high"; message: string } | null {
  const unexplained = groups.filter((g) => g.kind === "other").length;
  const unattributed = scorecard.unattributedLines;

  // Total changed lines. Every changed line lands in exactly one group, so the
  // group line counts already sum to the whole diff; `unattributedLines` is a
  // subset of those, not a separate bucket, so it must NOT be added again (doing
  // so caps the share below 0.5 and makes "high" unreachable).
  const total = groups.reduce((sum, g) => sum + g.lineCount, 0);
  if (total < MIN_TOTAL_LINES) return null;

  const share = total > 0 ? unattributed / total : 0;

  const unexplainedNotice = unexplained >= UNEXPLAINED_NOTICE;
  const unattributedNotice = share >= UNATTRIBUTED_SHARE_NOTICE;
  if (!unexplainedNotice && !unattributedNotice) return null;

  const parts: string[] = [];
  if (unexplained > 0) {
    parts.push(`${unexplained} unexplained group${unexplained === 1 ? "" : "s"}`);
  }
  if (unattributed > 0) {
    parts.push(`${unattributed} unattributed line${unattributed === 1 ? "" : "s"}`);
  }
  const summary = parts.join(" and ");

  const unexplainedHigh = unexplained >= UNEXPLAINED_HIGH;
  const unattributedHigh = share >= UNATTRIBUTED_SHARE_HIGH;
  if (unexplainedHigh && unattributedHigh) {
    return {
      level: "high",
      message: `${summary} — a large part of this diff isn't accounted for. Worth confirming it's all in scope.`,
    };
  }

  return {
    level: "notice",
    message: `${summary} — worth a quick scope check.`,
  };
}

/**
 * A quiet risk badge for one intent card, derived only from signals the panel
 * already holds — the group's kind and confidence, the paths it touches, and
 * the erosion flags landing on its own lines. No new backend signal, and in
 * particular the per-line `AttributedSpan` is deliberately never reached for;
 * the coarse `group.confidence` that already crosses IPC is enough.
 *
 * The point of the badge is to draw a reviewer's eye to the cards most worth a
 * second look, so the abstain rule the rest of this file follows is sharp here:
 * a badge appears ONLY when something concrete elevates the card, and a card
 * that trips nothing gets no badge at all (returns `null`). A badge on every
 * card would be a badge on none.
 *
 * What elevates a card, all to "elevated":
 *  - an unexplained ("other") card — nothing states what it was for;
 *  - low attribution confidence;
 *  - a file whose path carries a {@link SENSITIVE_MARKERS} marker (auth, crypto,
 *    a migration, an `.env`, …) — a location where a mistake costs more.
 *
 * What raises it to "high": an erosion flag that lands on one of THIS card's
 * own changed lines (same file, and its `index` among the card's `lineIndices`)
 * whose category is one of {@link HIGH_RISK_EROSION} — a removed assertion or
 * safeguard, or a hardcoded secret. A lower-severity flag on the card's lines
 * still elevates, but only to "elevated". A flag elsewhere in the file, or in
 * another file, is not this card's and is ignored.
 *
 * The `reasons` drive the tooltip, in the order the checks run.
 */
// Matched at a path boundary (start, or after / \ . _ -), never as a bare
// substring, so "author(s)" is not read as "auth" and an ordinary "config" file
// is not flagged at all (it was dropped — far too broad to be a signal). Each
// marker names a location where a mistake costs more than usual.
const SENSITIVE_PATTERNS: RegExp[] = [
  /(^|[/\\._-])auth(?!or)/i, // auth, authn, authz, authentication — not author(s)
  /(^|[/\\._-])(security|secure)([/\\._-]|$)/i,
  /(^|[/\\._-])crypto/i,
  /(^|[/\\._-])secret/i,
  /(^|[/\\._-])credential/i,
  /(^|[/\\._-])passw(or)?d/i, // password / passwd
  /(^|[/\\._-])payment/i,
  /(^|[/\\._-])billing/i,
  /(^|[/\\._-])migrations?([/\\._-]|$)/i,
  /(^|[/\\._-])\.?env([/\\._-]|$)/i, // .env, .env.local, env/
];

/** Erosion categories severe enough to push a card from "elevated" to "high". */
const HIGH_RISK_EROSION: ErosionCategory[] = [
  "secret",
  "removedSafeguard",
  "deletedAssertion",
];

export function cardRisk(
  group: IntentGroup,
  erosionFlags: ErosionFlag[],
): { level: "elevated" | "high"; reasons: string[] } | null {
  const reasons: string[] = [];
  let high = false;

  if (group.kind === "other") {
    reasons.push("No stated intent — unexplained change");
  }

  if (group.confidence === "low") {
    reasons.push("Low attribution confidence");
  }

  const sensitive = group.files
    .map((f) => f.path)
    .filter((path) => SENSITIVE_PATTERNS.some((re) => re.test(path)));
  if (sensitive.length > 0) {
    reasons.push(
      `Touches sensitive path${sensitive.length === 1 ? "" : "s"}: ${sensitive.join(", ")}`,
    );
  }

  // Erosion flags landing on this card's own changed lines — same file AND the
  // flag's index among the lines the card claims. A flag anywhere else in the
  // file belongs to some other card's hunks and must not be borrowed here.
  for (const file of group.files) {
    const cardLines = new Set(file.lineIndices);
    for (const flag of erosionFlags) {
      if (flag.path !== file.path || !cardLines.has(flag.index)) continue;
      reasons.push(`Erosion flagged here: ${flag.message}`);
      if (HIGH_RISK_EROSION.includes(flag.category)) high = true;
    }
  }

  if (reasons.length === 0) return null;
  return { level: high ? "high" : "elevated", reasons };
}

/**
 * The heading for the unfulfilled-claims section, or `null` when there is
 * nothing to say.
 *
 * The wording is deliberately neutral: "no matching change in this diff", never
 * "not done" or a claim the agent lied — the change may have landed and moved
 * beyond the matcher's reach. See {@link UnfulfilledClaim}.
 */
export function unfulfilledCaption(claims: UnfulfilledClaim[]): string | null {
  if (claims.length === 0) return null;
  const n = claims.length;
  return `${n} stated intent${n === 1 ? "" : "s"} with no matching change in this diff`;
}

/** What to say once an import has finished, given what it returned. */
export function importFeedback(total: number): string {
  if (total === 0) return "No past agent sessions were found for this workspace.";
  return `Imported ${total} recorded intent${total === 1 ? "" : "s"}.`;
}

/**
 * Can this comparison mode be rejected in?
 *
 * Only the working-tree views. Reverting in the staged view changes the index,
 * so the note would be written into a working tree the reviewer is not looking
 * at — and would itself be left unstaged. Rust refuses it too; this is so the
 * button can be disabled rather than failing on click.
 */
export function canRejectInMode(mode: ComparisonMode): boolean {
  return mode === "workingToHead" || mode === "workingToIndex";
}

/** Shortest reason worth writing into a file. */
const MIN_REASON = 4;

/**
 * Why this reason will not do, or `null` when it is fine.
 *
 * The reason is the entire difference between rejecting and reverting: without
 * one this is just a revert with extra steps, and the agent learns nothing. So
 * an empty or one-word reason is refused rather than written.
 */
export function rejectReasonError(reason: string): string | null {
  const trimmed = reason.trim();

  if (trimmed.length === 0) {
    return "A rejection needs a reason — it is what the agent will read.";
  }
  if (trimmed.length < MIN_REASON) {
    return "Too short to be useful. Say what was wrong with it.";
  }
  return null;
}

/** What to say once a rejection has been applied. */
export function rejectFeedback(summary: RejectSummary): string {
  if (summary.reverted === 0) {
    return "Nothing was reverted — the group may have already moved.";
  }

  const files = `${summary.reverted} file${summary.reverted === 1 ? "" : "s"}`;
  const noted = `Reverted ${files} and left the reason in ${summary.marked.length}.`;

  if (summary.unmarked.length === 0) return noted;

  // Naming them matters: these have no line comment to write into, so the
  // reason exists nowhere and no agent will ever see it.
  return (
    `${noted} Reverted without a note (no comment syntax): ` +
    `${summary.unmarked.join(", ")}.`
  );
}
