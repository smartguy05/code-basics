import { describe, expect, it } from "vitest";
import {
  cardCoverage,
  coverageSummaryLine,
  hasCoverage,
  uncoveredIndices,
  uncoveredIndicesForPath,
} from "./coverageOfChangeLogic";
import type { ChangeCoverage, FileChangeCoverage } from "../ipc/types";

function fileCov(over: Partial<FileChangeCoverage> & { path: string }): FileChangeCoverage {
  return {
    path: over.path,
    uncovered: over.uncovered ?? [],
    coveredChanged: over.coveredChanged ?? 0,
    uncoveredChanged: over.uncoveredChanged ?? 0,
  };
}

function report(over: Partial<ChangeCoverage>): ChangeCoverage {
  return {
    files: over.files ?? [],
    changedLines: over.changedLines ?? 0,
    coveredLines: over.coveredLines ?? 0,
    uncoveredLines: over.uncoveredLines ?? 0,
    warnings: over.warnings ?? [],
  };
}

describe("hasCoverage", () => {
  it("is false for null/undefined", () => {
    expect(hasCoverage(null)).toBe(false);
    expect(hasCoverage(undefined)).toBe(false);
  });

  it("is false for the empty map returned before any coverage run", () => {
    expect(hasCoverage(report({ warnings: ["No coverage collected yet."] }))).toBe(false);
  });

  it("is true once there are files or classified changed lines", () => {
    expect(hasCoverage(report({ files: [fileCov({ path: "a.ts" })] }))).toBe(true);
    expect(hasCoverage(report({ changedLines: 3 }))).toBe(true);
  });
});

describe("coverageSummaryLine", () => {
  it("reads as the four-part tally with abstained = warnings.length", () => {
    const line = coverageSummaryLine(
      report({
        changedLines: 42,
        coveredLines: 35,
        uncoveredLines: 7,
        warnings: ["src/x.ts: no coverage matched"],
      }),
    );
    expect(line).toBe("42 changed lines · 35 covered · 7 uncovered · 1 abstained");
  });

  it("singularises one changed line", () => {
    const line = coverageSummaryLine(
      report({ changedLines: 1, coveredLines: 1, uncoveredLines: 0 }),
    );
    expect(line).toBe("1 changed line · 1 covered · 0 uncovered · 0 abstained");
  });

  it("reads sensibly at zero", () => {
    expect(coverageSummaryLine(report({}))).toBe(
      "0 changed lines · 0 covered · 0 uncovered · 0 abstained",
    );
  });
});

describe("uncoveredIndices", () => {
  it("is empty for null", () => {
    expect(uncoveredIndices(null)).toEqual([]);
  });

  it("collects every uncovered index across files", () => {
    const r = report({
      files: [
        fileCov({ path: "a.ts", uncovered: [{ line: 10, index: 4 }, { line: 11, index: 5 }] }),
        fileCov({ path: "b.ts", uncovered: [{ line: 2, index: 1 }] }),
      ],
    });
    expect(uncoveredIndices(r)).toEqual([4, 5, 1]);
  });
});

describe("uncoveredIndicesForPath", () => {
  const r = report({
    files: [
      fileCov({ path: "a.ts", uncovered: [{ line: 10, index: 4 }, { line: 11, index: 5 }] }),
      fileCov({ path: "b.ts", uncovered: [{ line: 2, index: 4 }] }),
    ],
  });

  it("returns only the named file's indices, not another file's colliding index", () => {
    // Both files carry index 4; the pane for a.ts must not borrow b.ts's.
    expect(uncoveredIndicesForPath(r, "a.ts")).toEqual([4, 5]);
    expect(uncoveredIndicesForPath(r, "b.ts")).toEqual([4]);
  });

  it("is empty for an unknown path, a null path, or a null report", () => {
    expect(uncoveredIndicesForPath(r, "missing.ts")).toEqual([]);
    expect(uncoveredIndicesForPath(r, null)).toEqual([]);
    expect(uncoveredIndicesForPath(null, "a.ts")).toEqual([]);
  });
});

describe("cardCoverage", () => {
  const r = report({
    files: [
      fileCov({ path: "a.ts", uncovered: [{ line: 10, index: 4 }, { line: 11, index: 5 }] }),
      fileCov({ path: "b.ts", uncovered: [{ line: 2, index: 9 }] }),
    ],
  });

  it("counts the card's own line indices that are uncovered", () => {
    expect(cardCoverage(new Set([4, 5, 6]), r)).toBe(2);
    expect(cardCoverage(new Set([9]), r)).toBe(1);
  });

  it("is zero when none of the card's lines are uncovered", () => {
    expect(cardCoverage(new Set([1, 2, 3]), r)).toBe(0);
  });

  it("is zero for a null report", () => {
    expect(cardCoverage(new Set([4, 5]), null)).toBe(0);
  });

  it("is zero for an empty card", () => {
    expect(cardCoverage(new Set(), r)).toBe(0);
  });
});
