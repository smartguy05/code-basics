// Decision logic for the MCP settings page: which servers and tools exist, and
// the derived state a checkbox list needs. Pure so it can be unit-tested in the
// node environment (no DOM); the page is a rendering shell.
//
// The store is `cb_core::tool_gate`, and the enabled state is already resolved by
// the backend (an absent choice is reported as enabled) — this module never
// invents a default, it only derives from what was reported.

import type { McpServerToolsInfo, McpToolInfo } from "../ipc/types";

/** A server's aggregate enabled state, for a select-all control. */
export type ServerToggleState = "all" | "none" | "some";

/**
 * The aggregate state of one server's tools.
 *
 * A server with no tools reads as `all` (nothing is off), so a select-all box is
 * checked rather than indeterminate for an empty server.
 */
export function serverToggleState(server: McpServerToolsInfo): ServerToggleState {
  const total = server.tools.length;
  if (total === 0) return "all";
  const on = server.tools.filter((t) => t.enabled).length;
  if (on === total) return "all";
  if (on === 0) return "none";
  return "some";
}

/** How many of a server's tools are enabled. */
export function enabledCount(server: McpServerToolsInfo): number {
  return server.tools.filter((t) => t.enabled).length;
}

/**
 * The next enabled value a server-level toggle should apply to every tool.
 *
 * Clicking a select-all that is fully or partially on turns everything **off**;
 * clicking one that is fully off turns everything **on**. So `some` and `all`
 * both go to off, and only `none` goes to on — the least-surprising behaviour for
 * a tri-state box (a click clears a mixed set rather than filling it).
 */
export function nextServerValue(state: ServerToggleState): boolean {
  return state === "none";
}

/**
 * Whether a tool row should read as disabled-by-its-server rather than by its own
 * switch.
 *
 * `serverEnabled === null` means the feature list has not arrived; treat the
 * server as on (the same optimistic default `featureEnabled` uses), so nothing
 * flashes as blocked during startup. A server switched off through its feature
 * flag makes its tools moot — the whole server is not installed — so the page
 * shows them greyed with the server as the reason.
 */
export function toolBlockedByServer(serverEnabled: boolean | null): boolean {
  return serverEnabled === false;
}

/** A stable key for a tool row: the server id plus the tool name. */
export function toolKey(serverId: string, tool: McpToolInfo): string {
  return `${serverId}/${tool.name}`;
}
