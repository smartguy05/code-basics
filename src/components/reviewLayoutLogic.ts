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

// --- Remembering the panel's position --------------------------------------

/** The persisted panel position. Size is intentionally not persisted — it
 * stays CSS-native (the native resize grip), a deferred nicety to persist. */
export interface PanelLayout {
  left?: number;
  top?: number;
}

const LAYOUT_KEY = "cb.agentPanel.layout";

/** Read the remembered position. A missing or unparseable value is empty. */
export function loadPanelLayout(storage: Pick<Storage, "getItem">): PanelLayout {
  try {
    const raw = storage.getItem(LAYOUT_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const { left, top } = parsed as Record<string, unknown>;
    return {
      left: typeof left === "number" ? left : undefined,
      top: typeof top === "number" ? top : undefined,
    };
  } catch {
    return {};
  }
}

/** Remember the panel position. Never throws (storage may be unavailable). */
export function savePanelLayout(storage: Pick<Storage, "setItem">, layout: PanelLayout): void {
  try {
    storage.setItem(LAYOUT_KEY, JSON.stringify(layout));
  } catch {
    // Ignore: persistence is a convenience, not a requirement.
  }
}
