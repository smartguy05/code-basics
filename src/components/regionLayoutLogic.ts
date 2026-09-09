//! Pure decisions for **split docking** — dragging an editor, diff or terminal
//! to a screen edge and docking it into a resizable region, for side-by-side
//! work. Named "regions", never "dock": `dockLogic`/`Dock`/`DockContext` already
//! own the word "dock" for the minimize-to-pill strip, which is a different
//! feature. The React plumbing (`RegionHost`, `RegionSplitter`,
//! `RegionDropOverlay`, `useRegionDrag`) decides nothing and is untested (vitest
//! runs in the node environment, so there is no DOM); everything decidable lives
//! here.
//!
//! Model: the workspace is a **center** plus up to four edge regions
//! (`left`/`right`/`top`/`bottom`). Each region holds an ordered **stack** of
//! dockables with its own active index and a size (a fraction of the workspace
//! on that region's axis). A stack, not a single item, so several things can
//! share one docked group with a local tab strip — exactly like VS Code.
//!
//! A **dockable** is a *reference*, never a component instance: `kind: "tab"`
//! names an open editor/diff tab by its `openFiles` id (a diff tab is a tab too,
//! so files and diffs need no separate kind), and `kind: "terminal"` names a
//! terminal by its key. This is what keeps the one-`FileEditor`-per-path
//! invariant provable: the center renders the tabs a region does **not** claim
//! (`centerTabIds`), each region renders its own, and the two sets are disjoint
//! by construction.

export type Edge = "left" | "right" | "top" | "bottom";
export type DropZone = Edge | "center";

export const EDGES: readonly Edge[] = ["left", "right", "top", "bottom"];

export type DockableKind = "tab" | "terminal";

export interface Dockable {
  kind: DockableKind;
  /** An `openFiles` tab id for `"tab"`, a terminal key for `"terminal"`. */
  id: string;
}

export interface Region {
  items: Dockable[];
  /** Index into `items` of the visible dockable. Clamped by every mutator. */
  active: number;
  /** This region's size as a fraction of the workspace on its axis. */
  size: number;
}

export type RegionLayout = Partial<Record<Edge, Region>>;

/** The fraction of the workspace a fresh region takes on its axis. */
export const DEFAULT_REGION_SIZE = 0.32;
/** A region can never take less than this or more than `1 - this`. */
export const MIN_REGION_FRACTION = 0.15;
/** The proportion of the workspace, per side, that counts as an edge drop band. */
export const EDGE_BAND = 0.2;

function sameDockable(a: Dockable, b: Dockable): boolean {
  return a.kind === b.kind && a.id === b.id;
}

/** Clamp a region size fraction to the allowed band. */
export function clampRegionSize(fraction: number): number {
  if (!Number.isFinite(fraction)) return DEFAULT_REGION_SIZE;
  return Math.min(1 - MIN_REGION_FRACTION, Math.max(MIN_REGION_FRACTION, fraction));
}

/**
 * Which drop zone a pointer at `point` (viewport coordinates) is over, given the
 * workspace `rect`. The outer `band` fraction of each side is that edge; the
 * middle is `"center"` (an undock / no-dock target). Left/right win over
 * top/bottom in the corners, which matches how the regions nest (left and right
 * are full height, top and bottom sit between them).
 */
export function edgeHitTest(
  point: { x: number; y: number },
  rect: { left: number; top: number; width: number; height: number },
  band: number = EDGE_BAND,
): DropZone {
  if (rect.width <= 0 || rect.height <= 0) return "center";
  const x = (point.x - rect.left) / rect.width;
  const y = (point.y - rect.top) / rect.height;
  if (x < 0 || x > 1 || y < 0 || y > 1) return "center";
  if (x <= band) return "left";
  if (x >= 1 - band) return "right";
  if (y <= band) return "top";
  if (y >= 1 - band) return "bottom";
  return "center";
}

/** A copy of `layout` with `dockable` removed from every region; regions left
 *  empty are deleted. Active indices are re-clamped. Pure. */
export function removeDockable(layout: RegionLayout, dockable: Dockable): RegionLayout {
  const next: RegionLayout = {};
  for (const edge of EDGES) {
    const region = layout[edge];
    if (!region) continue;
    const items = region.items.filter((item) => !sameDockable(item, dockable));
    if (items.length === 0) continue;
    next[edge] = {
      items,
      active: Math.min(region.active, items.length - 1),
      size: clampRegionSize(region.size),
    };
  }
  return next;
}

/**
 * Dock `dockable` into `edge`, moving it out of wherever it was first (a
 * dockable lives in at most one region). Dropping onto `"center"` just undocks
 * it. The dockable becomes the active item of its destination region; a fresh
 * region is created at `DEFAULT_REGION_SIZE`.
 */
export function dockInto(
  layout: RegionLayout,
  dockable: Dockable,
  zone: DropZone,
): RegionLayout {
  const without = removeDockable(layout, dockable);
  if (zone === "center") return without;
  const region = without[zone];
  const items = region ? [...region.items, dockable] : [dockable];
  return {
    ...without,
    [zone]: {
      items,
      active: items.length - 1,
      size: region ? clampRegionSize(region.size) : DEFAULT_REGION_SIZE,
    },
  };
}

/** Set which item in an edge's stack is active. No-op if the edge is empty or
 *  the index is out of range. */
export function setRegionActive(layout: RegionLayout, edge: Edge, active: number): RegionLayout {
  const region = layout[edge];
  if (!region || active < 0 || active >= region.items.length) return layout;
  return { ...layout, [edge]: { ...region, active } };
}

/** Resize the region on `edge` by a pixel delta from where a splitter drag
 *  began. `containerPx` is the workspace extent on that edge's axis. The sign is
 *  handled per edge: a left/top region grows as the splitter moves away from its
 *  edge, a right/bottom region grows as it moves toward it. */
export function resizeRegion(
  edge: Edge,
  startFraction: number,
  deltaPx: number,
  containerPx: number,
): number {
  if (containerPx <= 0) return clampRegionSize(startFraction);
  const deltaFraction = deltaPx / containerPx;
  const signed = edge === "left" || edge === "top" ? deltaFraction : -deltaFraction;
  return clampRegionSize(startFraction + signed);
}

/**
 * The editor/diff tab ids that belong in the **center** strip: every open tab a
 * region does not claim. This is the function that makes duplicate `FileEditor`s
 * impossible — the center renders exactly these, each region renders its own,
 * and the union is disjoint.
 */
export function centerTabIds(openTabIds: readonly string[], layout: RegionLayout): string[] {
  const docked = new Set<string>();
  for (const edge of EDGES) {
    for (const item of layout[edge]?.items ?? []) {
      if (item.kind === "tab") docked.add(item.id);
    }
  }
  return openTabIds.filter((id) => !docked.has(id));
}

/** The terminal keys that are docked (so the floating layer skips them). */
export function dockedTerminalIds(layout: RegionLayout): Set<string> {
  const ids = new Set<string>();
  for (const edge of EDGES) {
    for (const item of layout[edge]?.items ?? []) {
      if (item.kind === "terminal") ids.add(item.id);
    }
  }
  return ids;
}

/** The editor/diff tab ids that are docked (so the center strip skips them). */
export function dockedTabIds(layout: RegionLayout): Set<string> {
  const ids = new Set<string>();
  for (const edge of EDGES) {
    for (const item of layout[edge]?.items ?? []) {
      if (item.kind === "tab") ids.add(item.id);
    }
  }
  return ids;
}

/**
 * Drop references to things that no longer exist — a tab that was closed, a
 * terminal that exited — restoring an emptied region to nothing. Read on load
 * (localStorage is untrusted) and whenever the open sets change, so a region
 * never renders a stale id.
 */
export function pruneLayout(
  layout: RegionLayout,
  validTabIds: ReadonlySet<string>,
  validTerminalIds: ReadonlySet<string>,
): RegionLayout {
  const next: RegionLayout = {};
  for (const edge of EDGES) {
    const region = layout[edge];
    if (!region) continue;
    const items = region.items.filter((item) =>
      item.kind === "tab" ? validTabIds.has(item.id) : validTerminalIds.has(item.id),
    );
    if (items.length === 0) continue;
    next[edge] = {
      items,
      active: Math.min(Math.max(region.active, 0), items.length - 1),
      size: clampRegionSize(region.size),
    };
  }
  return next;
}

/** Whether any region holds anything — the host can skip all region chrome when
 *  false. */
export function hasRegions(layout: RegionLayout): boolean {
  return EDGES.some((edge) => (layout[edge]?.items.length ?? 0) > 0);
}

const KEY_PREFIX = "cb.regions.layout";

/** Per-codebase persistence key. The root is percent-encoded for the same
 *  Windows-colon reason the other per-root keys document. */
export function regionLayoutKey(root: string): string {
  return `${KEY_PREFIX}:${encodeURIComponent(root)}`;
}

/** Whether a value is a structurally valid region (defensive against hand-edited
 *  or older-shaped storage). */
function isRegion(value: unknown): value is Region {
  if (!value || typeof value !== "object") return false;
  const r = value as Partial<Region>;
  return (
    Array.isArray(r.items) &&
    r.items.every(
      (i) =>
        i &&
        typeof i === "object" &&
        (i.kind === "tab" || i.kind === "terminal") &&
        typeof i.id === "string",
    ) &&
    typeof r.active === "number" &&
    typeof r.size === "number"
  );
}

/** Read a stored layout, tolerating anything malformed by returning `{}` — a bad
 *  blob must never stop the workspace rendering. */
export function loadRegionLayout(storage: Pick<Storage, "getItem">, key: string): RegionLayout {
  try {
    const raw = storage.getItem(key);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as RegionLayout;
    const next: RegionLayout = {};
    for (const edge of EDGES) {
      const region = parsed[edge];
      if (isRegion(region) && region.items.length > 0) {
        next[edge] = {
          items: region.items,
          active: Math.min(Math.max(region.active, 0), region.items.length - 1),
          size: clampRegionSize(region.size),
        };
      }
    }
    return next;
  } catch {
    return {};
  }
}

export function saveRegionLayout(
  storage: Pick<Storage, "setItem">,
  layout: RegionLayout,
  key: string,
): void {
  try {
    storage.setItem(key, JSON.stringify(layout));
  } catch {
    /* storage full or unavailable — a lost layout is not worth throwing over */
  }
}
