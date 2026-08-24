import { describe, expect, it } from "vitest";
import {
  navBack,
  navForward,
  navMouseAction,
  partitionTabs,
  pushNav,
  togglePin,
  type NavHistory,
} from "./editorNavLogic";

/** A history sitting at the end of its entries, as a fresh `pushNav` leaves it. */
function history(paths: string[]): NavHistory {
  return { entries: paths.map((path) => ({ path })), index: paths.length - 1 };
}

describe("pushNav", () => {
  it("appends to an empty history and points at the new entry", () => {
    const next = pushNav({ entries: [], index: -1 }, { path: "a" });
    expect(next).toEqual({ entries: [{ path: "a" }], index: 0 });
  });

  it("advances the index as entries are appended", () => {
    let h: NavHistory = { entries: [], index: -1 };
    h = pushNav(h, { path: "a" });
    h = pushNav(h, { path: "b" });
    h = pushNav(h, { path: "c" });
    expect(h.entries.map((e) => e.path)).toEqual(["a", "b", "c"]);
    expect(h.index).toBe(2);
  });

  it("no-ops when the new entry equals the current one (same path and line)", () => {
    const h = pushNav({ entries: [], index: -1 }, { path: "a", line: 5 });
    const again = pushNav(h, { path: "a", line: 5 });
    expect(again).toBe(h);
  });

  it("does record when only the line differs on the same path", () => {
    let h = pushNav({ entries: [], index: -1 }, { path: "a", line: 5 });
    h = pushNav(h, { path: "a", line: 9 });
    expect(h.entries).toEqual([
      { path: "a", line: 5 },
      { path: "a", line: 9 },
    ]);
    expect(h.index).toBe(1);
  });

  it("treats a missing line and an explicit line as different entries", () => {
    let h = pushNav({ entries: [], index: -1 }, { path: "a" });
    h = pushNav(h, { path: "a", line: 1 });
    expect(h.entries.length).toBe(2);
  });

  it("truncates the forward entries after a back-then-push", () => {
    // a -> b -> c, step back to b, then open d: c is discarded.
    let h = history(["a", "b", "c"]);
    h = navBack(h)!.history; // now at b (index 1)
    h = pushNav(h, { path: "d" });
    expect(h.entries.map((e) => e.path)).toEqual(["a", "b", "d"]);
    expect(h.index).toBe(2);
  });

  it("caps the length by evicting from the front and keeps the index valid", () => {
    let h: NavHistory = { entries: [], index: -1 };
    for (const path of ["a", "b", "c", "d"]) h = pushNav(h, { path }, 3);
    expect(h.entries.map((e) => e.path)).toEqual(["b", "c", "d"]);
    expect(h.index).toBe(2);
    // The index still addresses the last entry after eviction.
    expect(h.entries[h.index]?.path).toBe("d");
  });
});

describe("navBack / navForward", () => {
  it("navBack returns the previous entry and moves the index", () => {
    const result = navBack(history(["a", "b", "c"]));
    expect(result?.entry).toEqual({ path: "b" });
    expect(result?.history.index).toBe(1);
  });

  it("navBack returns null at the start of history", () => {
    expect(navBack({ entries: [{ path: "a" }], index: 0 })).toBeNull();
    expect(navBack({ entries: [], index: -1 })).toBeNull();
  });

  it("navForward returns the next entry and moves the index", () => {
    const back = navBack(history(["a", "b", "c"]))!; // at b, index 1
    const fwd = navForward(back.history);
    expect(fwd?.entry).toEqual({ path: "c" });
    expect(fwd?.history.index).toBe(2);
  });

  it("navForward returns null at the end of history", () => {
    expect(navForward(history(["a", "b"]))).toBeNull();
  });

  it("round-trips back then forward to the same entry", () => {
    const start = history(["a", "b", "c"]);
    const back = navBack(start)!;
    const fwd = navForward(back.history)!;
    expect(fwd.entry).toEqual({ path: "c" });
    expect(fwd.history).toEqual(start);
  });
});

describe("navMouseAction", () => {
  it("maps button 3 to back and button 4 to forward", () => {
    expect(navMouseAction(3)).toBe("back");
    expect(navMouseAction(4)).toBe("forward");
  });

  it("ignores the primary, middle and secondary buttons", () => {
    expect(navMouseAction(0)).toBeNull();
    expect(navMouseAction(1)).toBeNull();
    expect(navMouseAction(2)).toBeNull();
  });
});

describe("partitionTabs", () => {
  const files = [
    { path: "a", name: "a" },
    { path: "b", name: "b" },
    { path: "c", name: "c" },
  ];

  it("splits into pinned and unpinned, preserving order within each group", () => {
    const { pinned, unpinned } = partitionTabs(files, new Set(["c", "a"]));
    expect(pinned.map((f) => f.path)).toEqual(["a", "c"]);
    expect(unpinned.map((f) => f.path)).toEqual(["b"]);
  });

  it("puts everything in the unpinned group when nothing is pinned", () => {
    const { pinned, unpinned } = partitionTabs(files, new Set());
    expect(pinned).toEqual([]);
    expect(unpinned).toEqual(files);
  });

  it("ignores pinned paths that are not open", () => {
    const { pinned, unpinned } = partitionTabs(files, new Set(["z"]));
    expect(pinned).toEqual([]);
    expect(unpinned.map((f) => f.path)).toEqual(["a", "b", "c"]);
  });
});

describe("togglePin", () => {
  it("adds a path that was not pinned", () => {
    const next = togglePin(new Set(["a"]), "b");
    expect([...next].sort()).toEqual(["a", "b"]);
  });

  it("removes a path that was pinned", () => {
    const next = togglePin(new Set(["a", "b"]), "a");
    expect([...next]).toEqual(["b"]);
  });

  it("does not mutate the input set", () => {
    const input = new Set(["a"]);
    togglePin(input, "b");
    expect([...input]).toEqual(["a"]);
  });
});
