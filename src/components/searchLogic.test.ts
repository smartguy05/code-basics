import { describe, expect, it } from "vitest";
import {
  SHIFT_WINDOW_MS,
  actionableIds,
  dropUnactionable,
  type ShortcutEvent,
  groupHits,
  highlightSpans,
  indexNote,
  lineToPos,
  nextIndex,
  recogniseShortcut,
  resultsState,
  searchKey,
} from "./searchLogic";
import type { SymbolIndexStatus } from "../ipc/types";

/** A keydown with no modifiers held; each test overrides only what it means. */
function key(over: Partial<ShortcutEvent> & { key: string }): ShortcutEvent {
  return {
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...over,
  };
}

describe("recogniseShortcut", () => {
  it("opens everything when a second Shift lands inside the window", () => {
    expect(recogniseShortcut(key({ key: "Shift", shiftKey: true }), 1000, 1200)).toBe(
      "all",
    );
  });

  it("opens everything when the second Shift lands exactly on the window edge", () => {
    expect(
      recogniseShortcut(key({ key: "Shift", shiftKey: true }), 1000, 1000 + SHIFT_WINDOW_MS),
    ).toBe("all");
  });

  it("ignores a second Shift that arrives after the window has closed", () => {
    expect(
      recogniseShortcut(
        key({ key: "Shift", shiftKey: true }),
        1000,
        1000 + SHIFT_WINDOW_MS + 1,
      ),
    ).toBeNull();
  });

  it("ignores the first Shift of all, when there is no previous one", () => {
    expect(recogniseShortcut(key({ key: "Shift", shiftKey: true }), null, 1000)).toBeNull();
  });

  it("ignores a Shift that arrives before the recorded one, so a clock jump cannot fire it", () => {
    expect(recogniseShortcut(key({ key: "Shift", shiftKey: true }), 2000, 1000)).toBeNull();
  });

  it("never treats a Shift held with another modifier as a bare Shift press", () => {
    for (const held of ["ctrlKey", "altKey", "metaKey"] as const) {
      expect(
        recogniseShortcut(key({ key: "Shift", shiftKey: true, [held]: true }), 1000, 1100),
      ).toBeNull();
    }
  });

  it("searches symbols on Ctrl+N", () => {
    expect(recogniseShortcut(key({ key: "n", ctrlKey: true }), null, 0)).toBe("symbols");
  });

  it("searches files on Ctrl+Shift+N rather than symbols", () => {
    expect(
      recogniseShortcut(key({ key: "N", ctrlKey: true, shiftKey: true }), null, 0),
    ).toBe("files");
  });

  it("searches actions on Ctrl+Shift+A", () => {
    expect(
      recogniseShortcut(key({ key: "A", ctrlKey: true, shiftKey: true }), null, 0),
    ).toBe("actions");
  });

  it("leaves Ctrl+A alone, because select-all belongs to whatever has focus", () => {
    expect(recogniseShortcut(key({ key: "a", ctrlKey: true }), null, 0)).toBeNull();
  });

  it("leaves Ctrl+F alone, because the console's find bar owns it", () => {
    expect(recogniseShortcut(key({ key: "f", ctrlKey: true }), null, 0)).toBeNull();
    expect(
      recogniseShortcut(key({ key: "F", ctrlKey: true, shiftKey: true }), null, 0),
    ).toBeNull();
  });

  it("ignores a letter typed with no modifier at all", () => {
    expect(recogniseShortcut(key({ key: "n" }), null, 0)).toBeNull();
    expect(recogniseShortcut(key({ key: "a" }), null, 0)).toBeNull();
  });

  it("ignores a binding letter pressed with Alt or with the meta key instead of Ctrl", () => {
    expect(recogniseShortcut(key({ key: "n", altKey: true }), null, 0)).toBeNull();
    expect(recogniseShortcut(key({ key: "n", metaKey: true }), null, 0)).toBeNull();
    expect(
      recogniseShortcut(key({ key: "n", ctrlKey: true, altKey: true }), null, 0),
    ).toBeNull();
  });
});

describe("groupHits", () => {
  const hit = (kind: "file" | "symbol" | "action", label: string) => ({ kind, label });

  it("puts the sections in a fixed order regardless of the order the hits arrive in", () => {
    const sections = groupHits([
      hit("action", "Run Api"),
      hit("symbol", "Foo"),
      hit("file", "foo.ts"),
    ]);
    expect(sections.map((s) => s.kind)).toEqual(["file", "symbol", "action"]);
    expect(sections.map((s) => s.title)).toEqual(["Files", "Symbols", "Actions"]);
  });

  it("keeps the backend's rank order inside each section", () => {
    const sections = groupHits([
      hit("symbol", "best"),
      hit("file", "a.ts"),
      hit("symbol", "second"),
      hit("file", "b.ts"),
      hit("symbol", "third"),
    ]);
    expect(sections.map((s) => s.hits.map((h) => h.label))).toEqual([
      ["a.ts", "b.ts"],
      ["best", "second", "third"],
    ]);
  });

  it("omits a section that has no hits rather than drawing an empty heading", () => {
    const sections = groupHits([hit("action", "Run Api")]);
    expect(sections.map((s) => s.kind)).toEqual(["action"]);
  });

  it("returns nothing at all for no hits", () => {
    expect(groupHits([])).toEqual([]);
  });
});

describe("nextIndex", () => {
  it("moves down and up by one", () => {
    expect(nextIndex(0, 1, 3)).toBe(1);
    expect(nextIndex(2, -1, 3)).toBe(1);
  });

  it("wraps past the end back to the top", () => {
    expect(nextIndex(2, 1, 3)).toBe(0);
  });

  it("wraps past the top back to the end", () => {
    expect(nextIndex(0, -1, 3)).toBe(2);
  });

  it("stays at zero when there is nothing to move through", () => {
    expect(nextIndex(0, 1, 0)).toBe(0);
    expect(nextIndex(0, -1, 0)).toBe(0);
    expect(nextIndex(5, -3, 0)).toBe(0);
  });

  it("lands inside the list even when the delta is bigger than the list", () => {
    expect(nextIndex(0, 7, 3)).toBe(1);
    expect(nextIndex(0, -7, 3)).toBe(2);
  });

  it("brings a stale index that outran a shrunken list back into range", () => {
    expect(nextIndex(9, 1, 3)).toBe(1);
  });
});

describe("highlightSpans", () => {
  it("splits a label into the matched and unmatched runs around one position", () => {
    expect(highlightSpans("abc", [1])).toEqual([
      { text: "a", hit: false },
      { text: "b", hit: true },
      { text: "c", hit: false },
    ]);
  });

  it("merges adjacent positions into a single span", () => {
    expect(highlightSpans("abcd", [1, 2])).toEqual([
      { text: "a", hit: false },
      { text: "bc", hit: true },
      { text: "d", hit: false },
    ]);
  });

  it("merges a run that starts at the first character and one that ends at the last", () => {
    expect(highlightSpans("abcd", [0, 1, 3])).toEqual([
      { text: "ab", hit: true },
      { text: "c", hit: false },
      { text: "d", hit: true },
    ]);
  });

  it("returns one unhighlighted span when nothing matched", () => {
    expect(highlightSpans("abc", [])).toEqual([{ text: "abc", hit: false }]);
  });

  it("returns nothing at all for an empty label", () => {
    expect(highlightSpans("", [])).toEqual([]);
    expect(highlightSpans("", [0])).toEqual([]);
  });

  it("treats positions as character indices, so an emoji does not shift the rest", () => {
    const label = "a🙂bc";
    // Characters: a, 🙂, b, c — `b` is character 2, not string index 2.
    expect(highlightSpans(label, [2])).toEqual([
      { text: "a🙂", hit: false },
      { text: "b", hit: true },
      { text: "c", hit: false },
    ]);
    expect(
      highlightSpans(label, [2])
        .map((s) => s.text)
        .join(""),
    ).toBe(label);
  });

  it("reconstructs an accented label exactly, whatever the positions", () => {
    const label = "café_ñandú";
    for (const positions of [[], [0], [3, 4], [0, 9], [1, 2, 3, 6]]) {
      expect(
        highlightSpans(label, positions)
          .map((s) => s.text)
          .join(""),
      ).toBe(label);
    }
  });

  it("ignores positions past the end of the label instead of throwing", () => {
    expect(highlightSpans("ab", [1, 5, 99])).toEqual([
      { text: "a", hit: false },
      { text: "b", hit: true },
    ]);
  });

  it("ignores negative and non-integer positions", () => {
    expect(highlightSpans("abc", [-1, 1.5])).toEqual([{ text: "abc", hit: false }]);
  });

  it("tolerates repeated and unsorted positions", () => {
    expect(highlightSpans("abcd", [2, 1, 1])).toEqual([
      { text: "a", hit: false },
      { text: "bc", hit: true },
      { text: "d", hit: false },
    ]);
  });
});

describe("lineToPos", () => {
  it("returns the line unchanged when it is inside the document", () => {
    expect(lineToPos(10, 4)).toBe(4);
  });

  it("returns the first line for exactly one and for anything below it", () => {
    expect(lineToPos(10, 1)).toBe(1);
    expect(lineToPos(10, 0)).toBe(1);
    expect(lineToPos(10, -5)).toBe(1);
  });

  it("returns the last line for exactly the total and for anything above it", () => {
    expect(lineToPos(10, 10)).toBe(10);
    expect(lineToPos(10, 11)).toBe(10);
    expect(lineToPos(10, 10_000)).toBe(10);
  });

  it("rounds a non-integer line down to the line it falls inside", () => {
    expect(lineToPos(10, 4.9)).toBe(4);
    expect(lineToPos(10, 0.5)).toBe(1);
  });

  it("answers the first line for a document that claims to have none", () => {
    expect(lineToPos(0, 3)).toBe(1);
    expect(lineToPos(-1, 3)).toBe(1);
  });

  it("answers the first line for NaN and the last line for an infinite line", () => {
    expect(lineToPos(10, Number.NaN)).toBe(1);
    expect(lineToPos(10, Number.POSITIVE_INFINITY)).toBe(10);
    expect(lineToPos(10, Number.NEGATIVE_INFINITY)).toBe(1);
  });

  // Characterisation, not a guarantee. `Number.isNaN(undefined)` is false, so
  // undefined is not caught by the NaN guard and comes back out as NaN; the
  // reason that is not a bug is that the parameter is typed `number` and the
  // only caller narrows against null first. Pinned so that anyone tempted to
  // widen the signature sees what widening it would cost.
  it("does not defend against a non-number smuggled past the type checker", () => {
    expect(lineToPos(10, undefined as unknown as number)).toBeNaN();
    expect(lineToPos(10, "abc" as unknown as number)).toBeNaN();
    expect(lineToPos(10, null as unknown as number)).toBe(1);
  });
});

describe("actionableIds", () => {
  it("keeps the ids of application configurations, which the Run tab can select", () => {
    const ids = actionableIds([
      { id: "a", kind: "app" },
      { id: "b", kind: "app" },
    ]);
    expect([...ids].sort()).toEqual(["a", "b"]);
  });

  it("leaves out a test configuration, because no consumer of an action hit selects one", () => {
    expect([...actionableIds([{ id: "t", kind: "test" }])]).toEqual([]);
  });

  it("answers an empty set for a workspace with no configurations at all", () => {
    expect(actionableIds([]).size).toBe(0);
  });
});

describe("dropUnactionable", () => {
  const file = { kind: "file" as const, actionId: null };
  const symbol = { kind: "symbol" as const, actionId: null };
  const app = { kind: "action" as const, actionId: "a" };
  const test = { kind: "action" as const, actionId: "t" };

  it("keeps an action hit whose configuration the consumer can select", () => {
    expect(dropUnactionable([app], new Set(["a"]))).toEqual([app]);
  });

  it("drops an action hit for a configuration the consumer would silently ignore", () => {
    expect(dropUnactionable([app, test], new Set(["a"]))).toEqual([app]);
  });

  it("drops an action hit that carries no configuration id at all", () => {
    expect(dropUnactionable([{ kind: "action" as const, actionId: null }], new Set(["a"]))).toEqual(
      [],
    );
  });

  it("never touches file and symbol hits, which are acted on by a different route", () => {
    expect(dropUnactionable([file, symbol], new Set())).toEqual([file, symbol]);
  });

  it("drops every action hit while the actionable set is still unknown", () => {
    expect(dropUnactionable([file, app, test], null)).toEqual([file]);
  });

  it("preserves the backend's order among the hits it keeps", () => {
    const other = { kind: "action" as const, actionId: "b" };
    expect(dropUnactionable([other, test, app], new Set(["a", "b"]))).toEqual([other, app]);
  });
});

describe("searchKey", () => {
  it("gives the same key to the same scope and query", () => {
    expect(searchKey("files", "api")).toBe(searchKey("files", "api"));
  });

  it("gives different keys to the same query under two scopes", () => {
    expect(searchKey("files", "api")).not.toBe(searchKey("actions", "api"));
  });

  it("cannot be made to collide by a query that looks like a scope boundary", () => {
    expect(searchKey("all", "files:api")).not.toBe(searchKey("files", "api"));
  });
});

describe("resultsState", () => {
  it("asks for a query when the box is empty, whatever else is around", () => {
    expect(resultsState("", null, searchKey("all", ""), 0)).toBe("prompt");
    expect(resultsState("   ", searchKey("all", "old"), searchKey("all", "   "), 5)).toBe(
      "prompt",
    );
  });

  it("is pending while nothing has come back for the query on display", () => {
    expect(resultsState("api", null, searchKey("all", "api"), 0)).toBe("pending");
  });

  it("is pending when the hits in hand were answered for a different scope", () => {
    expect(resultsState("api", searchKey("actions", "api"), searchKey("files", "api"), 3)).toBe(
      "pending",
    );
  });

  it("is pending when the hits in hand were answered for a shorter query", () => {
    expect(resultsState("apip", searchKey("all", "api"), searchKey("all", "apip"), 3)).toBe(
      "pending",
    );
  });

  it("reports no matches only once this exact search has answered with none", () => {
    const key = searchKey("all", "api");
    expect(resultsState("api", key, key, 0)).toBe("empty");
  });

  it("shows hits when they belong to the search being displayed", () => {
    const key = searchKey("all", "api");
    expect(resultsState("api", key, key, 3)).toBe("hits");
  });
});

describe("indexNote", () => {
  const status = (over: Partial<SymbolIndexStatus> = {}): SymbolIndexStatus => ({
    ready: false,
    building: false,
    files: 0,
    symbols: 0,
    truncated: false,
    ...over,
  });

  it("says run configurations still match while the first index is being built", () => {
    const note = indexNote(status({ building: true, ready: false }));
    expect(note).not.toBeNull();
    expect(note).toMatch(/run configuration/i);
    // The backend searches configurations against an empty index from the first
    // millisecond, so claiming there is nothing yet contradicts the rows on screen.
    expect(note).not.toMatch(/no results yet/i);
  });

  it("warns that a rebuild may be showing incomplete results", () => {
    expect(indexNote(status({ building: true, ready: true }))).toMatch(/incomplete/i);
  });

  it("explains what can still match when no index was ever built", () => {
    expect(indexNote(status({ ready: false, building: false }))).toMatch(/run configuration/i);
  });

  it("reports a capped index with its counts", () => {
    const note = indexNote(status({ ready: true, truncated: true, files: 50000, symbols: 200000 }));
    expect(note).toContain("50000");
    expect(note).toContain("200000");
  });

  it("says nothing at all about a complete index", () => {
    expect(indexNote(status({ ready: true }))).toBeNull();
    expect(indexNote(null)).toBeNull();
  });
});
