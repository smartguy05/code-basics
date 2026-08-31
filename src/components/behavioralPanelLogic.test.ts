import { describe, expect, it } from "vitest";
import {
  behavioralBadge,
  behavioralScoreLine,
  deltaConfidenceNote,
  deltaDetail,
  EVIDENCE_LINE_CAP,
  testCaseRows,
  unattributedReason,
  httpFileCandidates,
  httpStatusTone,
  pickBehavioralConfig,
  resolveHttpFiles,
  transitionTone,
  type HttpFileSelection,
} from "./behavioralPanelLogic";
import type {
  BehavioralDelta,
  BehavioralScorecard,
  CardBehavior,
  CaseTransition,
  FileChange,
  RunConfig,
} from "../ipc/types";

function testDelta(
  transition: CaseTransition,
  fullName = `case.${transition}`,
): BehavioralDelta {
  return {
    kind: "test",
    fullName,
    base: null,
    work: null,
    transition,
    filesHint: [],
  };
}

function consoleDelta(): BehavioralDelta {
  return {
    kind: "console",
    addedLines: ["+ a"],
    removedLines: [],
    normalized: true,
    confidence: "medium",
  };
}

function httpDelta(status: [number, number] | null): BehavioralDelta {
  return {
    kind: "http",
    name: "GET /orders",
    status,
    headerChanges: [],
    body: null,
    confidence: "high",
  };
}

function card(deltas: BehavioralDelta[]): CardBehavior {
  return { groupId: "g1", deltas, confidence: "medium" };
}

describe("transitionTone", () => {
  it("marks a regression as a warning", () => {
    expect(transitionTone("regressed")).toBe("warning");
  });

  it("marks a still-failing case as a warning", () => {
    expect(transitionTone("stillFailing")).toBe("warning");
  });

  it("marks a fix as positive", () => {
    expect(transitionTone("fixed")).toBe("positive");
  });

  it("treats unchanged, added and removed as neutral", () => {
    expect(transitionTone("unchanged")).toBe("neutral");
    expect(transitionTone("added")).toBe("neutral");
    expect(transitionTone("removed")).toBe("neutral");
  });
});

describe("behavioralBadge", () => {
  it("warns when any test regressed, even alongside a fix", () => {
    const badge = behavioralBadge(
      card([testDelta("regressed"), testDelta("fixed")]),
    );
    expect(badge.tone).toBe("warning");
    expect(badge.label).toMatch(/1 regressed/);
    expect(badge.label).toMatch(/1 fixed/);
  });

  it("is positive when the card only fixes things", () => {
    const badge = behavioralBadge(card([testDelta("fixed"), testDelta("fixed")]));
    expect(badge.tone).toBe("positive");
    expect(badge.label).toMatch(/2 fixed/);
  });

  it("counts an http status regression as a warning", () => {
    const badge = behavioralBadge(card([httpDelta([200, 500])]));
    expect(badge.tone).toBe("warning");
    expect(badge.title).toMatch(/200/);
    expect(badge.title).toMatch(/500/);
  });

  it("treats an http status improvement as positive", () => {
    const badge = behavioralBadge(card([httpDelta([500, 200])]));
    expect(badge.tone).toBe("positive");
  });

  it("treats a non-status http change as neutral", () => {
    const badge = behavioralBadge(card([httpDelta(null)]));
    expect(badge.tone).toBe("neutral");
    expect(badge.label).toMatch(/1 response/);
  });

  it("reports a console change neutrally", () => {
    const badge = behavioralBadge(card([consoleDelta()]));
    expect(badge.tone).toBe("neutral");
    expect(badge.label).toMatch(/console/i);
  });

  it("reads sensibly for an empty card", () => {
    const badge = behavioralBadge(card([]));
    expect(badge.tone).toBe("neutral");
    expect(badge.label).toMatch(/no change/i);
  });
});

describe("behavioralScoreLine", () => {
  function sc(over: Partial<BehavioralScorecard> = {}): BehavioralScorecard {
    return {
      outcomesCompared: 0,
      deltas: 0,
      attributedDeltas: 0,
      unattributedDeltas: 0,
      abstained: 0,
      ...over,
    };
  }

  it("formats the runtime tally with plurals", () => {
    const line = behavioralScoreLine(
      sc({
        outcomesCompared: 3,
        deltas: 2,
        attributedDeltas: 1,
        unattributedDeltas: 1,
        abstained: 1,
      }),
    );
    expect(line).toBe(
      "3 outcomes compared · 2 deltas · 1 attributed · 1 unattributed · 1 abstained",
    );
  });

  it("uses the singular for one outcome and one delta", () => {
    const line = behavioralScoreLine(sc({ outcomesCompared: 1, deltas: 1 }));
    expect(line).toContain("1 outcome compared ·");
    expect(line).toContain("1 delta ·");
  });

  it("reads sensibly at zero", () => {
    expect(behavioralScoreLine(sc())).toBe(
      "0 outcomes compared · 0 deltas · 0 attributed · 0 unattributed · 0 abstained",
    );
  });
});

describe("httpStatusTone", () => {
  it("warns when leaving the 2xx band", () => {
    expect(httpStatusTone(200, 500)).toBe("warning");
    expect(httpStatusTone(200, 404)).toBe("warning");
  });

  it("is positive when entering the 2xx band", () => {
    expect(httpStatusTone(500, 200)).toBe("positive");
  });

  it("compares by code within the same non-2xx band", () => {
    expect(httpStatusTone(400, 500)).toBe("warning");
    expect(httpStatusTone(500, 400)).toBe("positive");
  });

  it("is neutral for an unchanged code", () => {
    expect(httpStatusTone(200, 200)).toBe("neutral");
    expect(httpStatusTone(404, 404)).toBe("neutral");
  });
});

describe("resolveHttpFiles", () => {
  it("returns null for the auto mode", () => {
    expect(resolveHttpFiles({ mode: "auto" })).toBeNull();
  });

  it("normalises an explicit list: dedupes and drops blanks", () => {
    const selection: HttpFileSelection = {
      mode: "explicit",
      files: ["api/orders.http", "api/orders.http", "  "],
    };
    expect(resolveHttpFiles(selection)).toEqual(["api/orders.http"]);
  });

  it("falls back to auto (null) for an empty explicit list", () => {
    // Matches the backend treating Some(empty) == None == discover.
    expect(resolveHttpFiles({ mode: "explicit", files: [] })).toBeNull();
  });

  it("returns null when an explicit list is only blanks", () => {
    expect(resolveHttpFiles({ mode: "explicit", files: ["", "   "] })).toBeNull();
  });
});

describe("httpFileCandidates", () => {
  const change = (path: string): FileChange =>
    ({
      path,
      oldPath: null,
      staged: null,
      unstaged: "modified",
      isBinary: false,
    }) as FileChange;

  it("returns changed .http and .rest paths", () => {
    const files = [
      change("api/orders.http"),
      change("api/health.rest"),
      change("src/main.ts"),
    ];
    expect(httpFileCandidates(files)).toEqual(["api/orders.http", "api/health.rest"]);
  });

  it("is case-insensitive on the extension", () => {
    expect(httpFileCandidates([change("api/Orders.HTTP")])).toEqual(["api/Orders.HTTP"]);
  });

  it("returns an empty list when nothing matches", () => {
    expect(httpFileCandidates([change("src/main.ts")])).toEqual([]);
  });
});

describe("pickBehavioralConfig", () => {
  const config = (id: string, kind: string): RunConfig =>
    ({ id, name: id, kind }) as unknown as RunConfig;

  it("prefers the first test config", () => {
    const configs = [config("run", "app"), config("t", "test"), config("t2", "test")];
    expect(pickBehavioralConfig(configs)?.id).toBe("t");
  });

  it("falls back to the first config when none are tests", () => {
    expect(pickBehavioralConfig([config("run", "app")])?.id).toBe("run");
  });

  it("returns null when there are no configs", () => {
    expect(pickBehavioralConfig([])).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// The evidence detail: what the panel and the verify-claims prompt both render
// under a one-line delta summary.
// ---------------------------------------------------------------------------

describe("deltaDetail", () => {
  it("renders console removals before additions, each prefixed", () => {
    const rows = deltaDetail({
      kind: "console",
      addedLines: ["now this"],
      removedLines: ["was this"],
      normalized: false,
      confidence: "medium",
    });
    expect(rows.map((r) => r.text)).toEqual(["- was this", "+ now this"]);
    expect(rows.map((r) => r.kind)).toEqual(["removed", "added"]);
  });

  it("notes masking only when the run actually normalised something", () => {
    const masked = deltaDetail({
      kind: "console",
      addedLines: ["a"],
      removedLines: [],
      normalized: true,
      confidence: "low",
    });
    expect(masked.at(-1)?.kind).toBe("note");
    expect(masked.at(-1)?.text).toMatch(/masked/i);

    const raw = deltaDetail({
      kind: "console",
      addedLines: ["a"],
      removedLines: [],
      normalized: false,
      confidence: "low",
    });
    expect(raw.every((r) => r.kind !== "note")).toBe(true);
  });

  it("caps each side and says how many it withheld", () => {
    const many = Array.from({ length: EVIDENCE_LINE_CAP + 3 }, (_, i) => `line ${i}`);
    const rows = deltaDetail({
      kind: "console",
      addedLines: many,
      removedLines: [],
      normalized: false,
      confidence: "medium",
    });
    expect(rows.filter((r) => r.kind === "added")).toHaveLength(EVIDENCE_LINE_CAP);
    expect(rows.at(-1)).toEqual({ text: "+3 more", tone: "neutral", kind: "note" });
  });

  it("renders an http status, its header changes and its body lines", () => {
    const rows = deltaDetail({
      kind: "http",
      name: "GET /orders",
      status: [200, 500],
      headerChanges: [
        { name: "x-total", before: "3", after: "4" },
        { name: "x-new", before: null, after: "1" },
        { name: "x-gone", before: "1", after: null },
      ],
      body: { addedLines: ["}"], removedLines: ["{"], normalized: true },
      confidence: "medium",
    });
    expect(rows.map((r) => r.text)).toEqual([
      "status 200 → 500",
      "x-total: 3 → 4",
      "x-new: absent → 1",
      "x-gone: 1 → absent",
      "- {",
      "+ }",
      "timestamps and ids were masked before comparing",
    ]);
    expect(rows[0]?.tone).toBe("warning");
  });

  it("says a response changed even with no status, header or body detail", () => {
    const rows = deltaDetail({
      kind: "http",
      name: "GET /orders",
      status: null,
      headerChanges: [],
      body: null,
      confidence: "high",
    });
    expect(rows).toEqual([
      { text: "the response differed, but no status, header or body detail was recorded", tone: "neutral", kind: "note" },
    ]);
  });

  it("spells a test case's before and after outcome, absent sides included", () => {
    expect(
      deltaDetail({
        kind: "test",
        fullName: "Ns.Case",
        base: "failed",
        work: "passed",
        transition: "fixed",
        filesHint: [],
      }),
    ).toEqual([{ text: "failed → passed", tone: "positive", kind: "note" }]);

    expect(
      deltaDetail({
        kind: "test",
        fullName: "Ns.Case",
        base: null,
        work: "passed",
        transition: "added",
        filesHint: [],
      })[0]?.text,
    ).toBe("absent → passed");
  });
});

describe("deltaConfidenceNote", () => {
  it("reports the confidence console and http deltas carry", () => {
    expect(deltaConfidenceNote(consoleDelta())).toBe("medium confidence");
    expect(deltaConfidenceNote(httpDelta(null))).toBe("high confidence");
  });

  it("has nothing to report for a test delta, which carries none of its own", () => {
    expect(deltaConfidenceNote(testDelta("fixed"))).toBeNull();
  });
});

describe("unattributedReason", () => {
  it("names the rule that kept each kind off a card", () => {
    expect(unattributedReason(testDelta("regressed"))).toMatch(/not mapped to a source file/);
    expect(unattributedReason(httpDelta([200, 500]))).toMatch(/handler is not derivable/);
    expect(unattributedReason(consoleDelta())).toMatch(/no single intent card/);
  });
});

describe("testCaseRows", () => {
  const summary = { total: 1, passed: 1, failed: 0, skipped: 0, other: 0 };

  it("lists each changed case with its transition and outcome pair", () => {
    const rows = testCaseRows({
      cases: [
        {
          fullName: "Ns.Broke",
          base: "passed",
          work: "failed",
          transition: "regressed",
          filesHint: [],
        },
      ],
      summaryBefore: summary,
      summaryAfter: summary,
    });
    expect(rows).toEqual([
      { text: "regressed: Ns.Broke (passed → failed)", tone: "warning", kind: "note" },
    ]);
  });

  it("says so plainly when nothing moved, rather than rendering nothing", () => {
    const rows = testCaseRows({ cases: [], summaryBefore: summary, summaryAfter: summary });
    expect(rows).toEqual([
      { text: "no test case changed outcome", tone: "neutral", kind: "note" },
    ]);
  });
});
