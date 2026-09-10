//! Pure decisions for the app-wide floating-panel focus order: which panel is in
//! front, and the raise step each renders at. One order spans every floating
//! panel — terminals, the embedded browser, Notes — so "last clicked is on top"
//! is a single fact rather than three private ones, and both a panel's z-index
//! and the browser page's occlusion derive from it.
//!
//! Extracted so it is testable without a DOM (vitest runs in the node
//! environment). The React plumbing that drives it lives in `focusOrderContext`
//! and the panels themselves; it decides nothing.
//!
//! These are the generalisation of the terminal-only stack helpers that used to
//! live in `terminalLogic` — same semantics, no longer terminal-named.
//! `terminalLogic` re-exports them under the old names so its callers and tests
//! are untouched.

/**
 * How many raise steps the stylesheet reserves for the floating-panel band.
 *
 * Pinned by a test against `--z-panel-stack-span` in `styles.css`, which is the
 * only other place this number appears: CSS owns the band base and this owns the
 * ordinal within it, so no z-index integer is ever written in TypeScript. The
 * clamp lives here because it is a decision, and decisions are tested.
 */
export const FOCUS_STACK_SPAN = 100;

/**
 * Bring one panel to the front of the focus order.
 *
 * The order is a list of panel ids, bottom-most first. Returns the **same array**
 * when the id is already top, so the caller's `setState` bails out and clicking
 * the front panel — much the commonest case — costs no render at all.
 */
export function raiseFocus(order: string[], id: string): string[] {
  if (order.length > 0 && order[order.length - 1] === id) return order;
  return [...order.filter((k) => k !== id), id];
}

/**
 * Reconcile the focus order against the panels that are actually present: drop
 * ids no longer registered, append newly registered ones (so a fresh panel starts
 * on top), and otherwise **leave the order alone**.
 *
 * Returns the same array when nothing changed, which is what stops the effect
 * that calls it from looping. Deliberately not persisted across restarts: floating
 * panels do not survive one, and their ids are minted fresh each session.
 */
export function syncFocusOrder(order: string[], present: string[]): string[] {
  const live = new Set(present);
  const kept = order.filter((k) => live.has(k));
  const known = new Set(kept);
  const added = present.filter((k) => !known.has(k));

  if (added.length === 0 && kept.length === order.length) return order;
  return [...kept, ...added];
}

/**
 * The raise step a panel renders at: 0 for the bottom of the order, rising to the
 * top. Clamped into `FOCUS_STACK_SPAN` so a very long-lived session can never
 * climb a panel out of its band and over the dock/overlays; the clamp collapses
 * the *bottom* of an absurd stack, never the top.
 *
 * An id the order has not seen yet — a panel rendered in the commit before the
 * reconciling effect runs — sits at the bottom rather than yielding `NaN`.
 */
export function focusOffset(order: string[], id: string): number {
  const index = order.indexOf(id);
  if (index < 0) return 0;
  const excess = Math.max(0, order.length - FOCUS_STACK_SPAN);
  return Math.max(0, index - excess);
}
