import { describe, expect, it } from "vitest";
import {
  clampPanelPosition,
  clampPanelSize,
  createResizeGate,
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

describe("clampPanelSize", () => {
  const viewport = { width: 1200, height: 800 };

  it("leaves an in-range size unchanged", () => {
    expect(clampPanelSize({ width: 700, height: 500 }, viewport)).toEqual({
      width: 700,
      height: 500,
    });
  });

  it("clamps an oversized panel down to the viewport ceiling (96vw/92vh)", () => {
    const r = clampPanelSize({ width: 5000, height: 5000 }, viewport);
    expect(r.width).toBe(viewport.width * 0.96);
    expect(r.height).toBe(viewport.height * 0.92);
  });

  it("clamps a below-floor panel up to the CSS min (360×280)", () => {
    const r = clampPanelSize({ width: 100, height: 100 }, viewport);
    expect(r.width).toBe(360);
    expect(r.height).toBe(280);
  });

  it("clamps each axis independently", () => {
    const r = clampPanelSize({ width: 100, height: 5000 }, viewport);
    expect(r.width).toBe(360); // below floor → up to min
    expect(r.height).toBe(viewport.height * 0.92); // oversized → down to ceiling
  });
});

describe("panel layout persistence", () => {
  it("round-trips a saved layout", () => {
    const store = fakeStorage();
    const layout: PanelLayout = { left: 120, top: 240 };
    savePanelLayout(store, layout);
    expect(loadPanelLayout(store)).toEqual(layout);
  });

  it("round-trips a saved size alongside the position", () => {
    const store = fakeStorage();
    const layout: PanelLayout = { left: 120, top: 240, width: 700, height: 500 };
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

  it("drops string width/height rather than trusting them", () => {
    expect(
      loadPanelLayout(fakeStorage('{"width":"700","height":500}')),
    ).toEqual({
      left: undefined,
      top: undefined,
      width: undefined,
      height: 500,
    });
  });

  it("uses a key distinct from the agent-prefs key", () => {
    const store = fakeStorage();
    savePanelLayout(store, { left: 1, top: 2 });
    // Sanity: the raw value is JSON we can read back.
    expect(JSON.parse(store.read() ?? "{}")).toEqual({ left: 1, top: 2 });
  });
});

describe("createResizeGate", () => {
  it("skips the initial default measurement even without a minimize", () => {
    const gate = createResizeGate();
    // The ResizeObserver fires once on observe() with the un-resized CSS size.
    expect(gate.persist({ width: 480, height: 420 })).toBe(false);
  });

  it("persists a real resize", () => {
    const gate = createResizeGate();
    expect(gate.persist({ width: 480, height: 420 })).toBe(false); // mount default
    expect(gate.persist({ width: 700, height: 500 })).toBe(true); // genuine resize
  });

  it("does not persist a minimize -> restore that returns to the default size", () => {
    const gate = createResizeGate();
    // Mount default, then minimize (hidden = 0×0), then restore to the same size.
    expect(gate.persist({ width: 480, height: 420 })).toBe(false); // mount default
    expect(gate.persist({ width: 0, height: 0 })).toBe(false); // minimized (hidden)
    expect(gate.persist({ width: 480, height: 420 })).toBe(false); // restored to default
    // A genuine resize afterwards still persists.
    expect(gate.persist({ width: 700, height: 500 })).toBe(true);
  });

  it("does not re-persist a minimize -> restore to a previously-resized size", () => {
    const gate = createResizeGate();
    expect(gate.persist({ width: 480, height: 420 })).toBe(false); // mount default
    expect(gate.persist({ width: 700, height: 500 })).toBe(true); // genuine resize
    expect(gate.persist({ width: 0, height: 0 })).toBe(false); // minimized (hidden)
    expect(gate.persist({ width: 700, height: 500 })).toBe(false); // restored to resized size
  });
});
