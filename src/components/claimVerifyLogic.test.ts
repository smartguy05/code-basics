import { describe, expect, it } from "vitest";
import {
  behavioralReportToPromptContext,
  rulesRunHint,
  verifyClaimsAction,
} from "./claimVerifyLogic";
import type {
  BehavioralDelta,
  BehavioralReport,
  BehavioralScorecard,
  CardBehavior,
  RulesReport,
  RunConfig,
} from "../ipc/types";

function testDelta(fullName = "Suite.Case"): BehavioralDelta {
  return {
    kind: "test",
    fullName,
    base: "failed",
    work: "passed",
    transition: "fixed",
    filesHint: [],
  };
}

function httpDelta(): BehavioralDelta {
  return {
    kind: "http",
    name: "GET /orders",
    status: [500, 200],
    headerChanges: [],
    body: null,
    confidence: "high",
  };
}

function consoleDelta(): BehavioralDelta {
  return {
    kind: "console",
    addedLines: ["+ a", "+ b"],
    removedLines: ["- x"],
    normalized: true,
    confidence: "medium",
  };
}

const zeroScore: BehavioralScorecard = {
  outcomesCompared: 0,
  deltas: 0,
  attributedDeltas: 0,
  unattributedDeltas: 0,
  abstained: 0,
};

function config(id: string, name: string, kind: RunConfig["kind"]): RunConfig {
  return { id, name, kind, ecosystem: "dotnet", source: "detected" };
}

describe("behavioralReportToPromptContext", () => {
  it("renders the scorecard, attributed and unattributed deltas, and warnings", () => {
    const card: CardBehavior = {
      groupId: "card-1",
      deltas: [testDelta("Orders.Returns404"), httpDelta()],
      confidence: "high",
    };
    const report: BehavioralReport = {
      tests: {
        cases: [],
        summaryBefore: { total: 3, passed: 1, failed: 2, skipped: 0, other: 0 },
        summaryAfter: { total: 3, passed: 3, failed: 0, skipped: 0, other: 0 },
      },
      console: null,
      http: [],
      attributions: [card],
      unattributed: [consoleDelta()],
      scorecard: {
        outcomesCompared: 3,
        deltas: 3,
        attributedDeltas: 2,
        unattributedDeltas: 1,
        abstained: 0,
      },
      warnings: ["Server never became ready for GET /health"],
    };

    const text = behavioralReportToPromptContext(report);

    // Scorecard line is present.
    expect(text).toContain("3 outcomes compared");
    // Attributed section names the card and its deltas.
    expect(text).toContain("card-1");
    expect(text).toContain("Orders.Returns404");
    expect(text).toContain("500");
    expect(text).toContain("200");
    // Unattributed console delta.
    expect(text).toContain("Unattributed");
    expect(text).toContain("console");
    // Warnings surfaced verbatim.
    expect(text).toContain("Server never became ready for GET /health");
    // Test summary line present.
    expect(text).toContain("Tests:");
  });

  it("is deterministic across repeated calls on the same report", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [
        { groupId: "b", deltas: [testDelta("B")], confidence: "medium" },
        { groupId: "a", deltas: [httpDelta()], confidence: "low" },
      ],
      unattributed: [],
      scorecard: zeroScore,
      warnings: [],
    };
    expect(behavioralReportToPromptContext(report)).toBe(
      behavioralReportToPromptContext(report),
    );
  });

  it("renders deltas with absent outcomes and a status-less http response", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [
        {
          groupId: "c",
          deltas: [
            {
              kind: "test",
              fullName: "New.Case",
              base: null,
              work: null,
              transition: "added",
              filesHint: [],
            },
            {
              kind: "http",
              name: "POST /orders",
              status: null,
              headerChanges: [],
              body: null,
              confidence: "medium",
            },
          ],
          confidence: "low",
        },
      ],
      unattributed: [],
      scorecard: zeroScore,
      warnings: [],
    };
    const text = behavioralReportToPromptContext(report);
    expect(text).toContain("absent → absent");
    expect(text).toContain("response changed");
  });

  it("hands the agent the actual console lines, not just their counts", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [],
      unattributed: [consoleDelta()],
      scorecard: { ...zeroScore, deltas: 1, unattributedDeltas: 1 },
      warnings: [],
    };
    const text = behavioralReportToPromptContext(report);
    // The counts stay — they are the summary — but the evidence is now there too.
    expect(text).toContain("console: 2 added, 1 removed");
    expect(text).toContain("- - x");
    expect(text).toContain("+ + a");
    expect(text).toContain("+ + b");
    expect(text).toMatch(/masked before comparing/);
  });

  it("says why an unattributed delta was pinned to no card", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [],
      unattributed: [httpDelta()],
      scorecard: { ...zeroScore, deltas: 1, unattributedDeltas: 1 },
      warnings: [],
    };
    expect(behavioralReportToPromptContext(report)).toMatch(
      /handler is not derivable/,
    );
  });

  it("carries the http header and body detail the summary line drops", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [
        {
          groupId: "c",
          deltas: [
            {
              kind: "http",
              name: "GET /orders",
              status: null,
              headerChanges: [{ name: "x-total", before: "3", after: "4" }],
              body: { addedLines: ["4"], removedLines: ["3"], normalized: false },
              confidence: "medium",
            },
          ],
          confidence: "medium",
        },
      ],
      unattributed: [],
      scorecard: zeroScore,
      warnings: [],
    };
    const text = behavioralReportToPromptContext(report);
    expect(text).toContain("x-total: 3 → 4");
    expect(text).toContain("- 3");
    expect(text).toContain("+ 4");
  });

  it("notes a card that carries no deltas", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [{ groupId: "empty", deltas: [], confidence: "low" }],
      unattributed: [],
      scorecard: zeroScore,
      warnings: [],
    };
    expect(behavioralReportToPromptContext(report)).toContain("(no deltas)");
  });

  it("notes an empty report without throwing and omits empty sections", () => {
    const report: BehavioralReport = {
      tests: null,
      console: null,
      http: [],
      attributions: [],
      unattributed: [],
      scorecard: zeroScore,
      warnings: [],
    };
    const text = behavioralReportToPromptContext(report);
    expect(() => behavioralReportToPromptContext(report)).not.toThrow();
    expect(text).toContain("No observable before/after differences");
    expect(text).not.toContain("Attributed deltas:");
    expect(text).not.toContain("Warnings:");
  });
});

describe("rulesRunHint", () => {
  it("prompts to extract rules when there are none", () => {
    const report: RulesReport = { rules: [], warnings: [] };
    expect(rulesRunHint(report)).toMatch(/Extract Business Rules/i);
  });

  it("reports unparseable rule files when some are present", () => {
    const report: RulesReport = {
      rules: [{ id: "r1", title: "One", body: "..." }],
      warnings: ["rules/two.md: could not read"],
    };
    expect(rulesRunHint(report)).toMatch(/1 rule file/);
  });

  it("pluralises the warning count", () => {
    const report: RulesReport = {
      rules: [{ id: "r1", title: "One", body: "..." }],
      warnings: ["a", "b"],
    };
    expect(rulesRunHint(report)).toMatch(/2 rule files/);
  });

  it("returns null when there are rules and no warnings", () => {
    const report: RulesReport = {
      rules: [{ id: "r1", title: "One", body: "..." }],
      warnings: [],
    };
    expect(rulesRunHint(report)).toBeNull();
  });

  it("prefers the no-rules message over a warning note", () => {
    const report: RulesReport = { rules: [], warnings: ["broken.md"] };
    expect(rulesRunHint(report)).toMatch(/Extract Business Rules/i);
  });
});

describe("verifyClaimsAction", () => {
  it("is enabled and picks the test config when one exists", () => {
    const action = verifyClaimsAction([
      config("app", "App", "app"),
      config("tests", "Unit Tests", "test"),
    ]);
    expect(action.enabled).toBe(true);
    expect(action.config?.id).toBe("tests");
    expect(action.hint).toContain("Unit Tests");
    // Must distinguish it from "Run before/after": it adds an agent that judges.
    expect(action.hint.toLowerCase()).toContain("agent");
  });

  it("is disabled with a hint when there is no config", () => {
    const action = verifyClaimsAction([]);
    expect(action.enabled).toBe(false);
    expect(action.config).toBeNull();
    expect(action.hint.length).toBeGreaterThan(0);
  });
});
