import { describe, expect, it } from "vitest";
import {
  behavioralBadge,
  behavioralScoreLine,
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
