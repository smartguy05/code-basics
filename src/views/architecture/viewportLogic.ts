/**
 * Where the user had panned and zoomed each diagram to, kept across a remount.
 *
 * # Why this has to exist at all
 *
 * The Architecture tab is mounted conditionally, like Changes and History: it
 * owns no process, and everything it shows is files on disk, so re-mounting
 * re-reads them — which is exactly what you want after a regeneration. The cost
 * is that leaving the tab destroys the canvas, and with it the transform. A
 * diagram of thirty projects is unreadable until it has been zoomed into the
 * corner you care about, and doing that again after every glance at the Run tab
 * is the kind of small repeated tax that makes a feature not get used.
 *
 * So the viewport is stored, per diagram, in `localStorage` — the precedent is
 * `recentsLogic.ts` and `RunView`'s split position, both of which do the same
 * thing for the same reason. It is keyed per diagram and not per tab because
 * the two built-in maps are different shapes: restoring the project map's
 * viewport onto the component map would open it scrolled to a corner where
 * that diagram has no ink at all.
 *
 * # Why every field is checked on the way back in
 *
 * `localStorage` is not this app's private memory. It is editable by hand, it
 * survives a version of this app with different zoom limits, and it survives a
 * version with a different shape of stored value. A `NaN` or a missing field
 * reaching the transform does not throw — it makes the entire diagram vanish,
 * with nothing in the console and nothing to search for. So a stored value is
 * treated as untrusted input: anything that is not three finite numbers with a
 * usable scale is declined, and declining means the canvas fits the diagram as
 * though it had never been opened. That is a visibly ordinary first view rather
 * than an invisible blank one.
 *
 * The scale is the one field that is repaired rather than refused: it is
 * clamped through {@link clampScale}, the same function the wheel uses, so the
 * range has exactly one definition. A view stored at 12× by an older build
 * would otherwise be unrecoverable — the canvas cannot zoom out past its own
 * limit, so the user would be left inside a box with no way back out.
 */

import { clampScale, type View } from "./panZoomLogic";

/** Namespace for every stored viewport, so one `removeItem` sweep could find them. */
export const VIEWPORT_KEY_PREFIX = "code-basics.diagramView";

/** The slice of `Storage` this needs (localStorage in the app, a map in tests). */
export type ViewportStorage = Pick<Storage, "getItem" | "setItem">;

/**
 * The storage key for one diagram of one workspace.
 *
 * Both parts are percent-encoded before being joined. A Windows workspace root
 * contains a colon (`C:/repo`) and a diagram id contains one too
 * (`builtin:project`, `saved:foo.mmd`), so a plain join has a movable boundary:
 * `("a:b", "c")` and `("a", "b:c")` would produce the same string, and one
 * diagram would silently open at another's pan and zoom. Encoding makes the
 * separator the only unescaped colon in each part.
 */
export function viewportKey(root: string, diagramId: string): string {
  return `${VIEWPORT_KEY_PREFIX}:${encodeURIComponent(root)}:${encodeURIComponent(diagramId)}`;
}

/** A finite number, and nothing else — `null`, `"1"` and `NaN` all fail here. */
function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

/**
 * The stored view for a diagram, or `null` when there is not a usable one.
 *
 * `null` means "fit it", which is what the canvas does anyway with no stored
 * view, so every rejection below lands the user somewhere sensible.
 */
export function loadViewport(storage: ViewportStorage, key: string): View | null {
  let raw: string | null;
  try {
    // A storage that throws on read is a real state (a disabled or full one),
    // and it is not worth taking the tab down for.
    raw = storage.getItem(key);
  } catch {
    return null;
  }
  if (raw === null) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return null;

  const { x, y, k } = parsed as Partial<View>;
  if (!finite(x) || !finite(y) || !finite(k)) return null;
  // Zero or negative is not a scale that is merely out of range: zero draws
  // nothing and negative draws the diagram mirrored through the origin. Neither
  // is a viewport anyone left behind, so it is refused rather than clamped.
  if (k <= 0) return null;

  return { x, y, k: clampScale(k) };
}

/**
 * Remember where a diagram is being looked at.
 *
 * Called from a pointer-move path, so it must not throw: a full storage quota
 * would otherwise abort a drag and take the diagram with it. A view that could
 * not be read back is not written at all, so a `NaN` cannot be stored and then
 * quietly declined on every future open.
 */
export function saveViewport(storage: ViewportStorage, key: string, view: View): void {
  if (!finite(view.x) || !finite(view.y) || !finite(view.k) || view.k <= 0) return;
  try {
    storage.setItem(key, JSON.stringify({ x: view.x, y: view.y, k: view.k }));
  } catch {
    /* nothing worth reporting: the diagram is on screen, only its memory is not */
  }
}
