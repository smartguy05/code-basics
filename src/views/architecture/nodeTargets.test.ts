import { describe, expect, it } from "vitest";

import {
  declaredNodes,
  mermaidIdOf,
  targetFor,
  targetsByDomId,
  targetsForAuthored,
} from "./nodeTargets";
import type { IndexEntry } from "./nodeTargets";
import type { ArchGraph, ArchNode } from "../../ipc/types";
import mermaidIds from "../../../crates/core/fixtures/architecture/mermaid_ids.json";

function node(partial: Partial<ArchNode> & { id: string }): ArchNode {
  return {
    label: partial.id,
    kind: "project",
    projectId: null,
    path: null,
    ecosystem: null,
    ...partial,
  };
}

function graph(nodes: ArchNode[]): ArchGraph {
  return { nodes, edges: [], warnings: [], derivation: { derived: { scanner: 1 } } };
}

describe("targetFor", () => {
  it("answers the node's own path", () => {
    const g = graph([
      node({ id: "src/Api", path: "src/Api/Api.csproj", kind: "project" }),
    ]);
    expect(targetFor("src/Api", g)).toEqual({ path: "src/Api/Api.csproj" });
  });

  it("returns null for a node that carries no path", () => {
    const g = graph([
      node({ id: "solution:App.sln", kind: "solutionFolder", path: null }),
      node({ id: "external:../Other", kind: "external", path: null }),
      node({ id: "store:postgres", kind: "dataStore", path: null }),
    ]);
    expect(targetFor("solution:App.sln", g)).toBeNull();
    expect(targetFor("external:../Other", g)).toBeNull();
    expect(targetFor("store:postgres", g)).toBeNull();
  });

  it("returns null for an id the graph does not contain", () => {
    expect(targetFor("nope", graph([node({ id: "yes", path: "a.csproj" })]))).toBeNull();
  });

  it("returns null for an empty or blank path", () => {
    const g = graph([node({ id: "a", path: "" }), node({ id: "b", path: "   " })]);
    expect(targetFor("a", g)).toBeNull();
    expect(targetFor("b", g)).toBeNull();
  });

  it("is clickable for a service node, which is a project with more said about it", () => {
    const g = graph([
      node({ id: "src/Api", kind: "service", path: "src/Api/Api.csproj" }),
    ]);
    expect(targetFor("src/Api", g)).toEqual({ path: "src/Api/Api.csproj" });
  });

  it("abstains when two nodes share an id and disagree about the path", () => {
    const g = graph([
      node({ id: "dup", path: "one.csproj" }),
      node({ id: "dup", path: "two.csproj" }),
    ]);
    expect(targetFor("dup", g)).toBeNull();
  });

  it("still answers when a duplicated id agrees with itself", () => {
    const g = graph([
      node({ id: "dup", path: "one.csproj" }),
      node({ id: "dup", path: "one.csproj" }),
    ]);
    expect(targetFor("dup", g)).toEqual({ path: "one.csproj" });
  });
});

describe("mermaidIdOf", () => {
  // Each expectation is `mermaid_id` in `crates/core/src/architecture/mermaid.rs`
  // applied by hand: a leading `n`, ASCII alphanumerics kept, everything else
  // as `_<lowercase hex code point>_`.
  it("mirrors the renderer's escaping for the ids the deriver mints", () => {
    expect(mermaidIdOf("crates-core")).toBe("ncrates_2d_core");
    expect(mermaidIdOf("src-tauri")).toBe("nsrc_2d_tauri");
    expect(mermaidIdOf("workspace:Cargo.toml")).toBe("nworkspace_3a_Cargo_2e_toml");
    expect(mermaidIdOf("solution:src/App.sln")).toBe("nsolution_3a_src_2f_App_2e_sln");
  });

  it("keeps apart the ids that stripping would have merged", () => {
    expect(mermaidIdOf("src/a-b")).not.toBe(mermaidIdOf("src/a.b"));
  });

  it("escapes a non-ASCII character by code point, as the renderer does", () => {
    expect(mermaidIdOf("café")).toBe("ncaf_e9_");
    expect(mermaidIdOf("a😀")).toBe("na_1f600_");
  });

  it("gives an empty id the bare prefix", () => {
    expect(mermaidIdOf("")).toBe("n");
  });
});

describe("targetsByDomId", () => {
  const g = graph([
    node({ id: "crates-core", path: "crates/core/Cargo.toml" }),
    node({ id: "src-tauri", path: "src-tauri/Cargo.toml" }),
    node({ id: "workspace:Cargo.toml", kind: "solution", path: "Cargo.toml" }),
    node({ id: "solution:App.sln", kind: "solutionFolder", path: null }),
  ]);

  // The exact shape mermaid 11.16.1 emits: `${renderId}-flowchart-${id}-${n}`
  // for a node (`MERMAID_DOM_ID_PREFIX` + the vertex counter, prefixed with the
  // diagram id in `render`), and `${renderId}-${id}` for a subgraph, which
  // never went through the vertex table and so carries neither.
  it("resolves a node's mermaid DOM id back to its graph node", () => {
    expect(
      targetsByDomId("cb-diagram-1", ["cb-diagram-1-flowchart-ncrates_2d_core-3"], g),
    ).toEqual(new Map([["cb-diagram-1-flowchart-ncrates_2d_core-3", { path: "crates/core/Cargo.toml" }]]));
  });

  it("resolves a subgraph's DOM id, which carries no prefix and no counter", () => {
    expect(
      targetsByDomId("cb-diagram-1", ["cb-diagram-1-nworkspace_3a_Cargo_2e_toml"], g),
    ).toEqual(
      new Map([["cb-diagram-1-nworkspace_3a_Cargo_2e_toml", { path: "Cargo.toml" }]]),
    );
  });

  it("holds only the ids that resolved", () => {
    const targets = targetsByDomId(
      "cb-diagram-1",
      [
        "cb-diagram-1-flowchart-ncrates_2d_core-3",
        "cb-diagram-1-nsolution_3a_App_2e_sln",
        "cb-diagram-1-flowchart-nabsent-9",
      ],
      g,
    );
    expect([...targets.keys()]).toEqual(["cb-diagram-1-flowchart-ncrates_2d_core-3"]);
  });

  it("leaves the legend alone", () => {
    expect(
      targetsByDomId(
        "cb-diagram-1",
        ["cb-diagram-1-legend", "cb-diagram-1-flowchart-legend_project-7"],
        g,
      ).size,
    ).toBe(0);
  });

  it("refuses a DOM id belonging to another diagram on the page", () => {
    expect(
      targetsByDomId("cb-diagram-2", ["cb-diagram-1-flowchart-ncrates_2d_core-3"], g).size,
    ).toBe(0);
  });

  it("is anchored, so a diagram id that merely contains ours does not match", () => {
    // `cb-diagram-1` is a prefix of `cb-diagram-10`'s ids as a substring test
    // would see it, and the two are different renders.
    expect(
      targetsByDomId("cb-diagram-1", ["cb-diagram-10-flowchart-ncrates_2d_core-3"], g).size,
    ).toBe(0);
  });

  it("refuses the sub-elements mermaid derives from a node's DOM id", () => {
    expect(
      targetsByDomId(
        "cb-diagram-1",
        ["cb-diagram-1-flowchart-ncrates_2d_core-3-background"],
        g,
      ).size,
    ).toBe(0);
  });

  it("refuses a node-shaped id with no counter", () => {
    expect(
      targetsByDomId("cb-diagram-1", ["cb-diagram-1-flowchart-ncrates_2d_core"], g).size,
    ).toBe(0);
  });

  it("abstains on a node the graph gives no path", () => {
    expect(
      targetsByDomId("cb-diagram-1", ["cb-diagram-1-nsolution_3a_App_2e_sln"], g).size,
    ).toBe(0);
  });

  it("abstains for every id when the render id is blank", () => {
    // Nothing can be anchored against an empty prefix, and a diagram with no
    // id is a state the caller should never reach — abstaining is the answer
    // that cannot open the wrong file.
    expect(targetsByDomId("", ["flowchart-ncrates_2d_core-3"], g).size).toBe(0);
  });
});

describe("declaredNodes", () => {
  it("reads every shape the renderer emits", () => {
    const source = [
      "flowchart TD",
      '    nA["Api"]',
      '    nB(["Outside"])',
      '    nC[["App.sln"]]',
      '    nD("Gateway")',
      '    nE[("postgres")]',
    ].join("\n");
    expect(declaredNodes(source)).toEqual([
      { id: "nA", label: "Api" },
      { id: "nB", label: "Outside" },
      { id: "nC", label: "App.sln" },
      { id: "nD", label: "Gateway" },
      { id: "nE", label: "postgres" },
    ]);
  });

  it("reads unquoted labels and the diamond shape", () => {
    expect(declaredNodes("flowchart LR\n  A[Api Gateway]\n  B{Choice}")).toEqual([
      { id: "A", label: "Api Gateway" },
      { id: "B", label: "Choice" },
    ]);
  });

  it("does not read a mention as a declaration", () => {
    // Only `A` and `C` are declared; `B` is named by an edge and never shaped,
    // so nothing is known about it beyond the id.
    const source = 'flowchart TD\n  A["Api"] --> B\n  B --> C["Db"]';
    expect(declaredNodes(source)).toEqual([
      { id: "A", label: "Api" },
      { id: "C", label: "Db" },
    ]);
  });

  it("does not mistake edge or subgraph text for a node", () => {
    const source = [
      "flowchart TD",
      '  subgraph S["Solution"]',
      '    A["Api"] -- calls --> B["Web"]',
      "  end",
      "  A -->|reads| B",
      "  style A fill:#333",
    ].join("\n");
    expect(declaredNodes(source)).toEqual([
      { id: "S", label: "Solution" },
      { id: "A", label: "Api" },
      { id: "B", label: "Web" },
    ]);
  });

  it("ignores comments and front matter", () => {
    const source = [
      "---",
      "derivation: inferred",
      'title: Ignored["NotANode"]',
      "---",
      "```mermaid",
      "flowchart TD",
      '%% Z["Commented"] could not be drawn',
      '  A["Api"]  %% B["Trailing"]',
      "```",
    ].join("\n");
    expect(declaredNodes(source)).toEqual([{ id: "A", label: "Api" }]);
  });

  it("unescapes the one escape the renderer emits", () => {
    expect(declaredNodes('flowchart TD\n  A["The #quot;Old#quot; Api"]')).toEqual([
      { id: "A", label: 'The "Old" Api' },
    ]);
  });

  it("keeps the first declaration when a label is repeated identically", () => {
    expect(declaredNodes('flowchart TD\n  A["Api"] --> B["Web"]\n  A["Api"]')).toEqual(
      [
        { id: "A", label: "Api" },
        { id: "B", label: "Web" },
      ],
    );
  });

  it("drops an id declared twice with two different labels", () => {
    expect(declaredNodes('flowchart TD\n  A["Api"]\n  A["Web"]\n  B["Db"]')).toEqual([
      { id: "B", label: "Db" },
    ]);
  });

  it("returns nothing for source with no declarations", () => {
    expect(declaredNodes("")).toEqual([]);
    expect(declaredNodes("flowchart TD\n  A --> B")).toEqual([]);
  });
});

describe("targetsForAuthored", () => {
  const index: IndexEntry[] = [
    { label: "Api.csproj", path: "src/Api/Api.csproj", line: null },
    { label: "OrderService", path: "src/Api/OrderService.cs", line: 12 },
    { label: "Web", path: "src/Web/Web.csproj", line: null },
  ];

  it("matches a node label onto a unique index entry", () => {
    const targets = targetsForAuthored('flowchart TD\n  A["OrderService"]', index);
    expect(targets.get("A")).toEqual({ path: "src/Api/OrderService.cs", line: 12 });
  });

  it("falls back to the node id when the label matches nothing", () => {
    const targets = targetsForAuthored("flowchart TD\n  Web[Payments box]", index);
    expect(targets.get("Web")).toEqual({ path: "src/Web/Web.csproj" });
  });

  it("returns no target for a node nothing in the index matches", () => {
    const targets = targetsForAuthored('flowchart TD\n  A["Nothing here"]', index);
    expect(targets.has("A")).toBe(false);
    expect(targets.size).toBe(0);
  });

  it("abstains when the label matches more than one place", () => {
    const ambiguous: IndexEntry[] = [
      { label: "Options", path: "src/Api/Options.cs", line: 3 },
      { label: "Options", path: "src/Web/Options.cs", line: 8 },
    ];
    const targets = targetsForAuthored('flowchart TD\n  A["Options"]', ambiguous);
    expect(targets.size).toBe(0);
  });

  it("abstains when one file offers two different lines", () => {
    const ambiguous: IndexEntry[] = [
      { label: "Options", path: "src/Api/Options.cs", line: null },
      { label: "Options", path: "src/Api/Options.cs", line: 8 },
    ];
    expect(targetsForAuthored('flowchart TD\n  A["Options"]', ambiguous).size).toBe(0);
  });

  it("does not fall back to the id after an ambiguous label", () => {
    // `Web` is unique as an id, but the label was ambiguous, and answering
    // from a key we had already found two answers under would be a guess.
    const entries: IndexEntry[] = [
      ...index,
      { label: "Options", path: "src/Api/Options.cs", line: 3 },
      { label: "Options", path: "src/Web/Options.cs", line: 8 },
    ];
    const targets = targetsForAuthored('flowchart TD\n  Web["Options"]', entries);
    expect(targets.size).toBe(0);
  });

  it("collapses index entries that agree exactly", () => {
    const duplicated: IndexEntry[] = [
      { label: "Api.csproj", path: "src/Api/Api.csproj", line: null },
      { label: "Api.csproj", path: "src/Api/Api.csproj", line: null },
    ];
    const targets = targetsForAuthored('flowchart TD\n  A["Api.csproj"]', duplicated);
    expect(targets.get("A")).toEqual({ path: "src/Api/Api.csproj" });
  });

  it("will not match on casing alone", () => {
    const targets = targetsForAuthored('flowchart TD\n  A["orderservice"]', index);
    expect(targets.size).toBe(0);
  });

  it("ignores index entries with no path", () => {
    const withAction: IndexEntry[] = [
      { label: "Api", path: null, line: null },
      { label: "Api", path: "src/Api/Api.csproj", line: null },
    ];
    const targets = targetsForAuthored('flowchart TD\n  A["Api"]', withAction);
    expect(targets.get("A")).toEqual({ path: "src/Api/Api.csproj" });
  });

  it("accepts a SearchHit as an index entry", () => {
    // The structural type is satisfied by what `searchEverywhere` returns.
    const hits = [
      {
        kind: "symbol" as const,
        label: "OrderService",
        detail: "src/Api/OrderService.cs",
        path: "src/Api/OrderService.cs",
        line: 12,
        symbolKind: "class" as const,
        actionId: null,
        positions: [],
        score: 1,
      },
    ];
    const targets = targetsForAuthored('flowchart TD\n  A["OrderService"]', hits);
    expect(targets.get("A")).toEqual({ path: "src/Api/OrderService.cs", line: 12 });
  });

  it("returns an empty map for an empty index", () => {
    expect(targetsForAuthored('flowchart TD\n  A["Api"]', []).size).toBe(0);
  });
});

describe("mermaidIdOf cross-language fixture", () => {
  // The single canonical id list lives in the cb-core guard
  // `mermaid_id_matches_committed_fixture` (mermaid_tests.rs): it computes
  // `mermaid_id` for each id and pins the pairs into the committed JSON imported
  // here. This side derives ids from the fixture only — it never re-declares
  // them — so the Rust `mermaid_id` and TS `mermaidIdOf` are held to one table
  // and any escaping change in either language breaks a test. Regenerate with
  // `UPDATE_FIXTURES=1 cargo test -p cb-core mermaid_id_matches_committed_fixture`.
  it("matches mermaid_id for every id in the committed cb-core fixture", () => {
    expect(mermaidIds.length).toBeGreaterThan(0);
    for (const { id, mermaidId } of mermaidIds) {
      expect(mermaidIdOf(id)).toBe(mermaidId);
    }
  });
});
