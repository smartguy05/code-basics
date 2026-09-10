// Pure decisions for the floating Tasks panel: when it is mounted, when a fresh
// open should restore a minimized one, and where its layout is remembered.
// Extracted so they are testable in the node environment — `TasksPanel.tsx` is a
// rendering shell and decides nothing, and `WorkspaceTab` only applies what this
// returns.
//
// A verbatim sibling of `sqlPanelLogic.ts`: the Tasks panel has the same
// lifecycle as the SQL console (one per codebase, minimizes rather than closes,
// re-opened by a request token), so it reuses the same shape rather than
// inventing a second one.

/**
 * The localStorage key the Tasks panel persists its position and size under.
 * Follows the `cb.<thing>.layout` convention shared with the SQL console
 * (`cb.sql.layout`), Notes (`cb.notes.layout`) and the terminals.
 *
 * Unscoped by workspace, like the SQL console's: there is one Tasks panel per
 * open codebase but they are the same window in the user's mind, and a panel
 * dragged somewhere comfortable should open there in every repository.
 */
export const TASKS_LAYOUT_KEY = "cb.tasks.layout";

/**
 * Whether the Tasks panel is open, and a token that changes on each *request* to
 * open it.
 *
 * The token exists because "open the Tasks panel" has two meanings once the
 * panel can be minimized: mount it, or bring the already-mounted one back. The
 * boolean cannot express the second — re-opening an open panel changes no field
 * a child could compare — so the token carries it, the same request-and-consume
 * shape `App` uses for `openRequest`/`selectRequest`.
 */
export interface TasksPanelState {
  open: boolean;
  restoreToken: number;
}

/** The initial state: no panel, no request yet. */
export const CLOSED_TASKS_PANEL: TasksPanelState = { open: false, restoreToken: 0 };

/**
 * Ask for the Tasks panel. Always advances the token, so a request that finds
 * the panel already open still reaches it as a restore.
 */
export function openTasksPanel(state: TasksPanelState): TasksPanelState {
  return { open: true, restoreToken: state.restoreToken + 1 };
}

/**
 * Close the panel (the ✕ on its header), unmounting it.
 *
 * The token is *kept*, not reset: it counts requests, and a later open must
 * still differ from every value the panel has already seen. Returns the same
 * object when there was nothing open, so a stray close costs no render.
 */
export function closeTasksPanel(state: TasksPanelState): TasksPanelState {
  if (!state.open) return state;
  return { open: false, restoreToken: state.restoreToken };
}

/**
 * The panel state after the optional-feature gate has answered.
 *
 * Switching the Tasks feature **off** must unmount the panel, not merely hide
 * it. Hidden-but-mounted is right for a panel the user can reopen in one click;
 * a disabled feature is not that. Returns the same object whenever nothing
 * changes (enabled, or already closed), so the effect that applies it cannot
 * loop.
 */
export function tasksPanelAfterFeatureChange(
  state: TasksPanelState,
  enabled: boolean,
): TasksPanelState {
  if (enabled) return state;
  return closeTasksPanel(state);
}

/**
 * Whether the Tasks panel should be rendered at all.
 *
 * Two independent conditions and not one: the user has asked for it, *and* the
 * feature is on. Asked separately so the mount gate and the opener's gate can
 * never disagree about a feature that was switched off while the panel was up.
 */
export function tasksPanelMounted(state: TasksPanelState, enabled: boolean): boolean {
  return state.open && enabled;
}
