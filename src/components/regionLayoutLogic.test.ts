import { describe, expect, it } from "vitest";
import {
  centerTabIds,
  clampRegionSize,
  dockInto,
  dockedTerminalIds,
  DEFAULT_REGION_SIZE,
  edgeHitTest,
  hasRegions,
  loadRegionLayout,
  MIN_REGION_FRACTION,
  pruneLayout,
  regionLayoutKey,
  removeDockable,
  resizeRegion,
  saveRegionLayout,
  setRegionActive,
  type Dockable,
  type RegionLayout,
} from "./regionLayoutLogic";

const tab = (id: string): Dockable => ({ kind: "tab", id });
const term = (id: string): Dockable => ({ kind: "terminal", id });
const RECT = { left: 0, top: 0, width: 1000, height: 800 };

describe("edgeHitTest", () => {
  it("classifies each edge band and the center", () => {
    expect(edgeHitTest({ x: 10, y: 400 }, RECT)).toBe("left");
    expect(edgeHitTest({ x: 990, y: 400 }, RECT)).toBe("right");
    expect(edgeHitTest({ x: 500, y: 10 }, RECT)).toBe("top");
    expect(edgeHitTest({ x: 500, y: 790 }, RECT)).toBe("bottom");
    expect(edgeHitTest({ x: 500, y: 400 }, RECT)).toBe("center");
  });

  it("prefers left/right over top/bottom in a corner", () => {
    // Top-left corner: inside both the left band and the top band.
    expect(edgeHitTest({ x: 10, y: 10 }, RECT)).toBe("left");
  });

  it("treats a point outside the rect, or a degenerate rect, as center", () => {
    expect(edgeHitTest({ x: -5, y: 400 }, RECT)).toBe("center");
    expect(edgeHitTest({ x: 10, y: 10 }, { left: 0, top: 0, width: 0, height: 0 })).toBe("center");
  });
});

describe("dockInto / removeDockable", () => {
  it("creates a region and makes the dropped item active", () => {
    const layout = dockInto({}, tab("a"), "right");
    expect(layout.right?.items).toEqual([tab("a")]);
    expect(layout.right?.active).toBe(0);
    expect(layout.right?.size).toBe(DEFAULT_REGION_SIZE);
  });

  it("appends to an existing region's stack and activates the new item", () => {
    let layout = dockInto({}, tab("a"), "right");
    layout = dockInto(layout, tab("b"), "right");
    expect(layout.right?.items).toEqual([tab("a"), tab("b")]);
    expect(layout.right?.active).toBe(1);
  });

  it("moves a dockable out of its old region when docked elsewhere", () => {
    let layout = dockInto({}, tab("a"), "right");
    layout = dockInto(layout, tab("a"), "left");
    expect(layout.right).toBeUndefined();
    expect(layout.left?.items).toEqual([tab("a")]);
  });

  it("dropping on center undocks without creating a region", () => {
    let layout = dockInto({}, tab("a"), "right");
    layout = dockInto(layout, tab("a"), "center");
    expect(hasRegions(layout)).toBe(false);
  });

  it("deletes a region emptied by removal and re-clamps active", () => {
    let layout = dockInto({}, tab("a"), "left");
    layout = dockInto(layout, tab("b"), "left"); // active = 1
    layout = removeDockable(layout, tab("b"));
    expect(layout.left?.items).toEqual([tab("a")]);
    expect(layout.left?.active).toBe(0);

    layout = removeDockable(layout, tab("a"));
    expect(layout.left).toBeUndefined();
  });
});

describe("setRegionActive", () => {
  it("changes the active index, ignoring out-of-range", () => {
    let layout = dockInto(dockInto({}, tab("a"), "top"), tab("b"), "top");
    layout = setRegionActive(layout, "top", 0);
    expect(layout.top?.active).toBe(0);
    expect(setRegionActive(layout, "top", 9)).toBe(layout);
    expect(setRegionActive(layout, "bottom", 0)).toBe(layout);
  });
});

describe("clampRegionSize / resizeRegion", () => {
  it("clamps to the min/max band", () => {
    expect(clampRegionSize(0.01)).toBe(MIN_REGION_FRACTION);
    expect(clampRegionSize(0.99)).toBe(1 - MIN_REGION_FRACTION);
    expect(clampRegionSize(Number.NaN)).toBe(DEFAULT_REGION_SIZE);
  });

  it("grows a left/top region moving away from its edge and shrinks a right/bottom one", () => {
    // 200px of a 1000px container is 0.2.
    expect(resizeRegion("left", 0.3, 200, 1000)).toBeCloseTo(0.5);
    expect(resizeRegion("right", 0.3, 200, 1000)).toBeCloseTo(0.1 < MIN_REGION_FRACTION ? MIN_REGION_FRACTION : 0.1);
    expect(resizeRegion("top", 0.3, -100, 800)).toBeCloseTo(0.175);
    expect(resizeRegion("bottom", 0.3, -100, 800)).toBeCloseTo(0.425);
  });

  it("guards a zero-size container", () => {
    expect(resizeRegion("left", 0.3, 200, 0)).toBe(clampRegionSize(0.3));
  });
});

describe("centerTabIds / dockedTerminalIds", () => {
  const layout: RegionLayout = dockInto(dockInto({}, tab("b"), "left"), term("t1"), "bottom");

  it("returns the open tabs no region claims, in order", () => {
    expect(centerTabIds(["a", "b", "c"], layout)).toEqual(["a", "c"]);
  });

  it("collects docked terminal keys", () => {
    expect([...dockedTerminalIds(layout)]).toEqual(["t1"]);
  });
});

describe("pruneLayout", () => {
  it("drops references to closed tabs and exited terminals", () => {
    let layout = dockInto({}, tab("gone"), "right");
    layout = dockInto(layout, tab("kept"), "right");
    layout = dockInto(layout, term("dead"), "top");
    const pruned = pruneLayout(layout, new Set(["kept"]), new Set());
    expect(pruned.right?.items).toEqual([tab("kept")]);
    expect(pruned.top).toBeUndefined();
  });
});

describe("persistence", () => {
  function memStorage(): Storage {
    const map = new Map<string, string>();
    return {
      get length() {
        return map.size;
      },
      clear: () => map.clear(),
      getItem: (k) => map.get(k) ?? null,
      key: () => null,
      removeItem: (k) => map.delete(k),
      setItem: (k, v) => void map.set(k, v),
    };
  }

  it("keys per root, percent-encoded", () => {
    expect(regionLayoutKey("C:\\a\\b")).toBe("cb.regions.layout:C%3A%5Ca%5Cb");
  });

  it("round-trips a layout", () => {
    const storage = memStorage();
    const layout = dockInto({}, tab("a"), "right");
    saveRegionLayout(storage, layout, "k");
    expect(loadRegionLayout(storage, "k")).toEqual(layout);
  });

  it("returns {} for malformed or absent storage", () => {
    const storage = memStorage();
    expect(loadRegionLayout(storage, "missing")).toEqual({});
    storage.setItem("bad", "{ not json");
    expect(loadRegionLayout(storage, "bad")).toEqual({});
    storage.setItem("shape", JSON.stringify({ left: { items: "no" } }));
    expect(loadRegionLayout(storage, "shape")).toEqual({});
  });

  it("clamps a stored size and drops an empty region on load", () => {
    const storage = memStorage();
    storage.setItem(
      "k",
      JSON.stringify({
        left: { items: [tab("a")], active: 5, size: 0.99 },
        right: { items: [], active: 0, size: 0.3 },
      }),
    );
    const loaded = loadRegionLayout(storage, "k");
    expect(loaded.left?.active).toBe(0);
    expect(loaded.left?.size).toBe(1 - MIN_REGION_FRACTION);
    expect(loaded.right).toBeUndefined();
  });
});
