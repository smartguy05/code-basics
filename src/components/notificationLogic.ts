//! The app's notification service: what deserves a notification, what it says,
//! and how the stack of them behaves.
//!
//! Global rather than per-workspace, and that is the point of it. The first
//! thing it exists to report — a long-running service dying — belongs to no
//! codebase in particular: a launcher entry's `cwd` may sit outside every open
//! workspace, and the tab-signal mechanism (`workspaceTabsLogic`) can only
//! outline a *workspace tab*. Something that is nobody's tab had nowhere to be
//! said before this.
//!
//! Everything here is pure, so the rules are testable in vitest's node
//! environment. `NotificationHost.tsx` renders and decides nothing; `App` owns
//! the list and mints the ids.

import type { ProcessEvent } from "../ipc/types";

/**
 * How loud a notification is, and — through {@link notificationTtlMs} — how
 * long it stays.
 *
 * Three levels rather than two because "it worked" and "you should look at
 * this" want opposite lifetimes, and collapsing them would either nag about
 * successes or quietly discard failures.
 */
export type NotificationKind = "error" | "warning" | "info";

export interface AppNotification {
  /** Unique per notification. Minted by the caller — this module has no clock. */
  id: string;
  kind: NotificationKind;
  title: string;
  /** The second line, when there is more worth saying than the title holds. */
  detail?: string;
  /**
   * What this is *about*, when re-notifying about the same thing should replace
   * rather than stack — a service that crash-loops must not bury the app in
   * toasts. Absent means every one of these is its own event.
   */
  dedupeKey?: string;
}

/**
 * How many notifications are kept on screen at once.
 *
 * Small on purpose: this is an interruption surface, and a stack tall enough to
 * need scrolling has stopped being one. The **oldest** are dropped when it
 * overflows, because the newest is the one that just happened.
 */
export const MAX_NOTIFICATIONS = 4;

/**
 * How long a notification stays before dismissing itself, or `null` for "until
 * the user dismisses it".
 *
 * An error never auto-dismisses. The whole reason this service exists is that a
 * service died while the user was looking at something else — a message that
 * removes itself after four seconds would lose exactly the case it was built
 * for. Successes and informational notes do go away on their own; leaving them
 * up would train the user to dismiss without reading, which costs the errors
 * their only advantage.
 */
export function notificationTtlMs(kind: NotificationKind): number | null {
  switch (kind) {
    case "error":
      return null;
    case "warning":
      return 12_000;
    case "info":
      return 5_000;
  }
}

/**
 * Whether a launched process ending deserves a notification.
 *
 * Only for an entry the user marked **persistent**. That flag is a statement
 * about intent — "this is a service, it is supposed to stay up" — so *any* end
 * to it is news, including a clean `exit 0`. A one-shot command finishing is
 * not news, and notifying about it would make the surface worthless.
 *
 * `cancelled` is the one exclusion, and it is the important one: it means the
 * user pressed Stop. Telling someone that the thing they just stopped has
 * stopped is noise of the worst kind — it teaches them the notifications are
 * not worth reading.
 *
 * Note this deliberately does **not** consult the exit code. `exited` with
 * `success: true` still notifies, because a service that exits cleanly on its
 * own is behaving *unexpectedly* even though it is behaving correctly. That
 * distinction — expected-by-the-process versus expected-by-the-user — is the
 * whole content of the `persistent` flag.
 */
export function notifiesUnexpectedStop(event: ProcessEvent, persistent: boolean): boolean {
  if (!persistent) return false;
  if (event.type === "failed") return true;
  return event.type === "exited" && !event.cancelled;
}

/**
 * What to say about a persistent service that stopped.
 *
 * The exit code is included when there is one and withheld when there is not —
 * `code: null` means the process was signalled or the runtime could not report
 * one, and printing "exited with code null" would be worse than saying nothing.
 */
export function describeUnexpectedStop(
  label: string,
  event: ProcessEvent,
): { kind: NotificationKind; title: string; detail: string } {
  if (event.type === "failed") {
    return {
      kind: "error",
      title: `${label} could not start`,
      detail: event.message,
    };
  }
  const code = event.type === "exited" ? event.code : null;
  const clean = event.type === "exited" && event.success;
  return {
    kind: clean ? "warning" : "error",
    title: `${label} stopped`,
    detail: clean
      ? // Said explicitly, because "it exited cleanly" reads like good news and
        // the user asked to be told precisely because it is not.
        "It exited on its own. This is a service, so it was expected to keep running."
      : code === null
        ? "It stopped unexpectedly."
        : `It stopped unexpectedly (exit code ${code}).`,
  };
}

/**
 * Add a notification, replacing any earlier one about the same thing and
 * dropping the oldest if the stack is full.
 *
 * Replacing by `dedupeKey` is what makes a crash-looping service survivable:
 * one entry that keeps updating, rather than a new toast every restart. The
 * replacement goes to the **end** — it is the most recent thing that happened,
 * and leaving it in its old position would let a service that failed an hour ago
 * and again just now sit quietly at the bottom of the stack.
 */
export function pushNotification(
  list: readonly AppNotification[],
  next: AppNotification,
): AppNotification[] {
  const kept = next.dedupeKey
    ? list.filter((n) => n.dedupeKey !== next.dedupeKey)
    : [...list];
  kept.push(next);
  return kept.slice(Math.max(0, kept.length - MAX_NOTIFICATIONS));
}

/** Remove one notification by id. Returns the same array when nothing matched. */
export function dismissNotification(
  list: readonly AppNotification[],
  id: string,
): AppNotification[] {
  const kept = list.filter((n) => n.id !== id);
  // The same reference on a no-op, so a dismiss for something already gone
  // costs no render.
  return kept.length === list.length ? (list as AppNotification[]) : kept;
}
