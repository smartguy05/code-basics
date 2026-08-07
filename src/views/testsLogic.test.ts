import { describe, expect, it } from "vitest";
import { applyLiveOutcomes, classifyLine, liveOutcomeFor } from "./testsLogic";
import type { TestCase, TestNode, TestOutcome } from "../ipc/types";

function makeCase(fullName: string, name = fullName): TestCase {
  return {
    id: fullName,
    name,
    fullName,
    suite: null,
    project: null,
    outcome: "other",
    durationMs: null,
    message: null,
    stackTrace: null,
    stdout: null,
  };
}

function leaf(fullName: string, name = fullName): TestNode {
  return {
    id: `node:${fullName}`,
    label: name,
    outcome: "passed",
    summary: { total: 1, passed: 1, failed: 0, skipped: 0, other: 0 },
    durationMs: 42,
    case: makeCase(fullName, name),
    children: [],
  };
}

function suite(label: string, children: TestNode[]): TestNode {
  return {
    id: `suite:${label}`,
    label,
    outcome: "passed",
    summary: { total: 0, passed: 0, failed: 0, skipped: 0, other: 0 },
    durationMs: 99,
    case: null,
    children,
  };
}

function results(entries: [string, TestOutcome][]): Map<string, TestOutcome> {
  return new Map(entries);
}

describe("classifyLine", () => {
  it("reads VSTest's `Passed Name [1 ms]`, dropping the duration", () => {
    expect(classifyLine("Passed MyNamespace.MyTest [1 ms]")).toEqual({
      outcome: "passed",
      name: "MyNamespace.MyTest",
    });
  });

  it("reads VSTest's `Failed Name [12.5 s]`", () => {
    expect(classifyLine("  Failed MyNamespace.Broken [12.5 s]")).toEqual({
      outcome: "failed",
      name: "MyNamespace.Broken",
    });
  });

  it("reads VSTest's `Skipped Name`", () => {
    expect(classifyLine("Skipped MyNamespace.Ignored")).toEqual({
      outcome: "skipped",
      name: "MyNamespace.Ignored",
    });
  });

  it("reads MTP's lowercase `failed Name`", () => {
    expect(classifyLine("failed Some.Test.Case")).toEqual({
      outcome: "failed",
      name: "Some.Test.Case",
    });
  });

  it("reads Vitest's tick with a `file > name` path and a bare duration", () => {
    expect(classifyLine("  ✓ src/foo.test.ts > adds numbers 3ms")).toEqual({
      outcome: "passed",
      name: "src/foo.test.ts > adds numbers",
    });
  });

  it("reads Vitest's cross as a failure", () => {
    expect(classifyLine("  × src/foo.test.ts > breaks")).toEqual({
      outcome: "failed",
      name: "src/foo.test.ts > breaks",
    });
  });

  it("reads Vitest's down-arrow as a skip", () => {
    expect(classifyLine("  ↓ src/foo.test.ts > pending")).toEqual({
      outcome: "skipped",
      name: "src/foo.test.ts > pending",
    });
  });

  it("reads Jest's per-file `PASS path` and `FAIL path`", () => {
    expect(classifyLine("PASS src/foo.test.ts")).toEqual({
      outcome: "passed",
      name: "src/foo.test.ts",
    });
    expect(classifyLine("FAIL src/bar.test.ts")).toEqual({
      outcome: "failed",
      name: "src/bar.test.ts",
    });
  });

  it("drops a parenthesised duration", () => {
    expect(classifyLine("✓ renders (12ms)")).toEqual({
      outcome: "passed",
      name: "renders",
    });
  });

  it("ignores VSTest's summary line, because `!` blocks the required space", () => {
    expect(classifyLine("Passed!  - Failed: 0, Passed: 5, Skipped: 0")).toBeNull();
  });

  it("ignores unrelated output, blank lines and a bare marker", () => {
    expect(classifyLine("Build succeeded.")).toBeNull();
    expect(classifyLine("")).toBeNull();
    expect(classifyLine("   ")).toBeNull();
    expect(classifyLine("passed")).toBeNull();
  });
});

describe("liveOutcomeFor", () => {
  it("prefers an exact fullName match", () => {
    const outcome = liveOutcomeFor(
      makeCase("Ns.Class.Test", "Test"),
      results([
        ["Ns.Class.Test", "failed"],
        ["Test", "passed"],
      ]),
    );
    expect(outcome).toBe("failed");
  });

  it("falls back to the short name when the fullName is absent", () => {
    const outcome = liveOutcomeFor(
      makeCase("Ns.Class.Test", "Test"),
      results([["Test", "skipped"]]),
    );
    expect(outcome).toBe("skipped");
  });

  it("matches when the reported name ends with the case's fullName", () => {
    const outcome = liveOutcomeFor(
      makeCase("Class.Test"),
      results([["Assembly.Ns.Class.Test", "failed"]]),
    );
    expect(outcome).toBe("failed");
  });

  it("matches when the case's fullName ends with the reported name", () => {
    const outcome = liveOutcomeFor(
      makeCase("Assembly.Ns.Class.Test"),
      results([["Class.Test", "passed"]]),
    );
    expect(outcome).toBe("passed");
  });

  it("returns `other` when nothing matches, including an empty map", () => {
    expect(liveOutcomeFor(makeCase("Foo.Bar"), results([]))).toBe("other");
    expect(
      liveOutcomeFor(makeCase("Foo.Bar"), results([["Completely.Other", "passed"]])),
    ).toBe("other");
  });

  it("takes the first suffix match in insertion order", () => {
    const outcome = liveOutcomeFor(
      makeCase("Class.Test"),
      results([
        ["A.Class.Test", "skipped"],
        ["B.Class.Test", "failed"],
      ]),
    );
    expect(outcome).toBe("skipped");
  });
});

describe("applyLiveOutcomes", () => {
  it("recolours a leaf and rewrites its one-test summary", () => {
    const node = applyLiveOutcomes(leaf("Ns.A"), results([["Ns.A", "failed"]]));
    expect(node.outcome).toBe("failed");
    expect(node.durationMs).toBeNull();
    expect(node.summary).toEqual({
      total: 1,
      passed: 0,
      failed: 1,
      skipped: 0,
      other: 0,
    });
  });

  it("marks an unreported leaf `other` and clears its duration", () => {
    const node = applyLiveOutcomes(leaf("Ns.A"), results([]));
    expect(node.outcome).toBe("other");
    expect(node.durationMs).toBeNull();
    expect(node.summary).toEqual({
      total: 1,
      passed: 0,
      failed: 0,
      skipped: 0,
      other: 1,
    });
  });

  it("rolls nested suites up, with any failure winning", () => {
    const tree = suite("root", [
      suite("inner", [leaf("Ns.A"), leaf("Ns.B")]),
      leaf("Ns.C"),
    ]);
    const node = applyLiveOutcomes(
      tree,
      results([
        ["Ns.A", "passed"],
        ["Ns.B", "failed"],
        ["Ns.C", "passed"],
      ]),
    );
    expect(node.summary).toEqual({
      total: 3,
      passed: 2,
      failed: 1,
      skipped: 0,
      other: 0,
    });
    expect(node.outcome).toBe("failed");
    expect(node.children[0]?.outcome).toBe("failed");
    expect(node.children[1]?.outcome).toBe("passed");
  });

  it("reports `other` while something below has not reported yet", () => {
    const tree = suite("root", [leaf("Ns.A"), leaf("Ns.B")]);
    const node = applyLiveOutcomes(tree, results([["Ns.A", "passed"]]));
    expect(node.outcome).toBe("other");
    expect(node.summary).toEqual({
      total: 2,
      passed: 1,
      failed: 0,
      skipped: 0,
      other: 1,
    });
  });

  it("reports `skipped` when every leaf below was skipped", () => {
    const tree = suite("root", [leaf("Ns.A"), leaf("Ns.B")]);
    const node = applyLiveOutcomes(
      tree,
      results([
        ["Ns.A", "skipped"],
        ["Ns.B", "skipped"],
      ]),
    );
    expect(node.outcome).toBe("skipped");
  });

  it("treats a childless suite as an empty, skipped tree", () => {
    const node = applyLiveOutcomes(suite("root", []), results([]));
    expect(node.children).toEqual([]);
    expect(node.outcome).toBe("skipped");
    expect(node.summary).toEqual({
      total: 0,
      passed: 0,
      failed: 0,
      skipped: 0,
      other: 0,
    });
  });

  it("does not mutate the input tree", () => {
    const tree = suite("root", [leaf("Ns.A")]);
    applyLiveOutcomes(tree, results([["Ns.A", "failed"]]));
    expect(tree.outcome).toBe("passed");
    expect(tree.children[0]?.durationMs).toBe(42);
  });
});
