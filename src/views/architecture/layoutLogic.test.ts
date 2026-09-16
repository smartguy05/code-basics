import { describe, expect, it } from "vitest";
import { layout } from "./layoutLogic";

/** Every placed coordinate is a finite number. */
function allFinite(result: ReturnType<typeof layout>): boolean {
  for (const { x, y } of result.positions.values()) {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return false;
  }
  const { x, y, width, height } = result.bounds;
  return [x, y, width, height].every(Number.isFinite);
}

describe("layout", () => {
  it("places nothing and returns an empty box for an empty graph", () => {
    const result = layout([], []);
    expect(result.positions.size).toBe(0);
    expect(result.bounds).toEqual({ x: 0, y: 0, width: 0, height: 0 });
  });

  it("places every node, once", () => {
    const nodes = [{ id: "a" }, { id: "b" }, { id: "c" }];
    const result = layout(nodes, [{ from: "a", to: "b" }]);
    expect([...result.positions.keys()].sort()).toEqual(["a", "b", "c"]);
  });

  it("produces only finite coordinates", () => {
    const result = layout(
      [{ id: "a" }, { id: "b" }, { id: "c" }],
      [
        { from: "a", to: "b" },
        { from: "b", to: "c" },
      ],
    );
    expect(allFinite(result)).toBe(true);
  });

  it("puts a root above its dependency", () => {
    const result = layout([{ id: "root" }, { id: "leaf" }], [{ from: "root", to: "leaf" }]);
    const root = result.positions.get("root")!;
    const leaf = result.positions.get("leaf")!;
    expect(root.y).toBeLessThan(leaf.y);
  });

  it("lays a chain out in strictly deepening layers", () => {
    const result = layout(
      [{ id: "a" }, { id: "b" }, { id: "c" }],
      [
        { from: "a", to: "b" },
        { from: "b", to: "c" },
      ],
    );
    const ya = result.positions.get("a")!.y;
    const yb = result.positions.get("b")!.y;
    const yc = result.positions.get("c")!.y;
    expect(ya).toBeLessThan(yb);
    expect(yb).toBeLessThan(yc);
  });

  it("is deterministic for a given input", () => {
    const nodes = [{ id: "b" }, { id: "a" }, { id: "c" }, { id: "d" }];
    const edges = [
      { from: "a", to: "c" },
      { from: "b", to: "c" },
      { from: "c", to: "d" },
    ];
    expect(layout(nodes, edges)).toEqual(layout(nodes, edges));
  });

  it("never places two nodes at the same point", () => {
    const result = layout(
      [{ id: "a" }, { id: "b" }, { id: "c" }, { id: "d" }],
      [
        { from: "a", to: "b" },
        { from: "a", to: "c" },
        { from: "a", to: "d" },
      ],
    );
    const seen = new Set<string>();
    for (const { x, y } of result.positions.values()) {
      const key = `${x},${y}`;
      expect(seen.has(key)).toBe(false);
      seen.add(key);
    }
  });

  it("terminates on a cycle and still places every node", () => {
    const result = layout(
      [{ id: "a" }, { id: "b" }, { id: "c" }],
      [
        { from: "a", to: "b" },
        { from: "b", to: "c" },
        { from: "c", to: "a" },
      ],
    );
    expect(result.positions.size).toBe(3);
    expect(allFinite(result)).toBe(true);
  });

  it("ignores an edge naming a node that is not present", () => {
    const withGhost = layout([{ id: "a" }, { id: "b" }], [{ from: "a", to: "ghost" }]);
    const clean = layout([{ id: "a" }, { id: "b" }], []);
    // The ghost edge places nothing and moves nobody: same layout as no edge.
    expect(withGhost.positions.get("a")).toEqual(clean.positions.get("a"));
    expect(withGhost.positions.get("b")).toEqual(clean.positions.get("b"));
    expect(withGhost.positions.has("ghost")).toBe(false);
  });

  it("centres a single node's row on x = 0", () => {
    const result = layout([{ id: "only" }], []);
    expect(result.positions.get("only")).toEqual({ x: 0, y: 0 });
  });

  it("bounds enclose every centre with margin on all sides", () => {
    const result = layout(
      [{ id: "a" }, { id: "b" }, { id: "c" }],
      [
        { from: "a", to: "b" },
        { from: "a", to: "c" },
      ],
    );
    const { x, y, width, height } = result.bounds;
    for (const p of result.positions.values()) {
      expect(p.x).toBeGreaterThanOrEqual(x);
      expect(p.x).toBeLessThanOrEqual(x + width);
      expect(p.y).toBeGreaterThanOrEqual(y);
      expect(p.y).toBeLessThanOrEqual(y + height);
    }
  });
});
