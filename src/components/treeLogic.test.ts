import { describe, expect, it } from "vitest";
import type {
  Branch,
  InspectGraph,
  InspectNode,
  TestCase,
  TestNode,
  TestOutcome,
} from "../ipc/types";
import {
  ancestorPaths,
  buildTree,
  countLabel,
  formatDuration,
  objectMatches,
  searchableValue,
  targetLabel,
  testMatches,
} from "./treeLogic";

function branch(name: string, extra: Partial<Branch> = {}): Branch {
  return { name, isHead: false, isRemote: false, upstream: null, ...extra };
}

function node(partial: Partial<InspectNode> & { id: string }): InspectNode {
  return {
    label: partial.id,
    value: { kind: "null" },
    children: [],
    hasMore: false,
    ...partial,
  };
}

function testNode(partial: Partial<TestNode> & { id: string }): TestNode {
  return {
    label: partial.id,
    outcome: "passed",
    summary: { total: 1, passed: 1, failed: 0, skipped: 0, other: 0 },
    durationMs: null,
    case: null,
    children: [],
    ...partial,
  };
}

function testCase(fullName: string): TestCase {
  return {
    id: fullName,
    name: fullName,
    fullName,
    suite: null,
    project: null,
    outcome: "passed",
    durationMs: null,
    message: null,
    stackTrace: null,
    stdout: null,
  };
}

// --- buildTree -------------------------------------------------------------

describe("buildTree", () => {
  it("puts flat branch names directly on the root", () => {
    const root = buildTree([branch("main"), branch("dev")]);
    expect(root.folders).toEqual([]);
    expect(root.leaves.map((leaf) => leaf.label)).toEqual(["main", "dev"]);
    expect(root.path).toBe("");
  });

  it("splits slash-separated names into nested folders", () => {
    const root = buildTree([branch("users/anthony/thing")]);
    const users = root.folders[0]!;
    expect(users.path).toBe("users");
    expect(users.label).toBe("users");
    const anthony = users.folders[0]!;
    expect(anthony.path).toBe("users/anthony");
    expect(anthony.label).toBe("anthony");
    expect(anthony.leaves).toHaveLength(1);
    expect(anthony.leaves[0]!.label).toBe("thing");
    expect(anthony.leaves[0]!.branch.name).toBe("users/anthony/thing");
  });

  it("reuses a folder shared by several branches", () => {
    const root = buildTree([
      branch("feature/a"),
      branch("feature/b"),
      branch("fix/c"),
    ]);
    expect(root.folders.map((folder) => folder.path)).toEqual([
      "feature",
      "fix",
    ]);
    expect(root.folders[0]!.leaves.map((leaf) => leaf.label)).toEqual([
      "a",
      "b",
    ]);
  });

  it("mixes folders and leaves at the same level", () => {
    const root = buildTree([branch("main"), branch("feature/a")]);
    expect(root.leaves.map((leaf) => leaf.label)).toEqual(["main"]);
    expect(root.folders.map((folder) => folder.label)).toEqual(["feature"]);
  });

  it("returns an empty root for no branches", () => {
    const root = buildTree([]);
    expect(root).toEqual({ path: "", label: "", folders: [], leaves: [] });
  });

  it("treats a trailing slash as an empty leaf label", () => {
    const root = buildTree([branch("wip/")]);
    expect(root.folders[0]!.path).toBe("wip");
    expect(root.folders[0]!.leaves[0]!.label).toBe("");
  });
});

// --- ancestorPaths ---------------------------------------------------------

describe("ancestorPaths", () => {
  it("returns nothing for an unnested name", () => {
    expect(ancestorPaths("main")).toEqual([]);
  });

  it("returns the chain of enclosing folders", () => {
    expect(ancestorPaths("users/anthony/thing")).toEqual([
      "users",
      "users/anthony",
    ]);
  });

  it("stops one short of the leaf segment", () => {
    expect(ancestorPaths("a/b")).toEqual(["a"]);
  });
});

// --- searchableValue -------------------------------------------------------

describe("searchableValue", () => {
  it("uses the text of a primitive", () => {
    expect(searchableValue({ kind: "primitive", text: "42" })).toBe("42");
  });

  it("uses the text of a string, truncated or not", () => {
    expect(searchableValue({ kind: "text", text: "hello", truncated: true })).toBe(
      "hello",
    );
  });

  it("joins a reference's type name and address", () => {
    expect(
      searchableValue({
        kind: "reference",
        typeName: "Order",
        address: "0x1234",
        expandable: true,
      }),
    ).toBe("Order 0x1234");
  });

  it("uses the path of a cycle and the reason of an unavailable", () => {
    expect(searchableValue({ kind: "cycle", address: "0x1", path: "root.a" })).toBe(
      "root.a",
    );
    expect(searchableValue({ kind: "unavailable", reason: "no memory" })).toBe(
      "no memory",
    );
  });

  it("has no searchable text for null or elided", () => {
    expect(searchableValue({ kind: "null" })).toBe("");
    expect(searchableValue({ kind: "elided", reason: "depthLimit" })).toBe("");
  });
});

// --- objectMatches ---------------------------------------------------------

describe("objectMatches", () => {
  it("matches everything on an empty filter", () => {
    expect(objectMatches(node({ id: "root" }), "")).toBe(true);
  });

  it("matches on the label", () => {
    expect(objectMatches(node({ id: "x", label: "Total" }), "tot")).toBe(true);
  });

  it("matches on the type name", () => {
    expect(
      objectMatches(node({ id: "x", label: "a", typeName: "Order" }), "ord"),
    ).toBe(true);
  });

  it("matches on the value text", () => {
    expect(
      objectMatches(
        node({ id: "x", label: "a", value: { kind: "primitive", text: "1234" } }),
        "123",
      ),
    ).toBe(true);
  });

  it("keeps a parent whose descendant matches", () => {
    const tree = node({
      id: "root",
      label: "root",
      children: [
        node({ id: "a", label: "a", children: [node({ id: "b", label: "needle" })] }),
      ],
    });
    expect(objectMatches(tree, "needle")).toBe(true);
  });

  it("rejects a node when nothing in its subtree matches", () => {
    const tree = node({
      id: "root",
      label: "root",
      children: [node({ id: "a", label: "a" })],
    });
    expect(objectMatches(tree, "zzz")).toBe(false);
  });

  it("only sees a lowercased needle (the caller lowercases the filter)", () => {
    const n = node({ id: "x", label: "Total" });
    expect(objectMatches(n, "total")).toBe(true);
    expect(objectMatches(n, "Total")).toBe(false);
  });
});

// --- countLabel ------------------------------------------------------------

describe("countLabel", () => {
  it("says nothing when everything was read", () => {
    expect(countLabel(node({ id: "x", children: [node({ id: "c" })] }))).toBeNull();
  });

  it("reports a counted total with thousands separators", () => {
    expect(
      countLabel(
        node({ id: "x", children: [node({ id: "c" })], childCountTotal: 5412 }),
      ),
    ).toBe(`showing ${(1).toLocaleString()} of ${(5412).toLocaleString()}`);
  });

  it("ignores a total that is not larger than what is shown", () => {
    expect(
      countLabel(node({ id: "x", children: [node({ id: "c" })], childCountTotal: 1 })),
    ).toBeNull();
  });

  it("admits an unknown remainder when hasMore is set", () => {
    expect(countLabel(node({ id: "x", children: [], hasMore: true }))).toBe(
      `showing ${(0).toLocaleString()}, more not read`,
    );
  });

  it("prefers the counted total over the vaguer hasMore wording", () => {
    const label = countLabel(
      node({ id: "x", children: [], hasMore: true, childCountTotal: 3 }),
    );
    expect(label).toContain("of");
    expect(label).not.toContain("more not read");
  });
});

// --- targetLabel -----------------------------------------------------------

function graph(target: InspectGraph["target"]): InspectGraph {
  return {
    sessionId: "s",
    snapshotId: "snap",
    capturedAt: "2026-01-01T00:00:00Z",
    target,
    roots: [],
    caps: { maxDepth: 4, maxChildren: 100, maxStringLength: 200, maxNodes: 5000 },
  };
}

describe("targetLabel", () => {
  it("uses the path for a dump", () => {
    expect(targetLabel(graph({ target: { kind: "dump", path: "C:/a.dmp" } }))).toBe(
      "C:/a.dmp",
    );
  });

  it("names a live process with its pid", () => {
    expect(
      targetLabel(
        graph({ target: { kind: "live", pid: 42 }, processName: "api.exe" }),
      ),
    ).toBe("api.exe (pid 42)");
  });

  it("falls back to the pid alone when the name is unknown", () => {
    expect(targetLabel(graph({ target: { kind: "live", pid: 42 } }))).toBe("pid 42");
  });
});

// --- formatDuration --------------------------------------------------------

describe("formatDuration", () => {
  it("is empty for an unknown duration", () => {
    expect(formatDuration(null)).toBe("");
  });

  it("rounds sub-second durations to whole milliseconds", () => {
    expect(formatDuration(0)).toBe("0ms");
    expect(formatDuration(12.4)).toBe("12ms");
    expect(formatDuration(12.5)).toBe("13ms");
    expect(formatDuration(999)).toBe("999ms");
  });

  it("switches to seconds at exactly 1000ms", () => {
    expect(formatDuration(1000)).toBe("1.00s");
    expect(formatDuration(1500)).toBe("1.50s");
  });

  it("keeps two decimals for long runs, with no minute unit", () => {
    expect(formatDuration(65_000)).toBe("65.00s");
    expect(formatDuration(3_600_000)).toBe("3600.00s");
  });
});

// --- testMatches -----------------------------------------------------------

describe("testMatches", () => {
  const none = new Set<TestOutcome>();

  it("matches a leaf with no filters at all", () => {
    expect(testMatches(testNode({ id: "a" }), "", none)).toBe(true);
  });

  it("matches a leaf on its label", () => {
    expect(testMatches(testNode({ id: "a", label: "Adds" }), "add", none)).toBe(true);
  });

  it("matches a leaf on the case full name", () => {
    const leaf = testNode({ id: "a", label: "Adds", case: testCase("Ns.Cls.adds") });
    expect(testMatches(leaf, "ns.cls", none)).toBe(true);
  });

  it("requires both the outcome and the text on a leaf", () => {
    const leaf = testNode({ id: "a", label: "Adds", outcome: "failed" });
    expect(testMatches(leaf, "adds", new Set<TestOutcome>(["failed"]))).toBe(true);
    expect(testMatches(leaf, "adds", new Set<TestOutcome>(["passed"]))).toBe(false);
    expect(testMatches(leaf, "zzz", new Set<TestOutcome>(["failed"]))).toBe(false);
  });

  it("keeps a branch when any descendant matches", () => {
    const tree = testNode({
      id: "suite",
      label: "Suite",
      children: [
        testNode({ id: "a", label: "alpha" }),
        testNode({ id: "b", label: "beta" }),
      ],
    });
    expect(testMatches(tree, "beta", none)).toBe(true);
    expect(testMatches(tree, "gamma", none)).toBe(false);
  });

  it("judges a branch by its children, not its own label", () => {
    const tree = testNode({
      id: "suite",
      label: "needle",
      children: [testNode({ id: "a", label: "alpha" })],
    });
    expect(testMatches(tree, "needle", none)).toBe(false);
  });

  it("filters a branch by descendant outcomes", () => {
    const tree = testNode({
      id: "suite",
      label: "Suite",
      children: [
        testNode({ id: "a", label: "alpha", outcome: "passed" }),
        testNode({ id: "b", label: "beta", outcome: "failed" }),
      ],
    });
    expect(testMatches(tree, "", new Set<TestOutcome>(["failed"]))).toBe(true);
    expect(testMatches(tree, "", new Set<TestOutcome>(["skipped"]))).toBe(false);
  });
});
