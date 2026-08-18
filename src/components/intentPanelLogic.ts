import type {
  ComparisonMode,
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
 * The card's hover tooltip. A stated intent shows its own text (the headline is
 * ellipsis-truncated, so hover is where the full intent is read); every other
 * kind is a location, so it keeps its explanatory KIND_TITLE sentence.
 */
export function cardTitle(group: IntentGroup, kindTitle: string): string {
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
