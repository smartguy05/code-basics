/**
 * The pure decisions behind the Run tab editor's navigation history and its
 * pinned file tabs. No React, no DOM — everything here is node-testable, which
 * is why it lives beside `RunView.tsx` rather than inside it (see the repo rule:
 * a component is a rendering shell, its decisions live in a `*Logic.ts`).
 *
 * The history is a back/forward stack over the files the user has looked at,
 * driven by the browser side mouse buttons. It is modelled exactly like a
 * browser's: a list of entries and an index into it, where "back" and "forward"
 * move the index and opening a *new* file truncates whatever was ahead.
 */

/** One place the editor has shown: a file, and optionally a line jumped to. */
export interface NavEntry {
  /** Workspace-relative path — the file's identity, as `openFiles` holds it. */
  path: string;
  /** 1-based line to reveal, when the navigation named one (a goto jump did). */
  line?: number;
}

/**
 * The back/forward stack. `index` addresses the entry currently shown; entries
 * before it are "back", entries after it are "forward". An empty history is
 * `{ entries: [], index: -1 }`.
 */
export interface NavHistory {
  entries: NavEntry[];
  index: number;
}

/** How far back the history is allowed to grow before it evicts from the front. */
export const NAV_HISTORY_CAP = 50;

/** Two entries are the same navigation when both path and line match. */
function sameEntry(a: NavEntry, b: NavEntry): boolean {
  return a.path === b.path && a.line === b.line;
}

/**
 * Record a navigation.
 *
 * Anything ahead of the current position is discarded (opening a new file from
 * the middle of history forks a new future, as a browser does), the entry is
 * appended, and the index moves to it. Re-recording the entry already shown is a
 * no-op — clicking the active tab again, or a goto that lands where you already
 * are, must not litter the stack with duplicates. When the cap is reached the
 * oldest entry is dropped so the index still points at the newest.
 */
export function pushNav(
  history: NavHistory,
  entry: NavEntry,
  cap: number = NAV_HISTORY_CAP,
): NavHistory {
  const current = history.entries[history.index];
  if (current && sameEntry(current, entry)) return history;

  const kept = history.entries.slice(0, history.index + 1);
  kept.push(entry);

  const trimmed = kept.length > cap ? kept.slice(kept.length - cap) : kept;
  return { entries: trimmed, index: trimmed.length - 1 };
}

/** Step back one entry, or `null` when already at the start. */
export function navBack(
  history: NavHistory,
): { entry: NavEntry; history: NavHistory } | null {
  if (history.index <= 0) return null;
  const index = history.index - 1;
  // `index` is in `[0, length)` given the guard, so the entry exists.
  return { entry: history.entries[index]!, history: { ...history, index } };
}

/** Step forward one entry, or `null` when already at the end. */
export function navForward(
  history: NavHistory,
): { entry: NavEntry; history: NavHistory } | null {
  if (history.index >= history.entries.length - 1) return null;
  const index = history.index + 1;
  // `index` is in `[0, length)` given the guard, so the entry exists.
  return { entry: history.entries[index]!, history: { ...history, index } };
}

/**
 * Which history move a mouse button asks for, if any.
 *
 * `button === 3` is the browser "back" side button, `4` is "forward"; every
 * other button (0 primary, 1 middle, 2 secondary) is not ours. Mirrors
 * `recogniseFontSizeShortcut` in keeping the raw-event reading out of the view.
 */
export function navMouseAction(button: number): "back" | "forward" | null {
  if (button === 3) return "back";
  if (button === 4) return "forward";
  return null;
}

/**
 * Split the open tabs into the pinned row and the normal row, keeping each in
 * the original tab order. Generic over the tab shape so `RunView`'s `OpenFile`
 * fits without a type dependency pointing the wrong way.
 */
export function partitionTabs<T extends { id: string }>(
  files: T[],
  pinned: ReadonlySet<string>,
): { pinned: T[]; unpinned: T[] } {
  const pinnedTabs: T[] = [];
  const unpinnedTabs: T[] = [];
  for (const file of files) {
    (pinned.has(file.id) ? pinnedTabs : unpinnedTabs).push(file);
  }
  return { pinned: pinnedTabs, unpinned: unpinnedTabs };
}

/** Toggle a path's pinned state, returning a fresh set (never mutating input). */
export function togglePin(pinned: ReadonlySet<string>, path: string): Set<string> {
  const next = new Set(pinned);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  return next;
}
