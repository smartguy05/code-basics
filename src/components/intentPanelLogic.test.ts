import { describe, expect, it } from "vitest";
import {
  canRejectInMode,
  cardRisk,
  cardCandidates,
  cardHeadline,
  cardTitle,
  isAmbiguousIntent,
  groupStagedState,
  importFeedback,
  intentDataHint,
  rejectFeedback,
  rejectReasonError,
  scopeCreep,
  scorecardLine,
  stagedState,
  unfulfilledCaption,
} from "./intentPanelLogic";
import type {
  ErosionFlag,
  FileChange,
  GroupFile,
  IntentGroup,
  ProviderStatus,
  Scorecard,
  UnfulfilledClaim,
} from "../ipc/types";

function change(over: Partial<FileChange> & { path: string }): FileChange {
  return {
    oldPath: null,
    staged: null,
    unstaged: null,
    isBinary: false,
    ...over,
  };
}

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

describe("cardTitle", () => {
  it("shows the group's own label for a stated intent", () => {
    const g = group("intent");
    expect(cardTitle(g, "a kind sentence")).toBe(g.label);
  });

  it("keeps the explanatory kind sentence for every non-intent kind", () => {
    expect(cardTitle(group("formatting"), "whitespace only")).toBe("whitespace only");
    expect(cardTitle(group("modifiedSymbol"), "body changed")).toBe("body changed");
    expect(cardTitle(group("other"), "grouped by file")).toBe("grouped by file");
  });

  it("lists every candidate for an ambiguous intent", () => {
    const g = { ...group("intent"), label: "", candidates: ["reason one", "reason two"] };
    const title = cardTitle(g, "unused");
    expect(title).toContain("reason one");
    expect(title).toContain("reason two");
  });
});

describe("ambiguous intent helpers", () => {
  it("detects an ambiguous intent only when candidates are present", () => {
    expect(isAmbiguousIntent({ ...group("intent"), candidates: ["a", "b"] })).toBe(true);
    expect(isAmbiguousIntent(group("intent"))).toBe(false); // single label, no candidates
    expect(isAmbiguousIntent({ ...group("other"), candidates: ["a", "b"] })).toBe(false); // not intent
  });

  it("headline shows the label normally and a marker when ambiguous", () => {
    expect(cardHeadline(group("intent"))).toBe("a intent group");
    expect(cardHeadline({ ...group("intent"), label: "", candidates: ["a", "b"] })).toBe(
      "Ambiguous intent",
    );
  });

  it("cardCandidates returns the reasons only when ambiguous", () => {
    expect(cardCandidates({ ...group("intent"), candidates: ["a", "b"] })).toEqual(["a", "b"]);
    expect(cardCandidates(group("intent"))).toEqual([]);
  });
});

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

describe("scorecardLine", () => {
  function card(over: Partial<Scorecard> = {}): Scorecard {
    return {
      claims: 0,
      evidenced: 0,
      unmatched: 0,
      hunks: 0,
      attributedHunks: 0,
      unattributedLines: 0,
      ...over,
    };
  }

  it("reads as a compact summary with plurals", () => {
    const line = scorecardLine(
      card({ claims: 4, evidenced: 3, unmatched: 1, hunks: 41, unattributedLines: 6 }),
    );
    expect(line).toBe("4 claims · 3 evidenced · 1 unmatched · 41 hunks · 6 unattributed");
  });

  it("uses the singular for one claim and one hunk", () => {
    const line = scorecardLine(card({ claims: 1, evidenced: 1, hunks: 1 }));
    expect(line).toContain("1 claim ·");
    expect(line).toContain("1 hunk ·");
  });

  it("reads sensibly at zero", () => {
    expect(scorecardLine(card())).toBe(
      "0 claims · 0 evidenced · 0 unmatched · 0 hunks · 0 unattributed",
    );
  });
});

describe("unfulfilledCaption", () => {
  function claim(over: Partial<UnfulfilledClaim> = {}): UnfulfilledClaim {
    return { turnId: "t", label: "l", provider: "claudeCode", paths: [], ...over };
  }

  it("is null when there is nothing unmatched", () => {
    expect(unfulfilledCaption([])).toBeNull();
  });

  it("uses the singular for one", () => {
    const text = unfulfilledCaption([claim()]);
    expect(text).toBe("1 stated intent with no matching change in this diff");
  });

  it("uses the plural for several", () => {
    const text = unfulfilledCaption([claim(), claim()]);
    expect(text).toContain("2 stated intents");
  });

  // A wrong label is worse than none: the wording never accuses.
  it("never accuses the agent of lying or not doing the work", () => {
    const text = unfulfilledCaption([claim()]) ?? "";
    expect(text).not.toMatch(/not done/i);
    expect(text).not.toMatch(/lie|lied/i);
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

describe("stagedState", () => {
  const files = [
    change({ path: "src/a.ts", staged: "modified" }), // fully staged
    change({ path: "src/b.ts", unstaged: "modified" }), // not staged
    change({ path: "src/c.ts", staged: "modified", unstaged: "modified" }), // partial
  ];

  it("reports a fully staged file as staged", () => {
    expect(stagedState("src/a.ts", files)).toBe("staged");
  });

  it("reports an unstaged file as none", () => {
    expect(stagedState("src/b.ts", files)).toBe("none");
  });

  it("reports a partially staged file as partial", () => {
    expect(stagedState("src/c.ts", files)).toBe("partial");
  });

  it("treats a path absent from status as none", () => {
    expect(stagedState("src/missing.ts", files)).toBe("none");
  });
});

describe("groupStagedState", () => {
  const staged = change({ path: "a", staged: "modified" });
  const unstaged = change({ path: "b", unstaged: "modified" });
  const partial = change({ path: "c", staged: "modified", unstaged: "modified" });

  it("is staged only when every file is fully staged", () => {
    expect(groupStagedState(["a"], [staged])).toBe("staged");
    expect(groupStagedState(["a", "b"], [staged, unstaged])).toBe("partial");
  });

  it("is none only when no file is staged at all", () => {
    expect(groupStagedState(["b"], [unstaged])).toBe("none");
  });

  it("is partial when any file is partially staged", () => {
    expect(groupStagedState(["c"], [partial])).toBe("partial");
  });

  it("is none for an empty group", () => {
    expect(groupStagedState([], [])).toBe("none");
  });
});

// -- scope-creep + risk badges ----------------------------------------------

const gfile = (path: string, lineIndices: number[] = []): GroupFile => ({
  path,
  lineIndices,
  hunks: [],
});

const mkGroup = (over: Partial<IntentGroup>): IntentGroup => ({
  id: "g",
  kind: "intent",
  label: "why",
  files: [],
  lineCount: 0,
  confidence: "high",
  ...over,
});

const card = (over: Partial<Scorecard>): Scorecard => ({
  claims: 0,
  evidenced: 0,
  unmatched: 0,
  hunks: 0,
  attributedHunks: 0,
  unattributedLines: 0,
  ...over,
});

const eflag = (over: Partial<ErosionFlag>): ErosionFlag => ({
  path: "a.ts",
  line: 1,
  index: 0,
  origin: "addition",
  category: "secret",
  ruleId: "r",
  message: "m",
  content: "c",
  ...over,
});

describe("scopeCreep", () => {
  it("abstains on a small diff even with an unattributed line", () => {
    const groups = [mkGroup({ lineCount: 10 })];
    expect(scopeCreep(card({ unattributedLines: 5 }), groups)).toBeNull();
  });

  it("abstains when there are no changes at all", () => {
    expect(scopeCreep(card({}), [])).toBeNull();
  });

  it("notices when unexplained groups cross the floor on a large diff", () => {
    const groups = [
      mkGroup({ kind: "other", lineCount: 20 }),
      mkGroup({ kind: "other", lineCount: 20 }),
    ];
    const out = scopeCreep(card({ unattributedLines: 0 }), groups);
    expect(out?.level).toBe("notice");
  });

  it("notices when a meaningful share of the diff is unattributed", () => {
    // total = sum(lineCount) = 100; share = 40/100 = 0.4 -> the NOTICE threshold.
    const groups = [mkGroup({ lineCount: 100 })];
    const out = scopeCreep(card({ unattributedLines: 40 }), groups);
    expect(out?.level).toBe("notice");
  });

  it("escalates to high only when BOTH signals are substantial", () => {
    // 5 unexplained groups (>=4) and share 60/100 = 0.6 (>=0.6): high is now
    // reachable — the denominator is sum(lineCount), not summed with unattributed.
    const groups = Array.from({ length: 5 }, () => mkGroup({ kind: "other", lineCount: 20 }));
    const out = scopeCreep(card({ unattributedLines: 60 }), groups);
    expect(out?.level).toBe("high");
  });

  it("stays at notice when only one signal is substantial", () => {
    // Many unexplained groups but a small unattributed share.
    const groups = Array.from({ length: 5 }, () => mkGroup({ kind: "other", lineCount: 20 }));
    const out = scopeCreep(card({ unattributedLines: 10 }), groups);
    expect(out?.level).toBe("notice");
  });
});

describe("cardRisk", () => {
  it("abstains for a high-confidence stated card in a plain path with no flag", () => {
    const g = mkGroup({ confidence: "high", files: [gfile("src/util/math.ts", [0, 1])] });
    expect(cardRisk(g, [])).toBeNull();
  });

  it("elevates an unexplained (other) card", () => {
    const g = mkGroup({ kind: "other", files: [gfile("src/util/math.ts")] });
    expect(cardRisk(g, [])?.level).toBe("elevated");
  });

  it("does not elevate a titled same-turn card in a plain path", () => {
    const g = mkGroup({ kind: "sameTurn", label: "3 files changed together", files: [gfile("src/util/math.ts")] });
    expect(cardRisk(g, [])).toBeNull();
  });

  it("elevates on low confidence", () => {
    const g = mkGroup({ confidence: "low", files: [gfile("src/util/math.ts")] });
    expect(cardRisk(g, [])?.level).toBe("elevated");
  });

  it("elevates on a sensitive path and names it", () => {
    const g = mkGroup({ files: [gfile("src/auth/session.ts")] });
    const out = cardRisk(g, []);
    expect(out?.level).toBe("elevated");
    expect(out?.reasons.some((r) => r.includes("auth/session.ts"))).toBe(true);
  });

  it("does not treat 'author' or an ordinary config file as sensitive", () => {
    expect(cardRisk(mkGroup({ files: [gfile("src/models/author.ts")] }), [])).toBeNull();
    expect(cardRisk(mkGroup({ files: [gfile("vite.config.ts")] }), [])).toBeNull();
  });

  it("goes high on an intersecting secret erosion flag", () => {
    const g = mkGroup({ files: [gfile("src/util/math.ts", [4, 5])] });
    const out = cardRisk(g, [eflag({ path: "src/util/math.ts", index: 5, category: "secret" })]);
    expect(out?.level).toBe("high");
  });

  it("stays elevated (not high) on an intersecting low-severity flag", () => {
    const g = mkGroup({ files: [gfile("src/util/math.ts", [4, 5])] });
    const out = cardRisk(g, [eflag({ path: "src/util/math.ts", index: 5, category: "droppedLog" })]);
    expect(out?.level).toBe("elevated");
  });

  it("ignores an erosion flag on a different line or in a different file", () => {
    const g = mkGroup({ files: [gfile("src/util/math.ts", [4, 5])] });
    expect(cardRisk(g, [eflag({ path: "src/util/math.ts", index: 9, category: "secret" })])).toBeNull();
    expect(cardRisk(g, [eflag({ path: "src/other.ts", index: 5, category: "secret" })])).toBeNull();
  });
});
