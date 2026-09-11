//! Pure decisions for the shared minimized-window **dock** — the single strip
//! that lays out every floating panel's minimized pill, replacing the ad-hoc CSS
//! corners each panel used to pick for itself (Notes/Review/Behavioral all landed
//! in the same bottom-right rectangle; SQL/Browser dodged to bottom-left;
//! terminals cascaded upward). Extracted so they are testable in the node
//! environment (vitest, no DOM); `Dock.tsx` renders what these return and decides
//! nothing.
//!
//! The dock owns *no* geometry arithmetic on purpose: a flex strip lays its pills
//! out with no overlaps structurally, so the only decisions here are which entries
//! belong on screen and in what order.

/**
 * One minimized panel's presence in the dock — data only. The live registry
 * stores `DockEntry & { onRestore }`; the restore callback lives in the React
 * layer, not here, so this stays a plain value that is trivial to test.
 */
export interface DockEntry {
  /**
   * Unique across every open codebase. Built by {@link dockId} from the entry's
   * scope and a panel-local id, so two codebases' `term-1` cannot collide.
   */
  id: string;
  /**
   * `"global"` for a panel that belongs to no codebase (Notes), or a workspace
   * root for a per-codebase panel. {@link visibleEntries} filters on this so a
   * background codebase's minimized work does not clutter the foreground dock —
   * matching today's behaviour, where a background `WorkspaceTab` is `hidden` and
   * its `position: fixed` pills already vanish.
   */
  scope: string;
  /** What the pill reads, e.g. "Terminal 3", "Notes", a page title. */
  label: string;
  /**
   * Registration order, for a stable left-to-right layout. A terminal passes its
   * reusable **number** rather than an array index, so closing a middle terminal
   * does not reflow the strip.
   */
  order: number;
  /** Pinned entries (Notes) sort to the leading edge and keep a stable slot. */
  pinned?: boolean;
  /** A user-chosen pill tint (terminals), carried straight through. */
  color?: string;
  /** A short status shown in the pill's tooltip (running/exited/…). */
  status?: string;
  /** Whether the pill should flash for attention (a terminal bell, an agent). */
  attention?: boolean;
  /** Whether to show the small running spinner (Review/Behavioral). */
  spinner?: boolean;
}

/** The minimal shape the ordering/registry functions need, so the same code
 * serves both `DockEntry` and the live `DockEntry & { onRestore }`. */
interface DockLike {
  id: string;
  scope: string;
  order: number;
  pinned?: boolean;
}

/**
 * Compose a globally-unique dock id from a scope and a panel-local id. Two
 * codebases each hosting a `term-1` get `"/a:term-1"` and `"/b:term-1"`, so the
 * registry never conflates them.
 */
export function dockId(scope: string, localId: string): string {
  return `${scope}:${localId}`;
}

/**
 * Register a new entry, or update an existing one **in place**.
 *
 * In place matters: an attention flag or a colour change must not move the pill,
 * which would make the strip jump under the user's cursor. Returns a new array
 * (so React state updates) but preserves every entry's position.
 */
export function upsertEntry<T extends DockLike>(list: T[], entry: T): T[] {
  const index = list.findIndex((e) => e.id === entry.id);
  if (index === -1) return [...list, entry];
  const next = list.slice();
  next[index] = entry;
  return next;
}

/**
 * Drop the entry with `id` (a panel restored or unmounted). Returns the **same
 * array reference** when the id is absent, so a stray removal costs no render.
 */
export function removeEntry<T extends DockLike>(list: T[], id: string): T[] {
  const index = list.findIndex((e) => e.id === id);
  if (index === -1) return list;
  return list.filter((e) => e.id !== id);
}

/**
 * The entries the dock should show, in order.
 *
 * Filters to `scope === "global"` plus the active codebase's root — a background
 * codebase's pills stay hidden, matching the old `hidden`-tab behaviour — then
 * sorts pinned-first (Notes keeps the leading slot) and otherwise by registration
 * `order`. The sort is **stable**: two entries with equal `order` keep their
 * relative order, so the strip never reshuffles on an unrelated update.
 */
export function visibleEntries<T extends DockLike>(list: T[], activeRoot: string | null): T[] {
  const shown = list.filter((e) => e.scope === "global" || e.scope === activeRoot);
  // `map`/`sort`/`map` rather than `sort` alone: `Array.prototype.sort` is stable
  // in modern engines, but keying by index makes the tie-break explicit and
  // independent of that guarantee.
  return shown
    .map((entry, index) => ({ entry, index }))
    .sort((a, b) => {
      const pinned = Number(b.entry.pinned ?? false) - Number(a.entry.pinned ?? false);
      if (pinned !== 0) return pinned;
      if (a.entry.order !== b.entry.order) return a.entry.order - b.entry.order;
      return a.index - b.index;
    })
    .map(({ entry }) => entry);
}
