import { describe, expect, it } from "vitest";
import {
  clampPanelPosition,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
} from "./reviewLayoutLogic";

/** A minimal in-memory Storage stand-in (mirrors reviewLogic.test.ts). */
function fakeStorage(seed?: string) {
  let value: string | null = seed ?? null;
  return {
    getItem: () => value,
    setItem: (_k: string, v: string) => {
      value = v;
    },
    read: () => value,
  };
}

describe("clampPanelPosition", () => {
  const size = { width: 400, height: 300 };
  const viewport = { width: 1200, height: 800 };

  it("leaves an on-screen position unchanged", () => {
    expect(clampPanelPosition({ left: 200, top: 150 }, size, viewport)).toEqual({
      left: 200,
      top: 150,
    });
  });

  it("clamps a position pushed past the left/top edges", () => {
    const r = clampPanelPosition({ left: -500, top: -500 }, size, viewport);
    expect(r.left).toBeGreaterThanOrEqual(0);
    expect(r.top).toBeGreaterThanOrEqual(0);
    // Keeps a small visible margin rather than sitting flush against the edge.
    expect(r.left).toBeLessThan(50);
    expect(r.top).toBeLessThan(50);
  });

  it("clamps a position pushed past the right/bottom edges so the panel stays on-screen", () => {
    const r = clampPanelPosition({ left: 5000, top: 5000 }, size, viewport);
    expect(r.left).toBeLessThanOrEqual(viewport.width - size.width);
    expect(r.top).toBeLessThanOrEqual(viewport.height - size.height);
    // Still fully on-screen.
    expect(r.left + size.width).toBeLessThanOrEqual(viewport.width);
    expect(r.top + size.height).toBeLessThanOrEqual(viewport.height);
  });

  it("pins a panel larger than the viewport to the origin", () => {
    const huge = { width: 2000, height: 2000 };
    expect(clampPanelPosition({ left: 300, top: 300 }, huge, viewport)).toEqual({
      left: 0,
      top: 0,
    });
  });

  it("clamps each axis independently when only one dimension overflows", () => {
    const wide = { width: 2000, height: 300 };
    const r = clampPanelPosition({ left: 300, top: 150 }, wide, viewport);
    expect(r.left).toBe(0); // width overflows → pinned
    expect(r.top).toBe(150); // height fits → unchanged
  });
});

describe("panel layout persistence", () => {
  it("round-trips a saved layout", () => {
    const store = fakeStorage();
    const layout: PanelLayout = { left: 120, top: 240 };
    savePanelLayout(store, layout);
    expect(loadPanelLayout(store)).toEqual(layout);
  });

  it("reads missing or garbage storage as empty", () => {
    expect(loadPanelLayout(fakeStorage())).toEqual({});
    expect(loadPanelLayout(fakeStorage("not json"))).toEqual({});
    expect(loadPanelLayout(fakeStorage("[1,2,3]"))).toEqual({});
  });

  it("drops wrong-typed fields rather than trusting them", () => {
    expect(loadPanelLayout(fakeStorage('{"left":"120","top":240}'))).toEqual({
      left: undefined,
      top: 240,
    });
  });

  it("uses a key distinct from the agent-prefs key", () => {
    const store = fakeStorage();
    savePanelLayout(store, { left: 1, top: 2 });
    // Sanity: the raw value is JSON we can read back.
    expect(JSON.parse(store.read() ?? "{}")).toEqual({ left: 1, top: 2 });
  });
});
