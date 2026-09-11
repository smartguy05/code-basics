import { describe, expect, it } from "vitest";
import {
  FOCUS_STACK_SPAN,
  focusOffset,
  raiseFocus,
  syncFocusOrder,
} from "./focusOrderLogic";

describe("raiseFocus", () => {
  it("moves the id to the top of the order", () => {
    expect(raiseFocus(["a", "b", "c"], "b")).toEqual(["a", "c", "b"]);
  });

  it("appends an id not yet in the order", () => {
    expect(raiseFocus(["a"], "z")).toEqual(["a", "z"]);
  });

  it("returns the same array when the id is already on top", () => {
    const order = ["a", "b", "c"];
    expect(raiseFocus(order, "c")).toBe(order);
  });

  it("raises into an empty order", () => {
    expect(raiseFocus([], "a")).toEqual(["a"]);
  });
});

describe("syncFocusOrder", () => {
  it("appends newly present ids so a fresh panel starts on top", () => {
    expect(syncFocusOrder(["a"], ["a", "b"])).toEqual(["a", "b"]);
  });

  it("prunes ids that are no longer present, keeping order", () => {
    expect(syncFocusOrder(["a", "b", "c"], ["c", "a"])).toEqual(["a", "c"]);
  });

  it("returns the same array when nothing changed", () => {
    const order = ["a", "b"];
    expect(syncFocusOrder(order, ["a", "b"])).toBe(order);
    // Order of `present` does not matter — membership does.
    expect(syncFocusOrder(order, ["b", "a"])).toBe(order);
  });

  it("prunes and appends in one pass", () => {
    expect(syncFocusOrder(["a", "b"], ["a", "c"])).toEqual(["a", "c"]);
  });

  it("empties when nothing is present", () => {
    expect(syncFocusOrder(["a", "b"], [])).toEqual([]);
  });
});

describe("focusOffset", () => {
  it("rises from 0 at the bottom to the top index", () => {
    expect(focusOffset(["a", "b", "c"], "a")).toBe(0);
    expect(focusOffset(["a", "b", "c"], "c")).toBe(2);
  });

  it("returns 0 for an id the order has not seen yet", () => {
    expect(focusOffset(["a"], "z")).toBe(0);
  });

  it("clamps into the span by collapsing the bottom, never the top", () => {
    const keys = Array.from({ length: FOCUS_STACK_SPAN + 5 }, (_, i) => `t${i}`);
    for (const key of keys) {
      const offset = focusOffset(keys, key);
      expect(offset).toBeGreaterThanOrEqual(0);
      expect(offset).toBeLessThanOrEqual(FOCUS_STACK_SPAN - 1);
    }
    // The top keeps its full raise; the bottom is collapsed onto 0.
    expect(focusOffset(keys, keys.at(-1) ?? "")).toBe(FOCUS_STACK_SPAN - 1);
    expect(focusOffset(keys, "t0")).toBe(0);
  });

  it("stays monotonically non-decreasing bottom to top even when clamped", () => {
    const keys = Array.from({ length: FOCUS_STACK_SPAN + 5 }, (_, i) => `t${i}`);
    let previous = -1;
    for (const offset of keys.map((k) => focusOffset(keys, k))) {
      expect(offset).toBeGreaterThanOrEqual(previous);
      previous = offset;
    }
  });

  it("pins the span against the stylesheet's --z-panel-stack-span", () => {
    // Deliberate drift alarm: styles.css reserves this many steps for the
    // floating-panel band. Changing one without the other silently lets a panel
    // climb into the dock/overlay bands.
    expect(FOCUS_STACK_SPAN).toBe(100);
  });
});
