import type { IntentGroup, ProviderStatus } from "../ipc/types";

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

/** What to say once an import has finished, given what it returned. */
export function importFeedback(total: number): string {
  if (total === 0) return "No past agent sessions were found for this workspace.";
  return `Imported ${total} recorded intent${total === 1 ? "" : "s"}.`;
}
