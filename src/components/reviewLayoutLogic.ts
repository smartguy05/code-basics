//! Pure decisions for positioning the floating agent panel — the clamp
//! arithmetic and the layout persistence — extracted so they are testable
//! without a DOM (vitest runs in the node environment). The pointer-event
//! plumbing that drives them lives in `ReviewPanel.tsx` and decides nothing.

/** A top/left position, in CSS pixels relative to the viewport. */
export interface PanelPoint {
  left: number;
  top: number;
}

/** A measured panel rect, in CSS pixels. */
export interface PanelSize {
  width: number;
  height: number;
}

/** The available viewport (`window.innerWidth`/`innerHeight`). */
export interface PanelViewport {
  width: number;
  height: number;
}

/** A small gap kept between the panel and each viewport edge. */
const EDGE_MARGIN = 8;

/**
 * Clamp a desired position so the panel stays on-screen with a small visible
 * margin at each edge. A dimension larger than the viewport can't be kept
 * inside it, so that axis pins to the origin (0) rather than floating partway
 * off. Pure arithmetic — the caller supplies the measured rect and viewport.
 */
export function clampPanelPosition(
  pos: PanelPoint,
  size: PanelSize,
  viewport: PanelViewport,
): PanelPoint {
  return {
    left: clampAxis(pos.left, size.width, viewport.width),
    top: clampAxis(pos.top, size.height, viewport.height),
  };
}

function clampAxis(desired: number, extent: number, available: number): number {
  const max = available - extent - EDGE_MARGIN;
  // The panel is wider/taller than the viewport (no on-screen room past the
  // margin): pin to the origin.
  if (max <= EDGE_MARGIN) return 0;
  return Math.min(Math.max(desired, EDGE_MARGIN), max);
}

// The CSS floor (`.review-panel` min-width/min-height) and the viewport-relative
// ceiling (max-width: 96vw / max-height: 92vh). Kept in step with styles.css so
// the persisted size never lands outside what the stylesheet would allow.
const MIN_WIDTH = 360;
const MIN_HEIGHT = 280;
const MAX_WIDTH_FACTOR = 0.96;
const MAX_HEIGHT_FACTOR = 0.92;

/**
 * Clamp a measured panel size to the range the stylesheet permits: no smaller
 * than the CSS floor, no larger than the viewport ceiling. The floor wins on a
 * conflict (as `min-width` beats `max-width` in CSS), so a tiny viewport still
 * yields a usable size. Pure arithmetic — the caller supplies the measured rect
 * and viewport.
 */
export function clampPanelSize(size: PanelSize, viewport: PanelViewport): PanelSize {
  return {
    width: clampExtent(size.width, MIN_WIDTH, viewport.width * MAX_WIDTH_FACTOR),
    height: clampExtent(size.height, MIN_HEIGHT, viewport.height * MAX_HEIGHT_FACTOR),
  };
}

function clampExtent(desired: number, min: number, max: number): number {
  return Math.max(min, Math.min(desired, max));
}

// --- Remembering the panel's position --------------------------------------

/** The persisted panel layout: its dragged position and its resized size. */
export interface PanelLayout {
  left?: number;
  top?: number;
  width?: number;
  height?: number;
}

/**
 * The default persistence key — the agent panel's. Callers that host more than
 * one kind of floating panel (the terminals) pass their own key so their layout
 * does not fight the agent panel's; omitting it keeps the original behaviour.
 */
const DEFAULT_LAYOUT_KEY = "cb.agentPanel.layout";

/** Read the remembered position. A missing or unparseable value is empty. */
export function loadPanelLayout(
  storage: Pick<Storage, "getItem">,
  key: string = DEFAULT_LAYOUT_KEY,
): PanelLayout {
  try {
    const raw = storage.getItem(key);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const { left, top, width, height } = parsed as Record<string, unknown>;
    return {
      left: typeof left === "number" ? left : undefined,
      top: typeof top === "number" ? top : undefined,
      width: typeof width === "number" ? width : undefined,
      height: typeof height === "number" ? height : undefined,
    };
  } catch {
    return {};
  }
}

/** Remember the panel position. Never throws (storage may be unavailable). */
export function savePanelLayout(
  storage: Pick<Storage, "setItem">,
  layout: PanelLayout,
  key: string = DEFAULT_LAYOUT_KEY,
): void {
  try {
    storage.setItem(key, JSON.stringify(layout));
  } catch {
    // Ignore: persistence is a convenience, not a requirement.
  }
}

// --- Deciding when a resize is worth persisting ----------------------------

/** A stateful gate over the raw ResizeObserver stream (see `createResizeGate`). */
export interface ResizeGate {
  /**
   * Given a measured panel size, decide whether it is a genuine user resize
   * worth persisting. Returns false for the mount default, for a hidden
   * (minimized) 0×0 measurement, and for a restore back to the last non-zero
   * size; true only when the size genuinely differs from the last one seen.
   */
  persist(size: PanelSize): boolean;
}

/**
 * A stateful, DOM-free gate deciding which ResizeObserver measurements are real
 * user resizes worth persisting. The observer fires once on `observe()` with
 * the un-resized CSS default, continuously through a drag, and reports 0×0 while
 * the panel is hidden (minimized) — a naive "not the first callback" test would
 * persist the CSS default the moment a minimize/restore cycle re-measured it.
 *
 * It keeps the last *non-zero* size seen: a hidden 0×0 measurement is ignored
 * without updating that memory, so a restore back to the same size compares
 * equal and is refused. The first non-zero measurement (the mount default) is
 * recorded but never persisted. Same shape as `createNdjsonBuffer` — a factory
 * closing over private state, tested headlessly.
 */
export function createResizeGate(): ResizeGate {
  // The last non-zero size seen. Undefined until the first real measurement.
  let last: PanelSize | undefined;
  return {
    persist(size: PanelSize): boolean {
      // Hidden (minimized): don't persist, and don't update `last`, so a later
      // restore to the same size still compares equal.
      if (size.width === 0 || size.height === 0) return false;
      const prev = last;
      last = size;
      // The mount default: recorded, never persisted.
      if (prev === undefined) return false;
      // A genuine resize only if the size actually changed.
      return size.width !== prev.width || size.height !== prev.height;
    },
  };
}
