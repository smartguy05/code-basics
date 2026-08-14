import { describe, expect, it } from "vitest";
import {
  ANCHOR_MARGIN_LINES,
  ANCHOR_RETRY_LIMIT,
  ANCHOR_RETRY_MS,
  INERT_LOCATION_REASON,
  MENU_WIDTH,
  READINESS_CEILING_MS,
  UNSETTLED_RETRY_LIMIT,
  type UsageRequestState,
  actionDetail,
  availabilityPhrase,
  clearUsageAnswers,
  countUsageRows,
  definitionAction,
  emptyGroupNote,
  failedUsageResult,
  groupUsages,
  newUsageAnswers,
  partialAnswerNote,
  placeMenu,
  recordUsageAnswer,
  retainUsageJobs,
  shouldAskUsages,
  shouldRetryAnchors,
  snippetParts,
  toneClass,
  usageCacheKey,
  usageStateFor,
  usageCountLabel,
  usageRowView,
  visibleAnchors,
} from "./usagesLogic";
import type {
  Availability,
  DeclarationAnchor,
  DefinitionResult,
  Target,
  Usage,
  UsageResult,
} from "../ipc/types";

// ---------------------------------------------------------------------------
// Builders. Every key the wire types declare is present, `null` where there is
// no value — the same rule the Rust structs follow, so a test can never pass by
// leaving a field off that the real backend always sends.
// ---------------------------------------------------------------------------

function result(over: Partial<UsageResult> & { outcome: Availability }): UsageResult {
  return {
    total: null,
    usages: [],
    truncated: false,
    message: null,
    server: null,
    ...over,
  };
}

/** A resolved request, which is the only state carrying a `UsageResult`. */
function answered(over: Partial<UsageResult> & { outcome: Availability }): UsageRequestState {
  return { status: "answered", result: result(over) };
}

function usage(over: Partial<Usage> & { label: string }): Usage {
  return {
    path: over.path === undefined ? over.label : over.path,
    line: 1,
    snippet: "",
    highlight: null,
    ...over,
  };
}

function target(over: Partial<Target> & { label: string }): Target {
  return {
    path: over.path === undefined ? over.label : over.path,
    line: 1,
    character: 0,
    snippet: "",
    container: null,
    ...over,
  };
}

function definition(over: Partial<DefinitionResult> & { outcome: Availability }): DefinitionResult {
  return {
    declarations: [],
    implementations: [],
    typeDefinitions: [],
    message: null,
    ...over,
  };
}

function anchor(over: Partial<DeclarationAnchor> & { line: number }): DeclarationAnchor {
  return {
    id: `a@${over.line}`,
    name: "Thing",
    kind: "function",
    character: 0,
    selectionLine: over.line,
    ...over,
  };
}

// ---------------------------------------------------------------------------
// 1 + 2. The inline row's text, and whether it does anything.
// ---------------------------------------------------------------------------

describe("usageRowView", () => {
  it("says nothing about a count for a request that has not been made yet", () => {
    const view = usageRowView({ status: "idle" });
    expect(view.total).toBeNull();
    expect(view.tone).toBe("idle");
    expect(view.action.kind).toBe("inert");
    expect(view.text).not.toMatch(/\d/);
  });

  it("shows a waiting phrase and no number while the request is in flight", () => {
    const view = usageRowView({ status: "pending" });
    expect(view.total).toBeNull();
    expect(view.tone).toBe("waiting");
    expect(view.text).not.toMatch(/\d/);
    expect(view.action.kind).toBe("inert");
  });

  it("says there are no usages when a ready answer really counted zero", () => {
    const view = usageRowView(answered({ outcome: "ready", total: 0 }));
    expect(view.text).toBe("No usages");
    expect(view.tone).toBe("empty");
    expect(view.total).toBe(0);
  });

  it("uses the singular for exactly one usage", () => {
    const view = usageRowView(answered({ outcome: "ready", total: 1 }));
    expect(view.text).toBe("1 usage");
    expect(view.tone).toBe("count");
  });

  it("uses the plural for more than one usage", () => {
    expect(usageRowView(answered({ outcome: "ready", total: 7 })).text).toBe("7 usages");
  });

  it("reports the true total rather than the number of rows when truncated", () => {
    const view = usageRowView(
      answered({
        outcome: "ready",
        total: 900,
        usages: [usage({ label: "a.ts" })],
        truncated: true,
      }),
    );
    expect(view.text).toBe("900 usages");
    expect(view.total).toBe(900);
    expect(view.truncated).toBe(true);
  });

  it("opens a dropdown only for an answer that carries a real count", () => {
    const view = usageRowView(answered({ outcome: "ready", total: 3 }));
    expect(view.action).toEqual({ kind: "dropdown", total: 3 });
    // Pinned false, not merely unread: a hardcoded `true` in every return path
    // would otherwise pass the suite and print "Showing the first 3 of 3" under
    // a complete list, which is the short-list/wrong-list distinction inverted.
    expect(view.truncated).toBe(false);
  });

  it("agrees with the label the dropdown heading uses, at every count", () => {
    // The heading and the row are one fact said twice, and the count of zero is
    // where a second pluralisation always diverges first ("0 usages").
    for (const total of [0, 1, 2, 7]) {
      expect(usageRowView(answered({ outcome: "ready", total })).text).toBe(
        usageCountLabel(total),
      );
    }
  });

  it("opens a dropdown even when the real count is zero, because zero is an answer", () => {
    expect(usageRowView(answered({ outcome: "ready", total: 0 })).action).toEqual({
      kind: "dropdown",
      total: 0,
    });
  });

  it("carries a ready answer's qualifying message as the tooltip", () => {
    const view = usageRowView(
      answered({
        outcome: "ready",
        total: 2,
        message: "the server was still loading its projects; this count may be low",
      }),
    );
    expect(view.tooltip).toBe(
      "the server was still loading its projects; this count may be low",
    );
    expect(view.action.kind).toBe("dropdown");
  });

  // -------------------------------------------------------------------------
  // `ready` × {message, no message} × {total 0, total n}. Four distinct states,
  // three of which were indistinguishable in the output until this block existed.
  // -------------------------------------------------------------------------

  /**
   * The exact payload the running app received above `TryGetElements` in
   * `sidecar/inspector/Collections.cs`, where the row read "No usages" about a
   * method with one usage at `Walker.cs:138`.
   */
  const CAVEAT =
    "this answer was taken from a server that never finished priming (the server " +
    "did not send `workspace/projectInitializationComplete` within 90s. It is being " +
    "treated as ready, so its answers may be incomplete.), so a count may be low.";

  it("does not claim there are none when the backend said the count may be low", () => {
    const view = usageRowView(
      answered({
        outcome: "ready",
        total: 0,
        usages: [],
        truncated: false,
        message: CAVEAT,
        server: "csharp",
      }),
    );
    // The observed wrong answer, verbatim.
    expect(view.text).not.toBe("No usages");
    // And not any other phrasing of "there are none" either.
    expect(view.text).not.toMatch(/\bno(ne)?\b/i);
    expect(view.provisional).toBe(true);
    expect(view.tooltip).toBe(CAVEAT);
    // Still clickable: the dropdown is where the reason is read at length.
    expect(view.action).toEqual({ kind: "dropdown", total: 0 });
  });

  it("presents a caveated count as a floor rather than a total", () => {
    const view = usageRowView(answered({ outcome: "ready", total: 7, message: CAVEAT }));
    // "7 usages" is the authoritative phrasing and is the one thing this must not
    // be: the backend said the count may be low, so 7 is a lower bound.
    expect(view.text).not.toBe("7 usages");
    expect(view.text).toMatch(/at least/i);
    expect(view.text).toContain("7");
    expect(view.provisional).toBe(true);
    expect(view.total).toBe(7);
  });

  it("keeps a caveated answer visually distinct from a settled one", () => {
    const plain = usageRowView(answered({ outcome: "ready", total: 7 }));
    const caveated = usageRowView(answered({ outcome: "ready", total: 7, message: CAVEAT }));
    expect(plain.tone).not.toBe(caveated.tone);
    // The tone a component styles must be one the stylesheet already has a rule
    // for; `reason` is the "this is about the tooling, hover for why" tone.
    expect(caveated.tone).toBe("reason");
  });

  it("distinguishes all four ready shapes from each other", () => {
    const rows = [
      usageRowView(answered({ outcome: "ready", total: 0 })),
      usageRowView(answered({ outcome: "ready", total: 0, message: CAVEAT })),
      usageRowView(answered({ outcome: "ready", total: 7 })),
      usageRowView(answered({ outcome: "ready", total: 7, message: CAVEAT })),
    ];
    expect(new Set(rows.map((r) => r.text)).size).toBe(4);
    expect(rows.map((r) => r.provisional)).toEqual([false, true, false, true]);
    // And the widget's own comparison sees the difference too, so a row does not
    // survive a caveat appearing or clearing.
    expect(new Set(rows.map((r) => `${r.text} ${r.tooltip} ${r.tone}`)).size).toBe(4);
  });

  it("marks a settled answer as not provisional, at zero and above", () => {
    expect(usageRowView(answered({ outcome: "ready", total: 0 })).provisional).toBe(false);
    expect(usageRowView(answered({ outcome: "ready", total: 7 })).provisional).toBe(false);
    expect(usageRowView({ status: "idle" }).provisional).toBe(false);
    expect(usageRowView({ status: "pending" }).provisional).toBe(false);
    for (const outcome of ["starting", "loading", "notConfigured", "failed", "unsupported"] as const) {
      expect(usageRowView(answered({ outcome })).provisional, outcome).toBe(false);
    }
  });

  it("says a server is still starting rather than showing a number", () => {
    const view = usageRowView(answered({ outcome: "starting" }));
    expect(view.tone).toBe("waiting");
    expect(view.total).toBeNull();
    expect(view.text).toMatch(/starting/i);
    expect(view.action.kind).toBe("inert");
  });

  it("says a server is still loading rather than showing a number", () => {
    const view = usageRowView(answered({ outcome: "loading" }));
    expect(view.tone).toBe("waiting");
    expect(view.total).toBeNull();
    expect(view.text).toMatch(/loading/i);
    expect(view.action.kind).toBe("inert");
  });

  it("shows the unconfigured server's own actionable message as the tooltip", () => {
    const hint =
      "no TypeScript language server was found on this machine. Install it with " +
      "`npm i -g typescript-language-server typescript`, or set " +
      "`lsp.servers.typescript.program` in .code-basics/config.json";
    const view = usageRowView(answered({ outcome: "notConfigured", message: hint }));
    expect(view.tone).toBe("reason");
    expect(view.tooltip).toBe(hint);
    expect(view.total).toBeNull();
    expect(view.action).toEqual({ kind: "inert", reason: hint });
  });

  it("shows why a failed server has no answer", () => {
    const view = usageRowView(
      answered({ outcome: "failed", message: "the server exited with code 134" }),
    );
    expect(view.tone).toBe("reason");
    expect(view.tooltip).toBe("the server exited with code 134");
    expect(view.total).toBeNull();
    expect(view.action.kind).toBe("inert");
  });

  it("says an unsupported capability cannot answer, and never that there are none", () => {
    const view = usageRowView(answered({ outcome: "unsupported" }));
    expect(view.tone).toBe("reason");
    expect(view.total).toBeNull();
    expect(view.text).not.toMatch(/^No usages$/);
    expect(view.text).not.toMatch(/\bnone\b/i);
    expect(view.text).toMatch(/cannot|does not/i);
    expect(view.action.kind).toBe("inert");
  });

  it("falls back to its own words when a non-ready outcome carries no message", () => {
    for (const outcome of ["starting", "loading", "notConfigured", "failed", "unsupported"] as const) {
      const view = usageRowView(answered({ outcome }));
      expect(view.tooltip, outcome).not.toBeNull();
      expect(view.tooltip, outcome).not.toBe("");
    }
  });

  it("shows words rather than a count when a ready answer carries no total", () => {
    // The contract says `total` is non-null whenever the outcome is ready, so
    // this shape is a backend contradicting itself — and the failure mode is a
    // row reading "null usages", which is why the guard is on the number and not
    // only on the outcome.
    const view = usageRowView(answered({ outcome: "ready", total: null }));
    expect(view.total).toBeNull();
    expect(view.text).not.toMatch(/null/);
    expect(view.action.kind).toBe("inert");
    // And it must not borrow the tone or the wording of an answer. `count` is
    // documented in `styles.css` as "the only tone that is brighter than the
    // file's comments, because it is the only one carrying an answer", and this
    // row is the one that has none.
    expect(view.tone).toBe("reason");
    expect(view.text).not.toBe("Ready");
  });

  it("never reports a count for any outcome other than ready", () => {
    for (const outcome of ["starting", "loading", "notConfigured", "failed", "unsupported"] as const) {
      // A backend that contradicted its own contract must still not produce a
      // number on screen — the type says `total` is null unless ready, and this
      // is the one place that would render it if it were not.
      const view = usageRowView(answered({ outcome, total: 4 as number }));
      expect(view.total, outcome).toBeNull();
      expect(view.action.kind, outcome).toBe("inert");
    }
  });
});

describe("availabilityPhrase", () => {
  it("gives every one of the six availabilities its own distinct sentence", () => {
    const all: Availability[] = [
      "notConfigured",
      "starting",
      "loading",
      "ready",
      "failed",
      "unsupported",
    ];
    const texts = all.map((o) => availabilityPhrase(o).text);
    expect(new Set(texts).size).toBe(all.length);
  });

  it("does not give the ready phrase the tone of a real count", () => {
    // This phrase was documented as "never actually shown" and is shown: it is
    // the fallthrough for a `ready` result whose `total` is not a number, which
    // the row above is deliberately written to survive. A row with no answer must
    // not be drawn in the answer colour.
    expect(availabilityPhrase("ready").tone).toBe("reason");
    expect(availabilityPhrase("ready").text).not.toBe("Ready");
  });
});

// ---------------------------------------------------------------------------
// 3. Grouping usages for the dropdown.
// ---------------------------------------------------------------------------

describe("groupUsages", () => {
  it("groups rows by file without reordering what the backend sorted", () => {
    const groups = groupUsages([
      usage({ label: "src/b.ts", line: 3 }),
      usage({ label: "src/b.ts", line: 9 }),
      usage({ label: "src/a.ts", line: 1 }),
    ]);
    expect(groups.map((g) => g.label)).toEqual(["src/b.ts", "src/a.ts"]);
    expect(groups[0]!.rows.map((r) => r.usage.line)).toEqual([3, 9]);
  });

  it("keeps two runs of the same file in one group even when they are not adjacent", () => {
    const groups = groupUsages([
      usage({ label: "a.ts", line: 1 }),
      usage({ label: "b.ts", line: 2 }),
      usage({ label: "a.ts", line: 3 }),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0]!.rows.map((r) => r.usage.line)).toEqual([1, 3]);
  });

  it("counts every row it groups, including the ones that cannot be opened", () => {
    const groups = groupUsages([
      usage({ label: "a.ts", line: 1 }),
      usage({ label: "source-generated:///Gen.cs", path: null, line: 4 }),
    ]);
    expect(groups.reduce((n, g) => n + g.rows.length, 0)).toBe(2);
  });

  it("marks a row with no path unopenable and labels it with its raw uri", () => {
    const groups = groupUsages([
      usage({ label: "source-generated:///Gen.cs", path: null, line: 4 }),
    ]);
    expect(groups[0]!.label).toBe("source-generated:///Gen.cs");
    expect(groups[0]!.path).toBeNull();
    expect(groups[0]!.openable).toBe(false);
    expect(groups[0]!.rows[0]!.openable).toBe(false);
  });

  it("still renders and still counts a group whose every row is unopenable", () => {
    const groups = groupUsages([
      usage({ label: "metadata:///System.Object", path: null, line: 1 }),
      usage({ label: "metadata:///System.Object", path: null, line: 2 }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0]!.openable).toBe(false);
    expect(groups[0]!.rows).toHaveLength(2);
  });

  it("does not merge two different documents that happen to share a label", () => {
    // A pathless row's identity is its raw uri, which is its label; a real file's
    // identity is its path. Keying everything on the label alone would fold an
    // unopenable row into the file whose relative path spells the same string.
    const groups = groupUsages([
      usage({ label: "src/a.ts", line: 1 }),
      usage({ label: "src/a.ts", path: null, line: 2 }),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0]!.openable).toBe(true);
    expect(groups[1]!.openable).toBe(false);
  });

  it("returns no groups for no usages", () => {
    expect(groupUsages([])).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// 4. What middle-click does.
// ---------------------------------------------------------------------------

describe("definitionAction", () => {
  it("jumps when exactly one target exists across all three groups", () => {
    const only = target({ label: "src/order.ts", line: 12 });
    const action = definitionAction(definition({ outcome: "ready", declarations: [only] }));
    expect(action).toEqual({ kind: "jump", target: only });
  });

  it("jumps for a lone target found in the implementations group", () => {
    const only = target({ label: "src/impl.ts", line: 3 });
    const action = definitionAction(
      definition({ outcome: "ready", implementations: [only] }),
    );
    expect(action.kind).toBe("jump");
  });

  it("jumps for a lone target found in the type definitions group", () => {
    const only = target({ label: "src/types.ts", line: 3 });
    const action = definitionAction(
      definition({ outcome: "ready", typeDefinitions: [only] }),
    );
    expect(action).toEqual({ kind: "jump", target: only });
  });

  it("does not jump on a lone target when the backend qualified the answer", () => {
    // The live failure: a server with no project loaded resolves `definition` and
    // answers `implementation` and `typeDefinition` with nothing, so "exactly one
    // target" is an artefact of what could not be asked. Jumping presents a symbol
    // with five implementations as having one place to go, and throws away the
    // sentence that said so. The picker renders the message; the jump cannot.
    const only = target({ label: "src/order.ts", line: 12 });
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [only],
        message: "this answer was taken from a server that never finished priming",
      }),
    );
    expect(action.kind).toBe("pick");
    if (action.kind !== "pick") throw new Error("unreachable");
    expect(action.message).toContain("never finished priming");
    expect(action.groups[0]?.targets).toEqual([only]);
  });

  it("jumps when two groups name the same place, because that is one destination", () => {
    // Observed in the running app on `Collections.TryGetElements`: a static
    // method is its own implementation, so `definition` and `implementation`
    // both answer `Collections.cs:26`. Counting rows rather than destinations
    // opened a picker offering one place to go, twice.
    const declaration = target({ label: "sidecar/inspector/Collections.cs", line: 26 });
    const implementation = target({ label: "sidecar/inspector/Collections.cs", line: 26 });
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [declaration],
        implementations: [implementation],
      }),
    );
    expect(action).toEqual({ kind: "jump", target: declaration });
  });

  it("still picks when two groups name the same file at different lines", () => {
    // One file, two places in it, is still a choice the user has to make.
    const declaration = target({ label: "src/order.ts", line: 12 });
    const implementation = target({ label: "src/order.ts", line: 40 });
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [declaration],
        implementations: [implementation],
      }),
    );
    expect(action.kind).toBe("pick");
  });

  it("keeps every group's rows when it picks, duplicates included", () => {
    // Deduplicating is a decision about *where to go*, not about what to show:
    // a reader wants to see that a symbol is both a declaration and an
    // implementation, so the groups are unchanged.
    const a = target({ label: "src/a.ts", line: 1 });
    const b = target({ label: "src/b.ts", line: 2 });
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [a],
        implementations: [a, b],
      }),
    );
    expect(action.kind).toBe("pick");
    if (action.kind !== "pick") throw new Error("unreachable");
    expect(action.groups[0]?.targets).toEqual([a]);
    expect(action.groups[1]?.targets).toEqual([a, b]);
  });

  it("does not merge two unopenable targets that name different documents", () => {
    // Both have no path, so neither can be jumped to, and they are not the same
    // place. Keying on the path alone would collapse them into one.
    const first = target({ label: "source-generated:///A.g.cs", path: null, line: 1 });
    const second = target({ label: "source-generated:///B.g.cs", path: null, line: 1 });
    const action = definitionAction(
      definition({ outcome: "ready", declarations: [first, second] }),
    );
    expect(action.kind).toBe("pick");
  });

  it("never silently picks one when there is more than one target", () => {
    const a = target({ label: "src/i.ts", line: 1 });
    const b = target({ label: "src/c.ts", line: 2 });
    const action = definitionAction(
      definition({ outcome: "ready", declarations: [a], implementations: [b] }),
    );
    expect(action.kind).toBe("pick");
  });

  it("groups a picker as declarations, implementations and type definitions in that order", () => {
    const a = target({ label: "a.ts" });
    const b = target({ label: "b.ts" });
    const action = definitionAction(
      definition({ outcome: "ready", declarations: [a, b] }),
    );
    if (action.kind !== "pick") throw new Error("expected a picker");
    expect(action.groups.map((g) => g.label)).toEqual([
      "Declarations",
      "Implementations",
      "Type definitions",
    ]);
  });

  it("keeps an empty group in the picker rather than omitting it", () => {
    const a = target({ label: "a.ts" });
    const b = target({ label: "b.ts" });
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [a, b],
        message: "No implementations: this server does not provide them.",
      }),
    );
    if (action.kind !== "pick") throw new Error("expected a picker");
    const implementations = action.groups[1]!;
    expect(implementations.targets).toEqual([]);
    expect(implementations.empty).toBe(true);
    expect(action.message).toBe("No implementations: this server does not provide them.");
  });

  it("does not attach a group-naming message to any one group, because it names its own", () => {
    // The backend's message is English prose that names the group it is about
    // ("No implementations: …"). Copying it under every empty group would put an
    // implementations sentence under Type definitions, and parsing it to find
    // out which group it means is the guess this subsystem exists to refuse.
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [target({ label: "a.ts" }), target({ label: "b.ts" })],
        message: "No implementations: this server does not provide them.",
      }),
    );
    if (action.kind !== "pick") throw new Error("expected a picker");
    expect(action.groups.every((g) => g.note === null)).toBe(true);
  });

  it("reports no definition found when a ready answer had all three groups empty", () => {
    const action = definitionAction(definition({ outcome: "ready" }));
    expect(action.kind).toBe("none");
    if (action.kind !== "none") throw new Error("expected none");
    expect(action.message).toMatch(/no definition/i);
  });

  it("gives the backend's own reason when nothing could be asked at all", () => {
    const action = definitionAction(
      definition({
        outcome: "unsupported",
        message: "this server provides none of definition, implementation or type definition",
      }),
    );
    expect(action).toEqual({
      kind: "none",
      message: "this server provides none of definition, implementation or type definition",
      outcome: "unsupported",
    });
  });

  it("says the server is still loading rather than that there is no definition", () => {
    const action = definitionAction(definition({ outcome: "loading" }));
    if (action.kind !== "none") throw new Error("expected none");
    expect(action.message).toMatch(/loading/i);
    expect(action.message).not.toMatch(/no definition/i);
  });

  it("carries the outcome into the picker, so a provisional list can say so", () => {
    // The one code path that had no test: a non-ready outcome that nonetheless
    // has targets. A loading server answering `definition` while still resolving
    // implementations gives a partial list, and a picker that renders it exactly
    // like a ready one states "no implementations" about a question nobody could
    // ask yet.
    const action = definitionAction(
      definition({
        outcome: "loading",
        declarations: [target({ label: "a.ts" }), target({ label: "b.ts" })],
      }),
    );
    if (action.kind !== "pick") throw new Error("expected a picker");
    expect(action.outcome).toBe("loading");
    expect(action.message).toBeNull();
    expect(partialAnswerNote(action.outcome, action.message)).not.toBeNull();
  });

  it("marks a ready picker as needing no provisional note", () => {
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [target({ label: "a.ts" }), target({ label: "b.ts" })],
      }),
    );
    if (action.kind !== "pick") throw new Error("expected a picker");
    expect(action.outcome).toBe("ready");
    expect(partialAnswerNote(action.outcome, action.message)).toBeNull();
  });

  it("does not offer a jump to a target that cannot be opened", () => {
    // A single pathless target is a real answer about where the symbol is and a
    // useless one to jump to, so it is shown rather than acted on.
    const action = definitionAction(
      definition({
        outcome: "ready",
        declarations: [target({ label: "metadata:///System.String", path: null })],
      }),
    );
    expect(action.kind).toBe("pick");
  });
});

describe("emptyGroupNote", () => {
  it("licenses 'none' only when nothing was refused", () => {
    expect(emptyGroupNote(null)).toBe("None.");
  });

  it("never says 'none' about a group the server may have refused", () => {
    // One message covers three lists and names its group in prose, so an empty
    // group beside a message is not evidence of emptiness. "None." there is the
    // "unsupported reads as there are none" failure, at one remove.
    const note = emptyGroupNote("No implementations: this server does not provide them.");
    expect(note).not.toMatch(/^None\.$/);
    expect(note).not.toMatch(/\bnone\b/i);
  });
});

describe("partialAnswerNote", () => {
  it("says nothing extra about a ready answer", () => {
    expect(partialAnswerNote("ready", null)).toBeNull();
  });

  it("warns about a ready answer the backend itself qualified", () => {
    // `ready` plus a message is a legal shape and means the lists came from a
    // server promoted at the readiness ceiling. Equating `ready` with settled is
    // the same assumption the inline row was just fixed for, one function down.
    const note = partialAnswerNote("ready", "so a count may be low.");
    expect(note).not.toBeNull();
    expect(note).toMatch(/incomplete/i);
  });

  it("warns that any other outcome's list may be incomplete", () => {
    for (const outcome of ["starting", "loading", "notConfigured", "failed", "unsupported"] as const) {
      const note = partialAnswerNote(outcome, null);
      expect(note, outcome).not.toBeNull();
      // The outcome's own words, so the picker and the inline row never disagree
      // about what the server is doing.
      expect(note, outcome).toContain(availabilityPhrase(outcome).text);
      expect(note, outcome).toMatch(/incomplete/i);
    }
  });
});

// ---------------------------------------------------------------------------
// 5. Which anchors are worth asking about.
// ---------------------------------------------------------------------------

describe("visibleAnchors", () => {
  it("keeps an anchor sitting exactly on the first visible line", () => {
    expect(visibleAnchors([anchor({ line: 40 })], 40, 60, 0).map((a) => a.line)).toEqual([
      40,
    ]);
  });

  it("keeps an anchor sitting exactly on the last visible line", () => {
    expect(visibleAnchors([anchor({ line: 60 })], 40, 60, 0).map((a) => a.line)).toEqual([
      60,
    ]);
  });

  it("drops an anchor one line beyond the margin at each end", () => {
    const anchors = [anchor({ line: 39 }), anchor({ line: 61 })];
    expect(visibleAnchors(anchors, 40, 60, 0)).toEqual([]);
  });

  it("keeps an anchor just off screen, within the margin", () => {
    const anchors = [anchor({ line: 35 }), anchor({ line: 65 })];
    expect(visibleAnchors(anchors, 40, 60, 5).map((a) => a.line)).toEqual([35, 65]);
  });

  it("uses the default margin when none is given", () => {
    const off = anchor({ line: 40 - ANCHOR_MARGIN_LINES });
    expect(visibleAnchors([off], 40, 60)).toEqual([off]);
    const tooFar = anchor({ line: 40 - ANCHOR_MARGIN_LINES - 1 });
    expect(visibleAnchors([tooFar], 40, 60)).toEqual([]);
  });

  it("preserves the order the anchors arrived in", () => {
    const anchors = [anchor({ line: 50 }), anchor({ line: 42 }), anchor({ line: 45 })];
    expect(visibleAnchors(anchors, 40, 60, 0).map((a) => a.line)).toEqual([50, 42, 45]);
  });

  it("never clips a margin below the first line of a document", () => {
    expect(visibleAnchors([anchor({ line: 1 })], 1, 10).map((a) => a.line)).toEqual([1]);
  });

  it("returns nothing for a file that declares nothing", () => {
    expect(visibleAnchors([], 1, 40)).toEqual([]);
  });

  it("decides on the row's line, not on the identifier's", () => {
    // Every other case here has `line === selectionLine`, because the builder
    // defaults one to the other — so those cases pass whichever field the filter
    // reads. A multi-line signature or an attributed member separates them
    // (Roslyn's `range.start` vs `selectionRange.start`), and the row is drawn on
    // `line`, so that is the field visibility must be decided on: an anchor whose
    // row is off screen must not be requested however close its identifier is.
    const rowOffScreen = anchor({ line: 61, selectionLine: 45 });
    expect(visibleAnchors([rowOffScreen], 40, 60, 0)).toEqual([]);
    const rowOnScreen = anchor({ line: 45, selectionLine: 61 });
    expect(visibleAnchors([rowOnScreen], 40, 60, 0)).toEqual([rowOnScreen]);
  });
});

// ---------------------------------------------------------------------------
// 5b. Whether to ask for the anchors again.
// ---------------------------------------------------------------------------

describe("shouldRetryAnchors", () => {
  it("retries only the two outcomes that become a different answer on their own", () => {
    expect(shouldRetryAnchors("starting", 0)).toBe(true);
    expect(shouldRetryAnchors("loading", 0)).toBe(true);
    expect(shouldRetryAnchors("ready", 0)).toBe(false);
    expect(shouldRetryAnchors("notConfigured", 0)).toBe(false);
    expect(shouldRetryAnchors("failed", 0)).toBe(false);
    expect(shouldRetryAnchors("unsupported", 0)).toBe(false);
  });

  it("gives up eventually rather than polling for ever", () => {
    expect(shouldRetryAnchors("loading", ANCHOR_RETRY_LIMIT - 1)).toBe(true);
    expect(shouldRetryAnchors("loading", ANCHOR_RETRY_LIMIT)).toBe(false);
  });

  it("keeps retrying past the backend's own readiness ceiling", () => {
    // `lsp/client.rs::READINESS_CEILING` is 90 s and `session.rs` answers
    // `loading` until then, so a retry chain that ends sooner stops while the
    // backend is still promising a different answer — and the badge would be left
    // saying "loading…" with nothing polling. Strictly greater, with room.
    expect(ANCHOR_RETRY_LIMIT * ANCHOR_RETRY_MS).toBeGreaterThan(READINESS_CEILING_MS);
  });
});

// ---------------------------------------------------------------------------
// 6. The cache key.
// ---------------------------------------------------------------------------

describe("usageCacheKey", () => {
  it("is stable for the same file, anchor and document version", () => {
    expect(usageCacheKey("src/a.ts", "Order.Total@12:8", 3)).toBe(
      usageCacheKey("src/a.ts", "Order.Total@12:8", 3),
    );
  });

  it("is invalidated by an edit, because the document version moved", () => {
    expect(usageCacheKey("src/a.ts", "Order.Total@12:8", 3)).not.toBe(
      usageCacheKey("src/a.ts", "Order.Total@12:8", 4),
    );
  });

  it("survives scrolling, which changes none of its three inputs", () => {
    // There is deliberately no viewport argument: a count is a fact about a
    // symbol in a document, and re-asking because the user scrolled back would
    // spend a whole references query to learn the same number.
    const before = usageCacheKey("src/a.ts", "Order.Total@12:8", 3);
    const afterScrollingAwayAndBack = usageCacheKey("src/a.ts", "Order.Total@12:8", 3);
    expect(afterScrollingAwayAndBack).toBe(before);
  });

  it("separates two anchors in one file", () => {
    expect(usageCacheKey("src/a.ts", "Order.Add@4:8", 1)).not.toBe(
      usageCacheKey("src/a.ts", "Order.Add@4:20", 1),
    );
  });

  it("separates the same anchor id in two files", () => {
    expect(usageCacheKey("src/a.ts", "Order.Add@4:8", 1)).not.toBe(
      usageCacheKey("src/b.ts", "Order.Add@4:8", 1),
    );
  });

  it("cannot be spoofed by a path or an anchor id containing the separator", () => {
    // Concatenating with a printable separator lets one crafted id collide with
    // another anchor's key, which would show one method's count against another.
    expect(usageCacheKey("a", "b:1", 1)).not.toBe(usageCacheKey("a:1", "b", 1));
  });

  it("survives a space in the path and in the anchor id", () => {
    // Both are routine and neither is crafted: Windows workspace paths contain
    // spaces, and a Roslyn anchor id is built from the *raw* symbol name, so a C#
    // id reads `Order.TryGet(ClrObject, int) : bool@12:8`. A space separator makes
    // these two keys the same string.
    expect(usageCacheKey("a", "b c", 1)).not.toBe(usageCacheKey("a b", "c", 1));
    expect(usageCacheKey("src/My App/a.cs", "Order.Add(int, int) : void@4:8", 2)).toBe(
      usageCacheKey("src/My App/a.cs", "Order.Add(int, int) : void@4:8", 2),
    );
  });
});

// ---------------------------------------------------------------------------
// 6a2. What the answer store keeps, and what it goes back for.
// ---------------------------------------------------------------------------

describe("the answer store", () => {
  const key = usageCacheKey("a.cs", "Order.Add@4:8", 1);

  it("keeps a ready answer and never asks again", () => {
    const store = newUsageAnswers();
    recordUsageAnswer(store, key, result({ outcome: "ready", total: 2 }));

    expect(usageStateFor(store, key)).toEqual({
      status: "answered",
      result: result({ outcome: "ready", total: 2 }),
    });
    expect(shouldAskUsages(store, key)).toBe(false);
  });

  it("goes back for a count the server could not give yet", () => {
    // The defect: every answer was cached under the version key, whatever its
    // outcome, and the only thing that cleared the cache was an edit. So a server
    // that died and was restarted (`session.rs` allows one restart a minute) left
    // every row on screen frozen on "Language server loading..." for the life of
    // the tab while the new process answered perfectly, recoverable only by typing.
    for (const outcome of ["starting", "loading", "failed"] as const) {
      const store = newUsageAnswers();
      recordUsageAnswer(store, key, result({ outcome, message: "why" }));

      // Still shown, so the row can say why rather than falling back to "Usages".
      expect(usageStateFor(store, key), outcome).toEqual({
        status: "answered",
        result: result({ outcome, message: "why" }),
      });
      // And asked again, which is the whole difference.
      expect(shouldAskUsages(store, key), outcome).toBe(true);
    }
  });

  it("stops going back once the retries are spent", () => {
    // Bounded for the same reason the anchors chain is: a server that is never
    // coming back must not be asked once per scroll event for ever.
    const store = newUsageAnswers();
    for (let i = 0; i < UNSETTLED_RETRY_LIMIT; i += 1) {
      expect(shouldAskUsages(store, key), `attempt ${i}`).toBe(true);
      recordUsageAnswer(store, key, result({ outcome: "failed" }));
    }
    expect(shouldAskUsages(store, key)).toBe(false);
    expect(usageStateFor(store, key).status).toBe("answered");
  });

  it("does not go back for a server that will never appear", () => {
    // `notConfigured` and `unsupported` are settled facts about the machine, not
    // states that become a different answer on their own.
    for (const outcome of ["notConfigured", "unsupported"] as const) {
      const store = newUsageAnswers();
      recordUsageAnswer(store, key, result({ outcome }));
      expect(shouldAskUsages(store, key), outcome).toBe(false);
    }
  });

  it("reports pending only while a request is out, and idle before anything is asked", () => {
    const store = newUsageAnswers();
    expect(usageStateFor(store, key)).toEqual({ status: "idle" });
    store.inFlight.add(key);
    expect(usageStateFor(store, key)).toEqual({ status: "pending" });
    expect(shouldAskUsages(store, key)).toBe(false);
  });

  it("prefers the pending state to a previous unsettled answer", () => {
    // A retry in flight is honest about the present tense; the stale reason is not.
    const store = newUsageAnswers();
    recordUsageAnswer(store, key, result({ outcome: "loading" }));
    store.inFlight.add(key);
    expect(usageStateFor(store, key)).toEqual({ status: "pending" });
  });

  it("forgets everything when the document changes", () => {
    const store = newUsageAnswers();
    recordUsageAnswer(store, key, result({ outcome: "ready", total: 1 }));
    recordUsageAnswer(store, usageCacheKey("a.cs", "Order.Sum@9:8", 1), result({ outcome: "failed" }));
    store.inFlight.add("whatever");

    clearUsageAnswers(store);
    expect(usageStateFor(store, key)).toEqual({ status: "idle" });
    expect(store.inFlight.size).toBe(0);
    // Including the retry counts: a new version is a new question.
    expect(shouldAskUsages(store, key)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// 6b. What the queue keeps.
// ---------------------------------------------------------------------------

describe("retainUsageJobs", () => {
  const job = (id: string, version: number) => ({
    anchor: anchor({ line: 1, id }),
    key: usageCacheKey("a.ts", id, version),
    version,
  });

  it("keeps only the jobs still worth issuing", () => {
    const onScreen = job("visible", 2);
    const scrolledPast = job("gone", 2);
    const superseded = job("visible", 1);
    const { keep, dropped } = retainUsageJobs(
      [scrolledPast, onScreen, superseded],
      new Set(["visible"]),
      2,
    );
    expect(keep).toEqual([onScreen]);
    // The dropped keys are handed back because the caller holds them in an
    // in-flight set: forgetting one would make that anchor unaskable for the rest
    // of the document version.
    expect(dropped).toEqual([scrolledPast.key, superseded.key]);
  });

  it("preserves the order of what it keeps", () => {
    const a = job("a", 1);
    const b = job("b", 1);
    const { keep } = retainUsageJobs([a, b], new Set(["a", "b"]), 1);
    expect(keep).toEqual([a, b]);
  });

  it("keeps nothing when nothing is visible", () => {
    expect(retainUsageJobs([job("a", 1)], new Set(), 1).keep).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// 6c. Counting, placing, and the two strings a component would otherwise invent.
// ---------------------------------------------------------------------------

describe("usageCountLabel", () => {
  it("says there are none rather than counting zero", () => {
    expect(usageCountLabel(0)).toBe("No usages");
  });

  it("uses the singular for one and the plural for more", () => {
    expect(usageCountLabel(1)).toBe("1 usage");
    expect(usageCountLabel(2)).toBe("2 usages");
    expect(usageCountLabel(900)).toBe("900 usages");
  });

  it("says a provisional count is a floor, keeping the singular and the plural", () => {
    expect(usageCountLabel(1, true)).toBe("at least 1 usage");
    expect(usageCountLabel(2, true)).toBe("at least 2 usages");
  });

  it("refuses to count a provisional zero at all, because zero is not the answer", () => {
    // "at least 0 usages" is true of every possible answer and reads as "none".
    const label = usageCountLabel(0, true);
    expect(label).not.toMatch(/\bno(ne)?\b/i);
    expect(label).not.toMatch(/0/);
    expect(label).toMatch(/unknown/i);
  });
});

describe("countUsageRows", () => {
  it("counts the rows the dropdown will actually list, across groups", () => {
    const groups = groupUsages([
      usage({ label: "a.ts", line: 1 }),
      usage({ label: "a.ts", line: 2 }),
      usage({ label: "b.ts", line: 3 }),
    ]);
    expect(countUsageRows(groups)).toBe(3);
  });

  it("counts nothing for no groups", () => {
    expect(countUsageRows([])).toBe(0);
  });
});

describe("placeMenu", () => {
  const bounds = { left: 100, top: 50, width: 1000, height: 400 };

  it("puts the menu under the point that opened it, in wrapper coordinates", () => {
    expect(placeMenu(300, 200, bounds)).toEqual({ left: 200, top: 154, maxHeight: 238 });
  });

  it("clamps a menu that would run off the right edge", () => {
    const { left } = placeMenu(1080, 200, bounds);
    expect(left).toBe(bounds.width - MENU_WIDTH - 4);
  });

  it("never places a menu at a negative offset, even in a pane narrower than it", () => {
    const narrow = { left: 0, top: 0, width: 300, height: 120 };
    const place = placeMenu(280, 100, narrow);
    expect(place.left).toBe(4);
    expect(place.top).toBeGreaterThanOrEqual(4);
    // Clamped rather than flipped, and never smaller than a readable strip: the
    // menu scrolls inside the pane instead of running off the window.
    expect(place.maxHeight).toBeGreaterThanOrEqual(80);
  });

  it("keeps the menu inside a short pane and lets it scroll there", () => {
    const short = { left: 0, top: 0, width: 1000, height: 200 };
    const place = placeMenu(10, 190, short);
    expect(place.top).toBeLessThanOrEqual(short.height - 80);
    expect(place.maxHeight).toBeGreaterThanOrEqual(80);
  });

  it("falls back to a placeable position when the wrapper has no rectangle yet", () => {
    const place = placeMenu(300, 200, null);
    expect(place.left).toBeGreaterThanOrEqual(0);
    expect(place.top).toBeGreaterThanOrEqual(0);
    expect(place.maxHeight).toBeGreaterThanOrEqual(80);
  });
});

describe("failedUsageResult", () => {
  it("turns a rejected call into a dead server rather than an empty answer", () => {
    const view = usageRowView({ status: "answered", result: failedUsageResult("no workspace") });
    expect(view.total).toBeNull();
    expect(view.tone).toBe("reason");
    expect(view.tooltip).toBe("no workspace");
    expect(view.action).toEqual({ kind: "inert", reason: "no workspace" });
  });
});

describe("INERT_LOCATION_REASON", () => {
  it("explains why a listed location cannot be opened, without denying it exists", () => {
    expect(INERT_LOCATION_REASON).not.toMatch(/\bno\b/i);
    expect(INERT_LOCATION_REASON.length).toBeGreaterThan(20);
  });
});

// ---------------------------------------------------------------------------
// 6d. The two decisions the CodeMirror layer asks for.
// ---------------------------------------------------------------------------

describe("toneClass", () => {
  it("gives each tone its own class, matching the stylesheet's contract", () => {
    const classes = (["idle", "waiting", "count", "empty", "reason"] as const).map(toneClass);
    expect(classes).toEqual([
      "cb-usages-idle",
      "cb-usages-waiting",
      "cb-usages-count",
      "cb-usages-empty",
      "cb-usages-reason",
    ]);
  });
});

describe("actionDetail", () => {
  it("separates two dropdown rows whose counts differ", () => {
    // Half of the widget's `eq`: comparing equal here keeps the old widget
    // instance alive, and a click then hands the host the previous count.
    const a = usageRowView(answered({ outcome: "ready", total: 3 }));
    const b = usageRowView(answered({ outcome: "ready", total: 4 }));
    expect(actionDetail(a)).not.toBe(actionDetail(b));
  });

  it("separates two inert rows whose reasons differ", () => {
    const a = usageRowView(answered({ outcome: "failed", message: "exit code 134" }));
    const b = usageRowView(answered({ outcome: "failed", message: "exit code 1" }));
    expect(actionDetail(a)).not.toBe(actionDetail(b));
  });

  it("compares equal for two rows that say and do the same thing", () => {
    const a = usageRowView({ status: "idle" });
    const b = usageRowView({ status: "idle" });
    expect(actionDetail(a)).toBe(actionDetail(b));
  });
});

// ---------------------------------------------------------------------------
// 7. Highlight slicing.
// ---------------------------------------------------------------------------

describe("snippetParts", () => {
  it("cuts the snippet into the three pieces around the highlight", () => {
    expect(snippetParts("const total = sum(x)", { start: 6, end: 11 })).toEqual({
      before: "const ",
      match: "total",
      after: " = sum(x)",
    });
  });

  it("underlines nothing when the match did not survive trimming", () => {
    expect(snippetParts("const total = 1", null)).toEqual({
      before: "const total = 1",
      match: "",
      after: "",
    });
  });

  it("slices by utf-16 code units, so a non-ascii prefix does not shift the match", () => {
    // "héllo " is six characters and six UTF-16 code units, but seven bytes.
    expect(snippetParts("héllo total", { start: 6, end: 11 })).toEqual({
      before: "héllo ",
      match: "total",
      after: "",
    });
  });

  it("counts an astral character as the two code units it is", () => {
    // A single emoji is one code point and two UTF-16 code units, which is what
    // the backend converted to. Reading these as code points would land the
    // match one short and cut the surrogate pair in half.
    const snippet = "🎺 total";
    // Two code units for the emoji, one for the space, five for the word.
    expect(snippet.length).toBe(8);
    expect(Array.from(snippet)).toHaveLength(7);
    expect(snippetParts(snippet, { start: 3, end: 8 })).toEqual({
      before: "🎺 ",
      match: "total",
      after: "",
    });
  });

  it("clamps a highlight whose end runs past the snippet rather than throwing", () => {
    expect(snippetParts("total", { start: 0, end: 99 })).toEqual({
      before: "",
      match: "total",
      after: "",
    });
  });

  it("clamps a start past the end of the snippet to an empty match", () => {
    expect(snippetParts("total", { start: 99, end: 120 })).toEqual({
      before: "total",
      match: "",
      after: "",
    });
  });

  it("clamps a negative start to the beginning", () => {
    expect(snippetParts("total", { start: -4, end: 2 })).toEqual({
      before: "",
      match: "to",
      after: "tal",
    });
  });

  it("yields an empty match for an inverted span rather than reordering it", () => {
    expect(snippetParts("total", { start: 3, end: 1 })).toEqual({
      before: "tot",
      match: "",
      after: "al",
    });
  });

  it("treats a non-numeric span as no highlight at all", () => {
    expect(snippetParts("total", { start: Number.NaN, end: 2 })).toEqual({
      before: "total",
      match: "",
      after: "",
    });
  });

  it("handles an empty snippet", () => {
    expect(snippetParts("", { start: 0, end: 3 })).toEqual({
      before: "",
      match: "",
      after: "",
    });
  });
});
