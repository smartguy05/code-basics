//! Pure decisions for the top-level workspace tab strip — adding an open
//! codebase, closing one and picking the neighbour that inherits focus, and
//! labelling tabs when two repositories share a basename. Extracted so they are
//! testable without a DOM (vitest runs in the node environment); the React
//! plumbing lives in `App.tsx` and decides nothing.
//!
//! The identity of an open workspace is its `root` path — the same key the
//! backend shards its `AppState` by, and the same value the app already treats
//! as a workspace id everywhere (`key={workspace.root}`, the setup-dismissal
//! key, recents entries).

import type { Workspace } from "../ipc/types";

/**
 * Add a freshly opened workspace to the open set and choose the active root.
 *
 * Opening a folder that is already open **focuses** it rather than appending a
 * duplicate tab — and replaces the stored object in place, because a re-open is
 * a rescan and carries the fresher `Workspace` (new configs, new name). This
 * mirrors `rememberRecent`'s de-dupe: identity is the `root`.
 */
export function addOpenWorkspace(
  open: Workspace[],
  opened: Workspace,
): { list: Workspace[]; activeRoot: string } {
  const idx = open.findIndex((w) => w.root === opened.root);
  const list = idx === -1 ? [...open, opened] : open.map((w) => (w.root === opened.root ? opened : w));
  return { list, activeRoot: opened.root };
}

/**
 * Remove a workspace from the open set and decide which tab is active next.
 *
 * When the closed tab was the active one, the neighbour that slid into its slot
 * leads — the tab now at the closed index, or the new last tab if it was the
 * final one — so focus does not jump to the far end of the strip. Closing a
 * background tab leaves the active one where it is. Closing the last tab yields
 * `null`, which the app renders as the welcome screen. This is the same rule as
 * `nextActiveAfterDelete` in `notesLogic.ts`.
 */
export function closeOpenWorkspace(
  open: Workspace[],
  closedRoot: string,
  activeRoot: string | null,
): { list: Workspace[]; activeRoot: string | null } {
  const list = open.filter((w) => w.root !== closedRoot);
  if (list.length === 0) return { list, activeRoot: null };
  if (activeRoot !== closedRoot) return { list, activeRoot };
  const idx = open.findIndex((w) => w.root === closedRoot);
  // `list` is non-empty here (guarded above), so this index is always in range;
  // the `?? null` branch is unreachable but satisfies noUncheckedIndexedAccess.
  const next = list[Math.min(idx, list.length - 1)];
  return { list, activeRoot: next?.root ?? null };
}

/**
 * Whether a workspace tab should flash to signal that one of its terminals
 * wants attention.
 *
 * Only a **background** tab flashes: the active codebase's terminals are on
 * screen and their own minimized pill already flashes there, so re-flashing the
 * active tab would be noise. Switching to a flashing tab makes it active and the
 * flash stops on its own — the display is purely derived from these three
 * inputs, so there is no separate "clear" step to keep in sync.
 */
export function shouldFlashWorkspaceTab(
  root: string,
  activeRoot: string | null,
  hasAttention: boolean,
): boolean {
  return hasAttention && root !== activeRoot;
}

/** Split a root path into its non-empty segments, tolerating either separator. */
function segments(root: string): string[] {
  return root.split(/[\\/]/).filter(Boolean);
}

/**
 * The label to show on each workspace tab, in list order.
 *
 * The bare `name` is used when it is unique among the open workspaces. When two
 * or more share a name (two repos both called `api`), each colliding tab is
 * disambiguated by prefixing the parent directory segment its root differs by,
 * so `/one/api` and `/two/api` become `one/api` and `two/api`. Non-colliding
 * names are left untouched.
 */
export function tabLabels(open: Workspace[]): string[] {
  const counts = new Map<string, number>();
  for (const w of open) counts.set(w.name, (counts.get(w.name) ?? 0) + 1);
  return open.map((w) => {
    if ((counts.get(w.name) ?? 0) <= 1) return w.name;
    const segs = segments(w.root);
    // The parent of the root (the segment before the basename) is what two
    // same-named repos differ by; fall back to the bare name if there is none.
    const parent = segs.length >= 2 ? segs[segs.length - 2] : undefined;
    return parent ? `${parent}/${w.name}` : w.name;
  });
}

/**
 * What a background workspace tab is signalling.
 *
 * Four states, deliberately distinct rather than collapsed into one "something
 * happened" flag, because they answer different questions and want different
 * reactions: a build that broke is worth interrupting yourself for, a terminal
 * that finished is not.
 *
 * - `error` — a build, rebuild or clean in that codebase failed.
 * - `attention` — a minimized terminal there rang the bell (the original, and
 *   still the only thing a *running* terminal can say).
 * - `success` — a build, rebuild or clean there succeeded.
 * - `done` — a minimized terminal there exited. Transient: it pulses twice and
 *   stops, because "it finished" goes stale the moment you have seen it, and
 *   an outline that persisted would still be there tomorrow.
 */
export type TabSignal = "error" | "attention" | "success" | "done";

/**
 * Rank, highest first. `error` outranks everything because a broken build is
 * the only one of these you cannot choose to ignore; `done` ranks lowest
 * because it is the only one that expires on its own.
 */
const SIGNAL_PRIORITY: Record<TabSignal, number> = {
  error: 4,
  attention: 3,
  success: 2,
  done: 1,
};

/**
 * Fold a new signal into whatever a tab was already showing.
 *
 * A weaker signal never masks a stronger one: a terminal finishing after a
 * build broke must not turn the tab from red to green, because the broken
 * build is still broken and the tab is the only place that is said. The user
 * clearing the tab (by clicking it) is the only thing that lowers the state.
 */
export function mergeSignal(current: TabSignal | null | undefined, incoming: TabSignal): TabSignal {
  if (!current) return incoming;
  return SIGNAL_PRIORITY[incoming] > SIGNAL_PRIORITY[current] ? incoming : current;
}

/**
 * The class suffix a signal renders as. `attention` is abbreviated because the
 * CSS class it pairs with (`.ws-tab.signal-attn`) predates the other three as
 * `.ws-tab.attention`, and a full word there reads as the old boolean.
 */
const SIGNAL_CLASS: Record<TabSignal, string> = {
  error: "signal-error",
  attention: "signal-attn",
  success: "signal-success",
  done: "signal-done",
};

/**
 * The classes a workspace tab wears for its current signal — empty when it
 * should not be flashing at all.
 *
 * Reuses {@link shouldFlashWorkspaceTab}'s rule that only a **background** tab
 * flashes. The active codebase is on screen: its terminals flash their own
 * pills and its build output is right there, so re-flashing its tab would be
 * noise. Unlike the original bell flash this is not purely derived — the
 * caller must drop the signal when the tab is activated — because a `success`
 * or `error` that survived being looked at would flash again the moment you
 * switched away.
 */
export function tabSignalClass(
  root: string,
  activeRoot: string | null,
  signal: TabSignal | null | undefined,
): string {
  if (!signal) return "";
  if (!shouldFlashWorkspaceTab(root, activeRoot, true)) return "";
  return ` signal ${SIGNAL_CLASS[signal]}`;
}
