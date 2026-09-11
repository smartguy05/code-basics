// Pure decisions for the floating Redis panel: open/close and the restore token.
// Modelled on `sqlPanelLogic`, minus the optional-feature gate — the Redis plugin
// is always-on (like the Roslyn server), so there is no feature to switch off and
// `redisPanelMounted` is just `state.open`.

/** The localStorage key the Redis panel persists its position and size under. */
export const REDIS_LAYOUT_KEY = "cb.redis.layout";

/** Whether the panel is open, and a token that changes on each open *request*. */
export interface RedisPanelState {
  open: boolean;
  restoreToken: number;
}

export const CLOSED_REDIS_PANEL: RedisPanelState = { open: false, restoreToken: 0 };

/** Ask for the panel. Always advances the token so a request reaches an
 *  already-open panel as a restore. */
export function openRedisPanel(state: RedisPanelState): RedisPanelState {
  return { open: true, restoreToken: state.restoreToken + 1 };
}

/** Close the panel. The token is kept (it counts requests). */
export function closeRedisPanel(state: RedisPanelState): RedisPanelState {
  if (!state.open) return state;
  return { open: false, restoreToken: state.restoreToken };
}

/** Whether the panel should be rendered. Always-on, so just "asked for". */
export function redisPanelMounted(state: RedisPanelState): boolean {
  return state.open;
}
