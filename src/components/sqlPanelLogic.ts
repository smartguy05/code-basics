// Pure decisions for the floating SQL console panel: when it is mounted, when a
// fresh open should restore a minimized one, and where its layout is remembered.
// Extracted so they are testable in the node environment — `SqlPanel.tsx` is a
// rendering shell and decides nothing, and `WorkspaceTab` only applies what this
// returns.

/**
 * The localStorage key the SQL panel persists its position and size under.
 * Follows the `cb.<thing>.layout` convention shared with the agent panel
 * (`cb.agentPanel.layout`), Notes (`cb.notes.layout`) and the terminals.
 *
 * Unscoped by workspace, unlike the terminals': there is one SQL panel per open
 * codebase but they are the same window in the user's mind, and a console
 * dragged somewhere comfortable should open there in every repository.
 */
export const SQL_LAYOUT_KEY = "cb.sql.layout";

/**
 * Whether the SQL panel is open, and a token that changes on each *request* to
 * open it.
 *
 * The token exists because "open the SQL console" has two meanings once the
 * panel can be minimized: mount it, or bring the already-mounted one back. The
 * boolean cannot express the second — re-opening an open panel changes no field
 * a child could compare — so the token carries it, the same request-and-consume
 * shape `App` uses for `openRequest`/`selectRequest`.
 */
export interface SqlPanelState {
  open: boolean;
  restoreToken: number;
}

/** The initial state: no panel, no request yet. */
export const CLOSED_SQL_PANEL: SqlPanelState = { open: false, restoreToken: 0 };

/**
 * Ask for the SQL console. Always advances the token, so a request that finds
 * the panel already open still reaches it as a restore.
 */
export function openSqlPanel(state: SqlPanelState): SqlPanelState {
  return { open: true, restoreToken: state.restoreToken + 1 };
}

/**
 * Close the panel (the ✕ on its header), unmounting the console.
 *
 * The token is *kept*, not reset: it counts requests, and a later open must
 * still differ from every value the panel has already seen. Returns the same
 * object when there was nothing open, so a stray close costs no render.
 */
export function closeSqlPanel(state: SqlPanelState): SqlPanelState {
  if (!state.open) return state;
  return { open: false, restoreToken: state.restoreToken };
}

/**
 * The panel state after the optional-feature gate has answered.
 *
 * Switching the SQL console **off** must unmount it, not merely hide it. Hidden-
 * but-mounted is right for a panel the user can reopen in one click; a disabled
 * feature is not that. Left mounted the console would keep its CodeMirror
 * document alive, keep its `sql_list_connections` result, and keep a query
 * streaming against a live database with no route to the rows and no route to
 * Stop — the user switched the console off and the console kept running.
 *
 * Returns the same object whenever nothing changes (enabled, or already closed),
 * so the effect that applies it cannot loop.
 */
export function sqlPanelAfterFeatureChange(
  state: SqlPanelState,
  enabled: boolean,
): SqlPanelState {
  if (enabled) return state;
  return closeSqlPanel(state);
}

/**
 * Whether the SQL panel should be rendered at all.
 *
 * Two independent conditions and not one: the user has asked for it, *and* the
 * feature is on. Asked separately so the mount gate and the opener's gate can
 * never disagree about a feature that was switched off while the panel was up.
 */
export function sqlPanelMounted(state: SqlPanelState, enabled: boolean): boolean {
  return state.open && enabled;
}
