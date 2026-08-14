/**
 * Panning and zooming a rendered diagram, as arithmetic.
 *
 * No library: a diagram is one SVG element, and everything the user can do to
 * it is a single affine transform. Adding a pan/zoom dependency to move one
 * `<g>` around would be a package to audit, a bundle to ship and a CSP surface
 * to re-check, in exchange for four functions of six lines each.
 *
 * # The one transform
 *
 * A {@link View} is `{ x, y, k }` and means exactly `translate(x, y) scale(k)`
 * — scale first about the content origin, then translate. So
 *
 * ```
 * screen = content * k + (x, y)
 * ```
 *
 * which is {@link toScreen}, and its inverse {@link toContent}. Those two
 * exist mostly so the tests can state the properties that matter in the same
 * terms the renderer applies them, rather than restating the algebra and
 * risking agreeing with a bug.
 *
 * Every function here returns a **new** view and mutates nothing: these values
 * live in React state, and a mutated one would not re-render.
 *
 * # Where this abstains
 *
 * A wheel event, an element's bounding box and a laid-out SVG all reach this
 * module as plain numbers, and any of them can be `NaN` — a zero-sized
 * container during the first layout pass, a diagram that has not rendered yet,
 * a `getBoundingClientRect` on a hidden element. A `NaN` that reaches a
 * transform does not throw: it makes the whole diagram vanish, with no error
 * anywhere and nothing to search for. So each function checks its inputs and
 * declines rather than propagating — the view stays where it was, which is
 * visibly nothing happening rather than invisibly everything disappearing.
 */

/** The transform applied to the diagram: `translate(x, y) scale(k)`. */
export interface View {
  x: number;
  y: number;
  k: number;
}

/** A point, in either space; which one is always said by the parameter name. */
export interface Point {
  x: number;
  y: number;
}

/** How much room there is to draw in. */
export interface Size {
  width: number;
  height: number;
}

/** A box in content space — where the diagram's ink actually is. */
export interface Box extends Point, Size {}

/** Untransformed: the diagram at its natural size, top-left in the corner. */
export const IDENTITY: View = Object.freeze({ x: 0, y: 0, k: 1 });

/**
 * The smallest scale. Below this a diagram of any size is a grey smudge, and
 * the user has lost the ability to zoom back in by aiming at something.
 */
export const MIN_SCALE = 0.2;

/**
 * The largest scale. Past this a box fills the viewport and the arrows leaving
 * it are off-screen, which is all context and no content.
 */
export const MAX_SCALE = 4;

/**
 * How fast the wheel zooms.
 *
 * Applied exponentially (see {@link zoomAt}), so one notch of a typical mouse
 * wheel — `deltaY` of ±100 — is about a 12% change either way, and two notches
 * compose to exactly the same place as one notch of twice the size.
 */
const ZOOM_SENSITIVITY = 0.0012;

/** Whether a number can be used in a transform without destroying it. */
function usable(...values: number[]): boolean {
  return values.every((value) => Number.isFinite(value));
}

/** Where a content point is drawn on screen under a view. */
export function toScreen(view: View, point: Point): Point {
  return { x: point.x * view.k + view.x, y: point.y * view.k + view.y };
}

/** Which content point is under a screen position. */
export function toContent(view: View, point: Point): Point {
  return { x: (point.x - view.x) / view.k, y: (point.y - view.y) / view.k };
}

/**
 * A scale held inside the usable range.
 *
 * `NaN` answers 1 rather than a bound. It is not a scale that is too large or
 * too small, it is the absence of one, and 1 is the only value that is both
 * valid and neutral — clamping it to `MIN_SCALE` would leave the user staring
 * at a smudge and wondering what they did.
 */
export function clampScale(k: number): number {
  if (Number.isNaN(k)) return 1;
  return Math.min(Math.max(k, MIN_SCALE), MAX_SCALE);
}

/**
 * Zoom about the cursor, so the thing under the pointer stays under it.
 *
 * This is the one people get wrong. Scaling about the origin is a line
 * shorter and feels broken in a way users cannot describe: they point at a
 * box, zoom, and the box slides off the screen, so every zoom has to be
 * followed by a pan to find what they were looking at. The fix is to solve for
 * the translation that pins one point:
 *
 * ```
 * anchor  = toContent(view, cursor)          // what is under the pointer now
 * k2      = clampScale(k * e^(-delta·s))     // where the wheel wants to go
 * (x2,y2) = cursor - anchor·k2               // put it back under the pointer
 * ```
 *
 * The translation is derived from the **clamped** scale, so the anchor holds
 * at the limits too: a user who keeps scrolling at maximum zoom sees nothing
 * move, rather than the diagram creeping sideways while the scale stays put.
 * The tests assert that property directly rather than the formula, over
 * scaling in, scaling out, and both clamps.
 *
 * `delta` is a wheel `deltaY`: positive scrolls down and zooms **out**, which
 * is the convention every map and editor uses. It is applied through `exp` so
 * that zooming is multiplicative — the visual step is the same whether the
 * user is at 0.3 or at 3 — and so that equal and opposite deltas compose back
 * to where they started instead of drifting.
 */
export function zoomAt(view: View, cursor: Point, delta: number): View {
  if (!usable(delta, cursor.x, cursor.y, view.x, view.y, view.k)) return view;
  if (view.k === 0) return view;

  const scaled = clampScale(view.k * Math.exp(-delta * ZOOM_SENSITIVITY));
  if (scaled === view.k) return view;

  const anchor = toContent(view, cursor);
  return {
    x: cursor.x - anchor.x * scaled,
    y: cursor.y - anchor.y * scaled,
    k: scaled,
  };
}

/**
 * Drag the diagram by a screen-space offset.
 *
 * The offset is not divided by the scale: a drag is measured in the pixels the
 * pointer actually moved, and the content should follow the pointer exactly at
 * every zoom level. Dividing by `k` — the mistake available here — makes the
 * diagram lag the cursor when zoomed in and outrun it when zoomed out.
 */
export function panBy(view: View, dx: number, dy: number): View {
  if (!usable(dx, dy, view.x, view.y)) return view;
  return { x: view.x + dx, y: view.y + dy, k: view.k };
}

/**
 * The view that centres `content` inside `viewport` with `padding` on all
 * sides.
 *
 * The smaller of the two axis ratios wins, so the whole box fits rather than
 * one axis fitting and the other overflowing. Content smaller than the
 * viewport is scaled **up** — "fit" means fill the space available, and a
 * two-project diagram pinned at 1× in a maximised window is mostly empty
 * background — but never past {@link MAX_SCALE}, so a single box does not
 * become a wall.
 *
 * `content.x`/`content.y` are subtracted rather than assumed to be zero: an
 * SVG's ink starts wherever the renderer put it, and a bounding box with a
 * non-zero origin is the normal case, not an edge case.
 *
 * Every degenerate input answers {@link IDENTITY}: content with no width or
 * height (nothing rendered yet, or a diagram of one invisible node), a
 * viewport with no room in it (a hidden tab, or the first paint), padding that
 * eats the whole viewport, or any non-finite number among them. Each of those
 * is a division by zero or a `NaN` away from a transform that draws nothing at
 * all, and the identity view at least shows the diagram at its natural size in
 * the corner — recognisably un-fitted, rather than recognisably absent.
 */
export function fit(content: Box, viewport: Size, padding: number): View {
  if (!usable(content.x, content.y, content.width, content.height)) return IDENTITY;
  if (!usable(viewport.width, viewport.height, padding)) return IDENTITY;
  if (content.width <= 0 || content.height <= 0) return IDENTITY;

  const inset = Math.max(padding, 0);
  const available = {
    width: viewport.width - inset * 2,
    height: viewport.height - inset * 2,
  };
  if (available.width <= 0 || available.height <= 0) return IDENTITY;

  const k = clampScale(
    Math.min(available.width / content.width, available.height / content.height),
  );
  return {
    x: (viewport.width - content.width * k) / 2 - content.x * k,
    y: (viewport.height - content.height * k) / 2 - content.y * k,
    k,
  };
}
