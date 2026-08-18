import { describe, expect, it } from "vitest";
import { badgeCount, categoryLabel, groupByCategory, CATEGORY_ORDER } from "./erosionLogic";
import type { ErosionCategory, ErosionFlag, ErosionReport } from "../ipc/types";

function flag(over: Partial<ErosionFlag> & { category: ErosionCategory }): ErosionFlag {
  return {
    path: "a.rs",
    line: 1,
    index: 0,
    origin: "addition",
    ruleId: "r",
    message: "m",
    content: "x",
    ...over,
  };
}

describe("categoryLabel", () => {
  it("gives a human heading for every category", () => {
    for (const category of CATEGORY_ORDER) {
      expect(categoryLabel(category)).toBeTruthy();
    }
  });
});

describe("groupByCategory", () => {
  it("groups flags under their category in the fixed order", () => {
    const flags = [
      flag({ category: "droppedLog" }),
      flag({ category: "deletedAssertion" }),
      flag({ category: "deletedAssertion" }),
    ];

    const sections = groupByCategory(flags);

    // deletedAssertion sorts before droppedLog regardless of input order.
    expect(sections.map((s) => s.category)).toEqual(["deletedAssertion", "droppedLog"]);
    expect(sections[0]!.flags).toHaveLength(2);
    expect(sections[1]!.flags).toHaveLength(1);
  });

  it("drops categories with no flags", () => {
    const sections = groupByCategory([flag({ category: "unsafeCast" })]);
    expect(sections).toHaveLength(1);
    expect(sections[0]!.category).toBe("unsafeCast");
  });

  it("preserves scan order within a section", () => {
    const flags = [
      flag({ category: "unsafeCast", path: "a.rs", line: 5 }),
      flag({ category: "unsafeCast", path: "a.rs", line: 2 }),
    ];
    const [section] = groupByCategory(flags);
    expect(section!.flags.map((f) => f.line)).toEqual([5, 2]);
  });

  it("is empty for no flags", () => {
    expect(groupByCategory([])).toEqual([]);
  });
});

describe("badgeCount", () => {
  function report(flags: ErosionFlag[]): ErosionReport {
    return { flags, warnings: [] };
  }

  it("counts the flags", () => {
    expect(badgeCount(report([flag({ category: "unsafeCast" }), flag({ category: "droppedLog" })]))).toBe(2);
  });

  it("is zero for a null report", () => {
    expect(badgeCount(null)).toBe(0);
  });

  it("is zero for an empty report", () => {
    expect(badgeCount(report([]))).toBe(0);
  });
});
