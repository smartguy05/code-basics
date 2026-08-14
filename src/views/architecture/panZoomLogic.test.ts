import { describe, expect, it } from "vitest";

import {
  IDENTITY,
  MAX_SCALE,
  MIN_SCALE,
  clampScale,
  fit,
  panBy,
  toContent,
  toScreen,
  zoomAt,
} from "./panZoomLogic";
import type { View } from "./panZoomLogic";

describe("clampScale", () => {
  it("leaves a scale inside the bounds alone", () => {
    expect(clampScale(1)).toBe(1);
    expect(clampScale(0.5)).toBe(0.5);
    expect(clampScale(3.9)).toBe(3.9);
  });

  it("accepts both ends exactly", () => {
    expect(clampScale(MIN_SCALE)).toBe(MIN_SCALE);
    expect(clampScale(MAX_SCALE)).toBe(MAX_SCALE);
  });

  it("clamps past both ends", () => {
    expect(clampScale(0.0001)).toBe(MIN_SCALE);
    expect(clampScale(-4)).toBe(MIN_SCALE);
    expect(clampScale(0)).toBe(MIN_SCALE);
    expect(clampScale(1000)).toBe(MAX_SCALE);
    expect(clampScale(Infinity)).toBe(MAX_SCALE);
    expect(clampScale(-Infinity)).toBe(MIN_SCALE);
  });

  it("answers 1 for a scale that is not a number at all", () => {
    expect(clampScale(NaN)).toBe(1);
  });
});

describe("toScreen / toContent", () => {
  it("round-trip through the same view", () => {
    const view: View = { x: 31, y: -14, k: 2.5 };
    const point = { x: 12, y: 40 };
    const back = toContent(view, toScreen(view, point));
    expect(back.x).toBeCloseTo(point.x, 10);
    expect(back.y).toBeCloseTo(point.y, 10);
  });
});

describe("zoomAt", () => {
  const cases: { view: View; cursor: { x: number; y: number }; delta: number }[] = [
    { view: IDENTITY, cursor: { x: 0, y: 0 }, delta: -100 },
    { view: IDENTITY, cursor: { x: 400, y: 300 }, delta: -100 },
    { view: IDENTITY, cursor: { x: 400, y: 300 }, delta: 100 },
    { view: { x: -120, y: 55, k: 0.7 }, cursor: { x: 33, y: 900 }, delta: -240 },
    { view: { x: 900, y: -900, k: 3.2 }, cursor: { x: -50, y: 12 }, delta: 60 },
    // Deltas big enough to drive the scale into both clamps; the anchor must
    // hold there too, otherwise the diagram jumps at the limits.
    { view: { x: 10, y: 10, k: 3.9 }, cursor: { x: 250, y: 250 }, delta: -10000 },
    { view: { x: 10, y: 10, k: 0.25 }, cursor: { x: 250, y: 250 }, delta: 10000 },
  ];

  it("keeps the point under the cursor under the cursor", () => {
    for (const { view, cursor, delta } of cases) {
      // The content point the cursor is over before the zoom...
      const anchor = toContent(view, cursor);
      const zoomed = zoomAt(view, cursor, delta);
      // ...must still be drawn at the cursor after it.
      const after = toScreen(zoomed, anchor);
      expect(after.x).toBeCloseTo(cursor.x, 9);
      expect(after.y).toBeCloseTo(cursor.y, 9);
    }
  });

  it("zooms in on a negative delta and out on a positive one", () => {
    expect(zoomAt(IDENTITY, { x: 10, y: 10 }, -100).k).toBeGreaterThan(1);
    expect(zoomAt(IDENTITY, { x: 10, y: 10 }, 100).k).toBeLessThan(1);
  });

  it("never leaves the scale bounds", () => {
    expect(zoomAt({ x: 0, y: 0, k: 3.5 }, { x: 1, y: 1 }, -100000).k).toBe(MAX_SCALE);
    expect(zoomAt({ x: 0, y: 0, k: 0.3 }, { x: 1, y: 1 }, 100000).k).toBe(MIN_SCALE);
  });

  it("does nothing on a delta of zero", () => {
    expect(zoomAt({ x: 5, y: 6, k: 1.5 }, { x: 1, y: 2 }, 0)).toEqual({
      x: 5,
      y: 6,
      k: 1.5,
    });
  });

  it("refuses a non-finite delta or cursor rather than returning NaN", () => {
    const view: View = { x: 5, y: 6, k: 1.5 };
    expect(zoomAt(view, { x: 1, y: 2 }, NaN)).toEqual(view);
    expect(zoomAt(view, { x: 1, y: 2 }, Infinity)).toEqual(view);
    expect(zoomAt(view, { x: NaN, y: 2 }, -100)).toEqual(view);
    expect(zoomAt({ x: 0, y: 0, k: NaN }, { x: 1, y: 2 }, -100)).toEqual({
      x: 0,
      y: 0,
      k: NaN,
    });
  });
});

describe("panBy", () => {
  it("moves the view by the offset and leaves the scale alone", () => {
    expect(panBy({ x: 10, y: 20, k: 2 }, -5, 7)).toEqual({ x: 5, y: 27, k: 2 });
  });

  it("does not mutate the view it was given", () => {
    const view: View = { x: 10, y: 20, k: 2 };
    panBy(view, 1, 1);
    expect(view).toEqual({ x: 10, y: 20, k: 2 });
  });

  it("ignores a non-finite offset", () => {
    const view: View = { x: 10, y: 20, k: 2 };
    expect(panBy(view, NaN, 5)).toEqual(view);
    expect(panBy(view, 5, Infinity)).toEqual(view);
  });
});

describe("fit", () => {
  const viewport = { width: 800, height: 600 };

  it("shrinks content larger than the viewport until it fits", () => {
    const view = fit({ x: 0, y: 0, width: 1600, height: 600 }, viewport, 20);
    // 760 of usable width against 1600 of content.
    expect(view.k).toBeCloseTo(760 / 1600, 10);
    const topLeft = toScreen(view, { x: 0, y: 0 });
    const bottomRight = toScreen(view, { x: 1600, y: 600 });
    expect(topLeft.x).toBeGreaterThanOrEqual(20 - 1e-9);
    expect(bottomRight.x).toBeLessThanOrEqual(780 + 1e-9);
    expect(topLeft.y).toBeGreaterThanOrEqual(20 - 1e-9);
    expect(bottomRight.y).toBeLessThanOrEqual(580 + 1e-9);
  });

  it("centres the content in the viewport", () => {
    const view = fit({ x: 0, y: 0, width: 1600, height: 400 }, viewport, 0);
    const topLeft = toScreen(view, { x: 0, y: 0 });
    const bottomRight = toScreen(view, { x: 1600, y: 400 });
    expect(topLeft.x + bottomRight.x).toBeCloseTo(viewport.width, 9);
    expect(topLeft.y + bottomRight.y).toBeCloseTo(viewport.height, 9);
  });

  it("takes the tighter of the two axes on a non-square aspect ratio", () => {
    // Tall content in a wide viewport: height decides.
    const view = fit({ x: 0, y: 0, width: 100, height: 1200 }, viewport, 0);
    expect(view.k).toBeCloseTo(600 / 1200, 10);
  });

  it("enlarges content smaller than the viewport", () => {
    const view = fit({ x: 0, y: 0, width: 200, height: 150 }, viewport, 0);
    expect(view.k).toBeCloseTo(4, 10);
    expect(view.k).toBeLessThanOrEqual(MAX_SCALE);
  });

  it("respects the scale bounds when the fit would exceed them", () => {
    expect(fit({ x: 0, y: 0, width: 1, height: 1 }, viewport, 0).k).toBe(MAX_SCALE);
    expect(
      fit({ x: 0, y: 0, width: 100000, height: 100000 }, viewport, 0).k,
    ).toBe(MIN_SCALE);
  });

  it("honours a content box that does not start at the origin", () => {
    const view = fit({ x: 500, y: 300, width: 400, height: 300 }, viewport, 0);
    const topLeft = toScreen(view, { x: 500, y: 300 });
    expect(topLeft.x).toBeCloseTo(0, 9);
    expect(topLeft.y).toBeCloseTo(0, 9);
    expect(view.k).toBeCloseTo(2, 10);
  });

  it("returns the identity view for zero-sized content rather than NaN", () => {
    for (const content of [
      { x: 0, y: 0, width: 0, height: 0 },
      { x: 0, y: 0, width: 0, height: 400 },
      { x: 0, y: 0, width: 400, height: 0 },
      { x: 0, y: 0, width: -10, height: 400 },
    ]) {
      expect(fit(content, viewport, 10)).toEqual(IDENTITY);
    }
  });

  it("returns the identity view for a viewport with no room in it", () => {
    const content = { x: 0, y: 0, width: 400, height: 300 };
    expect(fit(content, { width: 0, height: 0 }, 0)).toEqual(IDENTITY);
    // Padding eating the whole viewport is the same case.
    expect(fit(content, { width: 40, height: 600 }, 20)).toEqual(IDENTITY);
  });

  it("returns the identity view rather than propagating a non-finite box", () => {
    expect(fit({ x: NaN, y: 0, width: 10, height: 10 }, viewport, 0)).toEqual(
      IDENTITY,
    );
    expect(
      fit({ x: 0, y: 0, width: Infinity, height: 10 }, viewport, 0),
    ).toEqual(IDENTITY);
    expect(
      fit({ x: 0, y: 0, width: 10, height: 10 }, { width: NaN, height: 10 }, 0),
    ).toEqual(IDENTITY);
    expect(fit({ x: 0, y: 0, width: 10, height: 10 }, viewport, NaN)).toEqual(
      IDENTITY,
    );
  });

  it("treats negative padding as none", () => {
    const content = { x: 0, y: 0, width: 400, height: 300 };
    expect(fit(content, viewport, -50)).toEqual(fit(content, viewport, 0));
  });
});
