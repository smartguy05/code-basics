import { describe, expect, it } from "vitest";
import {
  claimChecklistCaption,
  claimRows,
  type ClaimRow,
} from "./claimChecklistLogic";
import type {
  BehavioralDelta,
  BehavioralReport,
  CardBehavior,
  UnfulfilledClaim,
} from "../ipc/types";

function claim(
  turnId: string,
  label: string,
  paths: string[] = [],
): UnfulfilledClaim {
  return { turnId, label, provider: "claudeCode", paths };
}

function testDelta(): BehavioralDelta {
  return {
    kind: "test",
    fullName: "case.fixed",
    base: "failed",
    work: "passed",
    transition: "fixed",
    filesHint: [],
  };
}

function card(groupId: string, deltas: BehavioralDelta[]): CardBehavior {
  return { groupId, deltas, confidence: "medium" };
}

function report(attributions: CardBehavior[]): BehavioralReport {
  return {
    tests: null,
    console: null,
    http: [],
    attributions,
    unattributed: [],
    scorecard: {
      outcomesCompared: 0,
      deltas: 0,
      attributedDeltas: 0,
      unattributedDeltas: 0,
      abstained: 0,
    },
    warnings: [],
  };
}

describe("claimRows", () => {
  it("maps unfulfilled claims to unmatched rows", () => {
    const rows = claimRows([], [claim("t1", "add cache", ["a.ts"])], null);
    expect(rows).toEqual<ClaimRow[]>([
      { label: "add cache", paths: ["a.ts"], state: "unmatched" },
    ]);
  });

  it("maps evidenced claims with no behavioral report to evidenced rows", () => {
    const rows = claimRows([claim("t1", "add cache", ["a.ts"])], [], null);
    expect(rows).toEqual<ClaimRow[]>([
      { label: "add cache", paths: ["a.ts"], state: "evidenced" },
    ]);
  });

  it("promotes an evidenced claim to corroborated when its card carries a delta", () => {
    // Card id convention: intent:{turnId}:{label}
    const rep = report([card("intent:t1:add cache", [testDelta()])]);
    const rows = claimRows([claim("t1", "add cache", ["a.ts"])], [], rep);
    expect(rows[0]?.state).toBe("corroborated");
  });

  it("builds the id from the fields even when the label contains a colon", () => {
    const rep = report([card("intent:t9:fix: the bug", [testDelta()])]);
    const rows = claimRows([claim("t9", "fix: the bug")], [], rep);
    expect(rows[0]?.state).toBe("corroborated");
  });

  it("stays evidenced when the matching card has no deltas (abstain)", () => {
    const rep = report([card("intent:t1:add cache", [])]);
    const rows = claimRows([claim("t1", "add cache")], [], rep);
    expect(rows[0]?.state).toBe("evidenced");
  });

  it("stays evidenced when no card matches the claim id", () => {
    const rep = report([card("intent:other:thing", [testDelta()])]);
    const rows = claimRows([claim("t1", "add cache")], [], rep);
    expect(rows[0]?.state).toBe("evidenced");
  });

  it("sorts unmatched first, then evidenced, then corroborated", () => {
    const rep = report([card("intent:t3:corr", [testDelta()])]);
    const rows = claimRows(
      [claim("t2", "ev"), claim("t3", "corr")],
      [claim("t1", "un")],
      rep,
    );
    expect(rows.map((r) => r.state)).toEqual([
      "unmatched",
      "evidenced",
      "corroborated",
    ]);
    expect(rows.map((r) => r.label)).toEqual(["un", "ev", "corr"]);
  });

  it("keeps input order within a state group (stable)", () => {
    const rows = claimRows(
      [claim("t1", "first"), claim("t2", "second")],
      [],
      null,
    );
    expect(rows.map((r) => r.label)).toEqual(["first", "second"]);
  });

  it("returns an empty list when nothing was declared", () => {
    expect(claimRows([], [], null)).toEqual([]);
  });
});

describe("claimChecklistCaption", () => {
  it("is null when there are no rows", () => {
    expect(claimChecklistCaption([])).toBeNull();
  });

  it("summarises the counts by state", () => {
    const rep = report([card("intent:t3:corr", [testDelta()])]);
    const rows = claimRows(
      [claim("t2", "ev"), claim("t3", "corr")],
      [claim("t1", "un")],
      rep,
    );
    expect(claimChecklistCaption(rows)).toBe(
      "3 claims · 1 corroborated · 1 evidenced · 1 unmatched",
    );
  });

  it("uses the singular for a single claim", () => {
    const rows = claimRows([], [claim("t1", "un")], null);
    expect(claimChecklistCaption(rows)).toBe(
      "1 claim · 0 corroborated · 0 evidenced · 1 unmatched",
    );
  });
});
