import { describe, expect, it } from "vitest";
import {
  dockId,
  removeEntry,
  upsertEntry,
  visibleEntries,
  type DockEntry,
} from "./dockLogic";

function entry(over: Partial<DockEntry> & Pick<DockEntry, "id" | "scope" | "order">): DockEntry {
  return { label: over.id, ...over };
}

describe("dockId", () => {
  it("namespaces a panel-local id by scope so two codebases do not collide", () => {
    expect(dockId("/a", "term-1")).toBe("/a:term-1");
    expect(dockId("/b", "term-1")).toBe("/b:term-1");
    expect(dockId("/a", "term-1")).not.toBe(dockId("/b", "term-1"));
  });
});

describe("upsertEntry", () => {
  it("appends a new entry and leaves the existing ones in order", () => {
    const a = entry({ id: "a", scope: "/w", order: 1 });
    const b = entry({ id: "b", scope: "/w", order: 2 });
    expect(upsertEntry([a], b)).toEqual([a, b]);
  });

  it("updates an existing entry in place without moving it", () => {
    const a = entry({ id: "a", scope: "/w", order: 1 });
    const b = entry({ id: "b", scope: "/w", order: 2 });
    const bFlashing = entry({ id: "b", scope: "/w", order: 2, attention: true });
    const next = upsertEntry([a, b], bFlashing);
    expect(next).toEqual([a, bFlashing]);
    expect(next[1]?.attention).toBe(true);
    // Position preserved: b stays second.
    expect(next.map((e) => e.id)).toEqual(["a", "b"]);
  });
});

describe("removeEntry", () => {
  it("drops the matching id", () => {
    const a = entry({ id: "a", scope: "/w", order: 1 });
    const b = entry({ id: "b", scope: "/w", order: 2 });
    expect(removeEntry([a, b], "a")).toEqual([b]);
  });

  it("returns the same array reference when the id is absent", () => {
    const list = [entry({ id: "a", scope: "/w", order: 1 })];
    expect(removeEntry(list, "nope")).toBe(list);
  });
});

describe("visibleEntries", () => {
  it("includes global and the active root, excludes other roots", () => {
    const notes = entry({ id: "notes", scope: "global", order: 0 });
    const here = entry({ id: "here", scope: "/a", order: 1 });
    const elsewhere = entry({ id: "elsewhere", scope: "/b", order: 2 });
    expect(visibleEntries([notes, here, elsewhere], "/a")).toEqual([notes, here]);
  });

  it("puts pinned entries first, then orders by ascending order", () => {
    const t2 = entry({ id: "t2", scope: "/a", order: 2 });
    const t1 = entry({ id: "t1", scope: "/a", order: 1 });
    const notes = entry({ id: "notes", scope: "global", order: 99, pinned: true });
    expect(visibleEntries([t2, t1, notes], "/a").map((e) => e.id)).toEqual([
      "notes",
      "t1",
      "t2",
    ]);
  });

  it("is stable for entries sharing an order", () => {
    const first = entry({ id: "first", scope: "/a", order: 1 });
    const second = entry({ id: "second", scope: "/a", order: 1 });
    expect(visibleEntries([first, second], "/a").map((e) => e.id)).toEqual(["first", "second"]);
  });

  it("carries color/attention/status/spinner untouched", () => {
    const rich = entry({
      id: "t",
      scope: "/a",
      order: 1,
      color: "#f00",
      attention: true,
      status: "running",
      spinner: true,
    });
    expect(visibleEntries([rich], "/a")[0]).toEqual(rich);
  });

  it("shows nothing when no codebase is active except globals", () => {
    const notes = entry({ id: "notes", scope: "global", order: 0 });
    const here = entry({ id: "here", scope: "/a", order: 1 });
    expect(visibleEntries([notes, here], null)).toEqual([notes]);
  });
});
