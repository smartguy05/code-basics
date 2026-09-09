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
import { customLabel, type WorkspaceLabels } from "./workspaceRenameLogic";

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
 * Move an open workspace from one position in the strip to another, for
 * drag-to-reorder. `from`/`to` are indices into `open`; the dragged tab is
 * removed and re-inserted so the other tabs close the gap and shift by one.
 *
 * Out-of-range indices, or a no-op move (`from === to`), return the **same
 * array reference** — the caller can skip the state update, and a stray drag
 * event cannot churn the list. Identity is the `root`, so this never touches
 * which root is active; the caller keeps `activeRoot` as it was.
 */
export function reorderWorkspaces(open: Workspace[], from: number, to: number): Workspace[] {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= open.length ||
    to >= open.length
  ) {
    return open;
  }
  const next = [...open];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) return open;
  next.splice(to, 0, moved);
  return next;
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
 *
 * `custom` carries the names the user typed (see `workspaceRenameLogic.ts`),
 * keyed by root. A renamed tab shows that name **verbatim** and takes no part
 * in disambiguation at all — not as a candidate for a prefix, and not as
 * evidence that another tab collides. Two rules follow from that, and the
 * second is the one worth stating:
 *
 * - A renamed tab never sprouts a parent-directory prefix. The user said what
 *   this tab is called; decorating it would be the app overruling them, and the
 *   full path is still one hover away in the tab's `title`.
 * - When a custom label happens to equal another tab's *derived* name, **both**
 *   are left alone. Prefixing the derived one would mean a tab the user never
 *   touched silently changing its label — mid-keystroke, while they type into a
 *   different tab's rename box — for a reason that is not about it. The two
 *   look alike, which the user can see and fix by renaming again; the honest
 *   name is a better answer than a prefix invented from someone else's choice.
 */
export function tabLabels(open: Workspace[], custom: WorkspaceLabels = {}): string[] {
  // Counted over *derived* names only, and over ALL of them — including tabs
  // that carry a custom label. Skipping the renamed ones looks equivalent and
  // is not: renaming `/one/api` drops `api` from two to one, which un-prefixes
  // `/two/api` from `two/api` back to `api`. That is a tab the user never
  // touched changing its label because of a rename applied to a different tab,
  // which is the exact harm the rule above forbids.
  const counts = new Map<string, number>();
  for (const w of open) counts.set(w.name, (counts.get(w.name) ?? 0) + 1);

  return open.map((w) => {
    const chosen = customLabel(custom, w.root);
    if (chosen !== undefined) return chosen;
    if ((counts.get(w.name) ?? 0) <= 1) return w.name;
    return disambiguate(w, open);
  });
}

/**
 * A label for one of several same-named roots: the fewest trailing path
 * segments that tell it apart from the others it collides with.
 *
 * Walking up rather than taking the parent unconditionally, because the parent
 * is not always the segment they differ by. The same repository checked out on
 * two drives (`C:/repos/api` and `D:/repos/api`) shares its parent, so a single
 * step produces `repos/api` **twice** — two identical labels, which is what
 * disambiguation exists to prevent. The walk stops at the first depth that is
 * actually unique, and gives up at the whole root, which is unique by
 * definition since a root is a tab's identity.
 */
function disambiguate(target: Workspace, open: Workspace[]): string {
  const rivals = open.filter((w) => w !== target && w.name === target.name);
  const segs = segments(target.root);
  const rivalSegs = rivals.map((w) => segments(w.root));

  for (let depth = 2; depth <= segs.length; depth += 1) {
    const tail = segs.slice(segs.length - depth).join("/");
    const clash = rivalSegs.some((other) => other.slice(other.length - depth).join("/") === tail);
    if (!clash) return tail;
  }
  // Every rival matched at every depth: the roots are spelled identically, so
  // there is nothing left to tell them apart with. The bare name is the honest
  // answer — these really are two tabs on the same path.
  return target.name;
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

/**
 * How long a background tab flashes for one bell before it settles.
 *
 * The attention flag a terminal raises is *sticky* — it clears only when its
 * workspace is focused — and an agent TUI such as Codex rings the bell on nearly
 * every redraw, so a level-triggered flash blinks the tab forever. The pulse
 * below is edge-triggered instead: a rising edge starts a single flash of this
 * length, which then settles and does not re-arm until the tab is acknowledged
 * (focused) or the attention clears entirely. Long enough to notice, short
 * enough not to nag.
 */
export const ATTENTION_FLASH_MS = 4000;

/**
 * When each root's current attention pulse started, by root. A root's presence
 * means it has flashed for the attention it is currently holding; its absence
 * means a future rising edge is free to start a fresh flash. The value is the
 * start timestamp so {@link attentionActive} can tell an in-flight pulse from a
 * settled one without a second field.
 */
export type AttentionPulses = Record<string, number>;

/**
 * Arm a flash for `root` on the rising edge of its attention.
 *
 * Idempotent while a pulse is recorded — whether that pulse is still flashing or
 * has already settled — so the constant bells that arm the flag in the first
 * place cannot keep restarting it. Only {@link acknowledgeAttention} (on focus)
 * or a fall to no-attention frees the root to flash again. Returns the **same
 * reference** on a no-op so a caller driving React state does not re-render.
 */
export function pulseAttention(state: AttentionPulses, root: string, now: number): AttentionPulses {
  if (root in state) return state;
  return { ...state, [root]: now };
}

/** Whether `root`'s flash is still within its {@link ATTENTION_FLASH_MS} window. */
export function attentionActive(state: AttentionPulses, root: string, now: number): boolean {
  const started = state[root];
  return started !== undefined && now - started < ATTENTION_FLASH_MS;
}

/**
 * Clear `root`'s pulse so a later rising edge can flash again — on focus, or
 * when the workspace stops holding any attention. Returns the same reference
 * when there is nothing to clear.
 */
export function acknowledgeAttention(state: AttentionPulses, root: string): AttentionPulses {
  if (!(root in state)) return state;
  const next = { ...state };
  delete next[root];
  return next;
}

/**
 * Milliseconds until the soonest in-flight pulse settles, or `null` when none is
 * flashing. The caller uses it to schedule the single re-render that ends the
 * flash — a settled pulse needs no timer, and an absent one needs nothing at all.
 */
export function nextPulseExpiry(state: AttentionPulses, now: number): number | null {
  let soonest: number | null = null;
  for (const started of Object.values(state)) {
    const remaining = started + ATTENTION_FLASH_MS - now;
    if (remaining > 0 && (soonest === null || remaining < soonest)) soonest = remaining;
  }
  return soonest;
}
