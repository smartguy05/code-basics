import { describe, expect, it } from "vitest";
import {
  canRejectInMode,
  importFeedback,
  intentDataHint,
  rejectFeedback,
  rejectReasonError,
} from "./intentPanelLogic";
import type { IntentGroup, ProviderStatus } from "../ipc/types";

function group(kind: IntentGroup["kind"], id = kind): IntentGroup {
  return {
    id,
    kind,
    label: `a ${kind} group`,
    files: [{ path: "src/a.ts", lineIndices: [1], hunks: [0] }],
    lineCount: 1,
    confidence: "medium",
  };
}

function provider(over: Partial<ProviderStatus> = {}): ProviderStatus {
  return {
    provider: "claudeCode",
    detected: true,
    capture: null,
    sessions: 0,
    ...over,
  };
}

const PINNED =
  "Your user-level hook is pinned to C:\\Users\\Someone\\Code\\ONEflight and " +
  "will not record here. Enable capture again to repair it — the entry is " +
  "replaced, not duplicated.";

describe("intentDataHint", () => {
  it("says nothing when at least one group is a stated intent", () => {
    const hint = intentDataHint([group("intent"), group("other")], [provider()]);
    expect(hint.kind).toBe("none");
  });

  it("says nothing when there are no groups at all", () => {
    expect(intentDataHint([], [provider()]).kind).toBe("none");
    expect(intentDataHint([], [provider({ capture: "user" })]).kind).toBe("none");
  });

  it("offers both enabling and importing when capture is off and sessions exist", () => {
    const hint = intentDataHint(
      [group("other"), group("formatting")],
      [provider({ sessions: 3 }), provider({ provider: "codex", sessions: 1 })],
    );
    expect(hint).toMatchObject({
      kind: "hint",
      reason: "captureOffWithSessions",
      canEnable: true,
      canImport: true,
      sessions: 4,
    });
    if (hint.kind !== "hint") throw new Error("expected a hint");
    expect(hint.text).toMatch(/capture is off/i);
    expect(hint.text).toMatch(/4 past session/);
  });

  it("offers only enabling when capture is off and no sessions were found", () => {
    const hint = intentDataHint([group("modifiedSymbol")], [provider()]);
    expect(hint).toMatchObject({
      kind: "hint",
      reason: "captureOffNoSessions",
      canEnable: true,
      canImport: false,
      sessions: 0,
    });
    if (hint.kind !== "hint") throw new Error("expected a hint");
    expect(hint.text).toMatch(/nothing is being recorded/i);
  });

  it("explains a branch or age mismatch when capture is on but nothing matched", () => {
    const hint = intentDataHint(
      [group("newSymbol")],
      [provider({ capture: "project", sessions: 2 })],
    );
    expect(hint).toMatchObject({
      kind: "hint",
      reason: "capturingButNothingMatched",
      canEnable: false,
      canImport: true,
      sessions: 2,
    });
    if (hint.kind !== "hint") throw new Error("expected a hint");
    expect(hint.text).toMatch(/another branch/i);
    expect(hint.text).toMatch(/predate/i);
  });

  it("surfaces provider caveats, deduplicated, in the hint", () => {
    const hint = intentDataHint(
      [group("other")],
      [
        provider({ caveats: [PINNED] }),
        provider({ provider: "codex", caveats: [PINNED, "This project is untrusted."] }),
      ],
    );
    if (hint.kind !== "hint") throw new Error("expected a hint");
    expect(hint.caveats).toEqual([PINNED, "This project is untrusted."]);
  });

  it("carries no caveats when the providers report none", () => {
    const hint = intentDataHint([group("other")], [provider()]);
    if (hint.kind !== "hint") throw new Error("expected a hint");
    expect(hint.caveats).toEqual([]);
  });

  it("treats an empty provider list as capture being off with no sessions", () => {
    expect(intentDataHint([group("other")], [])).toMatchObject({
      reason: "captureOffNoSessions",
      canEnable: true,
      canImport: false,
    });
  });
});

describe("importFeedback", () => {
  it("names how many records were imported", () => {
    expect(importFeedback(7)).toBe("Imported 7 recorded intents.");
  });

  it("uses the singular for one", () => {
    expect(importFeedback(1)).toBe("Imported 1 recorded intent.");
  });

  it("says plainly when nothing was found", () => {
    expect(importFeedback(0)).toBe("No past agent sessions were found for this workspace.");
  });
});

// ---------------------------------------------------------------------------
// Rejecting
// ---------------------------------------------------------------------------

describe("canRejectInMode", () => {
  it("allows the working-tree views", () => {
    expect(canRejectInMode("workingToHead")).toBe(true);
    expect(canRejectInMode("workingToIndex")).toBe(true);
  });

  // Reverting in the staged view changes the index, so a note written into the
  // working tree would explain a change the reviewer is not looking at.
  it("refuses the staged view", () => {
    expect(canRejectInMode("indexToHead")).toBe(false);
  });
});

describe("rejectReasonError", () => {
  it("accepts an ordinary reason", () => {
    expect(rejectReasonError("regex mis-detects a column named limit")).toBeNull();
  });

  // The reason is the entire difference between rejecting and reverting.
  it("requires something to have been typed", () => {
    expect(rejectReasonError("")).not.toBeNull();
    expect(rejectReasonError("   \n ")).not.toBeNull();
  });

  it("requires more than a shrug", () => {
    expect(rejectReasonError("no")).not.toBeNull();
    expect(rejectReasonError("bad")).not.toBeNull();
  });
});

describe("rejectFeedback", () => {
  it("says nothing happened when nothing did", () => {
    const text = rejectFeedback({ reverted: 0, marked: [], unmarked: [] });
    expect(text).toMatch(/nothing/i);
  });

  it("reports what was reverted and noted", () => {
    const text = rejectFeedback({ reverted: 2, marked: ["a.ts", "b.ts"], unmarked: [] });
    expect(text).toContain("2");
    expect(text).not.toMatch(/without a note/i);
  });

  // A reviewer who typed a reason has to be told when it went nowhere,
  // otherwise they believe the agent was informed.
  it("names the files that could not carry the reason", () => {
    const text = rejectFeedback({ reverted: 2, marked: ["a.ts"], unmarked: ["data.json"] });
    expect(text).toContain("data.json");
    expect(text).toMatch(/without a note/i);
  });
});
