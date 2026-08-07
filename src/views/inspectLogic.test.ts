import { describe, expect, it } from "vitest";
import {
  byConfigThenAttribution,
  byNameThenPid,
  couldHaveMoved,
  formatBytes,
  formatCaptured,
  preferApplicationProcess,
  readsAsTargetGone,
  rebase,
  selectorValue,
  setupSnippet,
  spliceInto,
} from "./inspectLogic";
import type {
  AttachableProcess,
  InspectGraph,
  InspectNode,
  InspectTarget,
} from "../ipc/types";

function proc(over: Partial<AttachableProcess> = {}): AttachableProcess {
  return {
    pid: 1,
    name: "App",
    attribution: "launched",
    isApplication: false,
    ...over,
  };
}

function node(over: Partial<InspectNode> = {}): InspectNode {
  return {
    id: "root",
    label: "root",
    value: { kind: "null" },
    children: [],
    hasMore: false,
    ...over,
  };
}

function graph(target: InspectTarget, snapshotId: string): InspectGraph {
  return {
    sessionId: "s",
    snapshotId,
    capturedAt: "2026-01-01T00:00:00Z",
    target: { target },
    roots: [],
    caps: { maxDepth: 5, maxChildren: 10, maxStringLength: 100, maxNodes: 100 },
  };
}

describe("formatBytes", () => {
  it("reports whole bytes below one kilobyte", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("switches to kilobytes at exactly 1024", () => {
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("2 KB"); // toFixed(0) rounds
  });

  it("switches to megabytes at exactly one megabyte", () => {
    expect(formatBytes(1024 * 1024)).toBe("1.0 MB");
    expect(formatBytes(1024 * 1024 - 1)).toBe("1024 KB");
    expect(formatBytes(9 * 1024 * 1024)).toBe("9.0 MB");
  });

  it("switches to gigabytes at exactly one gigabyte", () => {
    expect(formatBytes(1024 * 1024 * 1024)).toBe("1.00 GB");
    expect(formatBytes(1024 * 1024 * 1024 - 1024 * 1024)).toBe("1023.0 MB");
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe("3.00 GB");
  });
});

describe("formatCaptured", () => {
  it("treats the number as unix seconds, not milliseconds", () => {
    const seconds = 1_700_000_000;
    expect(formatCaptured(seconds)).toBe(new Date(seconds * 1000).toLocaleString());
  });

  it("formats the epoch itself", () => {
    expect(formatCaptured(0)).toBe(new Date(0).toLocaleString());
  });
});

describe("rebase", () => {
  it("rewrites ids that start with the old root", () => {
    const fresh = node({
      id: "root",
      children: [node({ id: "root._total" })],
    });
    const out = rebase(fresh, "root", "a.b");
    expect(out.id).toBe("a.b");
    expect(out.children.map((c) => c.id)).toEqual(["a.b._total"]);
  });

  it("leaves ids outside the subtree alone", () => {
    const out = rebase(node({ id: "elsewhere.x" }), "root", "a.b");
    expect(out.id).toBe("elsewhere.x");
  });

  it("rewrites a cycle path inside the subtree", () => {
    const out = rebase(
      node({
        id: "root.next",
        value: { kind: "cycle", address: "0x1", path: "root" },
      }),
      "root",
      "a.b",
    );
    expect(out.value).toEqual({ kind: "cycle", address: "0x1", path: "a.b" });
  });

  it("leaves a cycle pointing outside the subtree exactly as read", () => {
    const value = { kind: "cycle" as const, address: "0x1", path: "other.thing" };
    const out = rebase(node({ id: "root", value }), "root", "a.b");
    expect(out.value).toEqual(value);
  });

  it("does not mutate the input node", () => {
    const input = node({ id: "root", children: [node({ id: "root.a" })] });
    rebase(input, "root", "z");
    expect(input.id).toBe("root");
    expect(input.children.map((c) => c.id)).toEqual(["root.a"]);
  });
});

describe("spliceInto", () => {
  it("returns an empty list unchanged", () => {
    expect(spliceInto([], "anything", node())).toEqual([]);
  });

  it("returns the tree unchanged when the target id is not present", () => {
    const nodes = [node({ id: "a", children: [node({ id: "a.b" })] })];
    const out = spliceInto(nodes, "missing", node({ id: "root" }));
    expect(out).toEqual(nodes);
  });

  it("replaces the children of the target with the rebased fresh node", () => {
    const nodes = [node({ id: "a", children: [node({ id: "a.b" })] })];
    const fresh = node({
      id: "root",
      value: { kind: "primitive", text: "7" },
      children: [node({ id: "root.x" })],
      hasMore: true,
      childCountTotal: 42,
    });

    const [spliced] = spliceInto(nodes, "a", fresh);
    expect(spliced?.id).toBe("a");
    expect(spliced?.children.map((c) => c.id)).toEqual(["a.x"]);
    expect(spliced?.hasMore).toBe(true);
    expect(spliced?.childCountTotal).toBe(42);
    expect(spliced?.value).toEqual({ kind: "primitive", text: "7" });
  });

  it("finds a target nested several levels down", () => {
    const nodes = [
      node({
        id: "a",
        children: [node({ id: "a.b", children: [node({ id: "a.b.c" })] })],
      }),
    ];
    const out = spliceInto(
      nodes,
      "a.b.c",
      node({ id: "r", children: [node({ id: "r.d" })] }),
    );
    const deep = out[0]?.children[0]?.children[0];
    expect(deep?.children.map((c) => c.id)).toEqual(["a.b.c.d"]);
  });

  it("leaves siblings of the target untouched", () => {
    const nodes = [node({ id: "a" }), node({ id: "b", children: [node({ id: "b.x" })] })];
    const out = spliceInto(nodes, "a", node({ id: "r" }));
    expect(out[1]).toEqual(nodes[1]);
  });
});

describe("selectorValue", () => {
  it("escapes backslashes and quotes for an attribute selector", () => {
    expect(selectorValue(`c:\\dir`)).toBe(`c:\\\\dir`);
    expect(selectorValue(`say "hi"`)).toBe(`say \\"hi\\"`);
  });

  it("leaves an ordinary path-shaped id alone", () => {
    expect(selectorValue("root.orders[3]._total")).toBe("root.orders[3]._total");
  });
});

describe("byConfigThenAttribution", () => {
  it("orders by configuration name first", () => {
    const a = proc({ configName: "Api", pid: 9 });
    const b = proc({ configName: "Worker", pid: 1 });
    expect([b, a].sort(byConfigThenAttribution).map((p) => p.pid)).toEqual([9, 1]);
  });

  it("puts the evidenced application ahead of the rest of its configuration", () => {
    const app = proc({ configName: "Api", pid: 5, isApplication: true });
    const other = proc({
      configName: "Api",
      pid: 2,
      attribution: "descendant",
      isApplication: false,
    });
    expect([other, app].sort(byConfigThenAttribution).map((p) => p.pid)).toEqual([
      5, 2,
    ]);
  });

  it("ranks descendant above launched when neither is the application", () => {
    const launched = proc({ configName: "Api", pid: 1, attribution: "launched" });
    const descendant = proc({ configName: "Api", pid: 9, attribution: "descendant" });
    expect(
      [launched, descendant].sort(byConfigThenAttribution).map((p) => p.pid),
    ).toEqual([9, 1]);
  });

  it("falls back to pid for otherwise identical rows", () => {
    const rows = [proc({ pid: 30 }), proc({ pid: 10 }), proc({ pid: 20 })];
    expect(rows.sort(byConfigThenAttribution).map((p) => p.pid)).toEqual([10, 20, 30]);
  });

  it("treats a missing configuration name as an empty one", () => {
    const named = proc({ configName: "Api", pid: 1 });
    const unnamed = proc({ pid: 2 });
    expect([named, unnamed].sort(byConfigThenAttribution).map((p) => p.pid)).toEqual([
      2, 1,
    ]);
  });
});

describe("byNameThenPid", () => {
  it("orders by name", () => {
    const rows = [proc({ name: "zed", pid: 1 }), proc({ name: "alpha", pid: 2 })];
    expect(rows.sort(byNameThenPid).map((p) => p.name)).toEqual(["alpha", "zed"]);
  });

  it("tells same-named processes apart by pid", () => {
    const rows = [proc({ name: "App", pid: 30 }), proc({ name: "App", pid: 4 })];
    expect(rows.sort(byNameThenPid).map((p) => p.pid)).toEqual([4, 30]);
  });
});

describe("preferApplicationProcess", () => {
  it("returns null for an empty list", () => {
    expect(preferApplicationProcess([])).toBeNull();
  });

  it("picks the evidenced application over a launcher", () => {
    const launcher = proc({
      pid: 1,
      name: "dotnet",
      attribution: "launched",
      launcherCaveat: "this is the CLI",
    });
    const app = proc({ pid: 2, attribution: "descendant", isApplication: true });
    expect(preferApplicationProcess([launcher, app])?.pid).toBe(2);
  });

  it("falls back to a launcher that carries its caveat", () => {
    const launcher = proc({
      pid: 1,
      attribution: "launched",
      launcherCaveat: "this is the CLI",
    });
    const worker = proc({ pid: 3, attribution: "descendant" });
    expect(preferApplicationProcess([worker, launcher])?.pid).toBe(1);
  });

  it("returns null when nothing has evidence and nothing carries a caveat", () => {
    const worker = proc({ pid: 3, attribution: "descendant" });
    expect(preferApplicationProcess([worker])).toBeNull();
  });

  it("never returns an unrelated process, even one marked as an application", () => {
    const stranger = proc({ pid: 7, attribution: "unrelated", isApplication: true });
    expect(preferApplicationProcess([stranger])).toBeNull();
  });
});

describe("readsAsTargetGone", () => {
  it("matches the messages a vanished target produces", () => {
    for (const message of [
      "Process 1234 is no longer running",
      "target not running",
      "the process exited before it could be read",
      "No such process",
      "the pid does not exist",
      "Could not attach to 42",
      "attach failed",
    ]) {
      expect(readsAsTargetGone(message)).toBe(true);
    }
  });

  it("is case-insensitive", () => {
    expect(readsAsTargetGone("ATTACH FAILED")).toBe(true);
  });

  it("does not match an unrelated failure", () => {
    expect(readsAsTargetGone("the inspector sidecar is missing")).toBe(false);
    expect(readsAsTargetGone("")).toBe(false);
  });
});

describe("couldHaveMoved", () => {
  it("is false for two reads of the same dump, whatever the snapshot ids", () => {
    const dump: InspectTarget = { kind: "dump", path: "c:\\d.dmp" };
    expect(couldHaveMoved(graph(dump, "one"), graph(dump, "two"))).toBe(false);
  });

  it("is true when the dump path differs", () => {
    expect(
      couldHaveMoved(
        graph({ kind: "dump", path: "a.dmp" }, "s"),
        graph({ kind: "dump", path: "b.dmp" }, "s"),
      ),
    ).toBe(true);
  });

  it("is true when the kind of target changed", () => {
    expect(
      couldHaveMoved(
        graph({ kind: "dump", path: "a.dmp" }, "s"),
        graph({ kind: "live", pid: 1 }, "s"),
      ),
    ).toBe(true);
  });

  it("compares snapshot ids for a live target", () => {
    const live: InspectTarget = { kind: "live", pid: 1 };
    expect(couldHaveMoved(graph(live, "one"), graph(live, "two"))).toBe(true);
    expect(couldHaveMoved(graph(live, "one"), graph(live, "one"))).toBe(false);
  });
});

describe("setupSnippet", () => {
  it("points the dump directory at the workspace's .code-basics/dumps", () => {
    expect(setupSnippet("C:\\repo")).toContain(
      'var dir = @"C:\\repo\\.code-basics\\dumps";',
    );
  });

  it("strips trailing separators of either kind", () => {
    expect(setupSnippet("C:\\repo\\\\")).toContain('@"C:\\repo\\.code-basics\\dumps"');
    expect(setupSnippet("/home/me/repo/")).toContain(
      '@"/home/me/repo\\.code-basics\\dumps"',
    );
  });

  it("doubles a quote so the C# verbatim literal stays valid", () => {
    expect(setupSnippet('C:\\od"d')).toContain('@"C:\\od""d\\.code-basics\\dumps"');
  });

  it("writes a dump named the way the tab's parser expects", () => {
    const snippet = setupSnippet("C:\\repo");
    expect(snippet).toContain(
      'var name = $"{exe}_{Environment.ProcessId}_{stamp}.dmp";',
    );
    expect(snippet).toContain("DumpType.WithHeap");
    expect(snippet).toContain("Microsoft.Diagnostics.NETCore.Client");
    expect(snippet).toContain("throw;");
  });
});
