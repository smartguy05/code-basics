import { describe, expect, it } from "vitest";
import type { ArchKind, EdgeKind } from "../../ipc/types";
import { categoryOf, edgeStyle, legendEntries } from "./graphStyleLogic";

const NON_PROJECT_KINDS: ArchKind[] = [
  "solution",
  "solutionFolder",
  "external",
  "service",
  "dataStore",
];

const ALL_EDGE_KINDS: EdgeKind[] = [
  "projectReference",
  "packageDependency",
  "contains",
  "dataAccess",
  "serviceCall",
];

describe("categoryOf", () => {
  it("answers a colour variable and a label for every kind", () => {
    const nodes = [
      { kind: "project" as const, ecosystem: "dotnet" },
      ...NON_PROJECT_KINDS.map((kind) => ({ kind })),
    ];
    for (const node of nodes) {
      const category = categoryOf(node);
      expect(category.label).not.toBe("");
      expect(category.colorVar.startsWith("--")).toBe(true);
    }
  });

  it("colours a project by its ecosystem", () => {
    const dotnet = categoryOf({ kind: "project", ecosystem: "dotnet" });
    const node = categoryOf({ kind: "project", ecosystem: "node" });
    const cargo = categoryOf({ kind: "project", ecosystem: "cargo" });
    expect(dotnet.label).toBe(".NET");
    expect(node.label).toBe("Node");
    expect(cargo.label).toBe("Rust");
    // Each ecosystem is a distinct colour, or the point is lost.
    const vars = new Set([dotnet.colorVar, node.colorVar, cargo.colorVar]);
    expect(vars.size).toBe(3);
  });

  it("falls back to a plain Project for an unknown or missing ecosystem", () => {
    expect(categoryOf({ kind: "project", ecosystem: "python" }).label).toBe("Project");
    expect(categoryOf({ kind: "project", ecosystem: null }).label).toBe("Project");
    expect(categoryOf({ kind: "project" }).label).toBe("Project");
  });

  it("colours a service by role, not ecosystem", () => {
    const csharp = categoryOf({ kind: "service", ecosystem: "dotnet" });
    const js = categoryOf({ kind: "service", ecosystem: "node" });
    expect(csharp).toEqual(js);
    expect(csharp.label).toBe("Service");
  });

  it("gives a solution and a solution folder the same category", () => {
    expect(categoryOf({ kind: "solution" })).toEqual(categoryOf({ kind: "solutionFolder" }));
  });

  it("labels a web app and a mobile app by their kind", () => {
    expect(categoryOf({ kind: "webApp" }).label).toBe("Web app");
    expect(categoryOf({ kind: "mobileApp" }).label).toBe("Mobile app");
  });
});

describe("legendEntries", () => {
  it("is empty for a graph with no nodes", () => {
    expect(legendEntries([])).toEqual([]);
  });

  it("lists an ecosystem chip per project ecosystem present", () => {
    const entries = legendEntries([
      { kind: "project", ecosystem: "dotnet" },
      { kind: "project", ecosystem: "node" },
    ]);
    expect(entries.map((entry) => entry.label)).toEqual([".NET", "Node"]);
  });

  it("lists only the categories actually present", () => {
    const entries = legendEntries([{ kind: "service" }, { kind: "dataStore" }]);
    const labels = entries.map((entry) => entry.label);
    expect(labels).toEqual(["Service", "Data store"]);
    expect(labels).not.toContain(".NET");
  });

  it("collapses solution and solution folder into one chip", () => {
    const entries = legendEntries([{ kind: "solution" }, { kind: "solutionFolder" }]);
    expect(entries.map((entry) => entry.label)).toEqual(["Container"]);
  });

  it("lists Web app and Mobile app in the fixed order right after Service", () => {
    const entries = legendEntries([
      { kind: "mobileApp" },
      { kind: "external" },
      { kind: "webApp" },
      { kind: "service" },
    ]);
    expect(entries.map((entry) => entry.label)).toEqual([
      "Service",
      "Web app",
      "Mobile app",
      "External",
    ]);
  });

  it("draws chips in a fixed order regardless of node order", () => {
    const forward = legendEntries([
      { kind: "external" },
      { kind: "dataStore" },
      { kind: "service" },
      { kind: "project", ecosystem: "dotnet" },
    ]);
    const reversed = legendEntries([
      { kind: "project", ecosystem: "dotnet" },
      { kind: "service" },
      { kind: "dataStore" },
      { kind: "external" },
    ]);
    expect(forward).toEqual(reversed);
    expect(forward.map((entry) => entry.label)).toEqual([
      ".NET",
      "Service",
      "Data store",
      "External",
    ]);
  });
});

describe("edgeStyle", () => {
  it("answers a colour variable for every edge kind", () => {
    for (const kind of ALL_EDGE_KINDS) {
      const style = edgeStyle(kind);
      expect(style.colorVar.startsWith("--")).toBe(true);
      expect(typeof style.dashed).toBe("boolean");
    }
  });

  it("dashes a package dependency and nothing else", () => {
    expect(edgeStyle("packageDependency").dashed).toBe(true);
    for (const kind of ALL_EDGE_KINDS.filter((k) => k !== "packageDependency")) {
      expect(edgeStyle(kind).dashed).toBe(false);
    }
  });
});
