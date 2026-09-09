//! The **Plugins** titlebar menu: what the optional features offer, and whether
//! each one can be opened right now.
//!
//! The SQL console used to sit in the terminal split button's menu, beside New
//! Terminal and Launch. That was where it fit at the time and not where it
//! belongs: those rows are all *ways of running something*, and a database
//! console is not one of them. Pulling it out gives the optional features a
//! surface of their own, so the next one is a row here rather than another
//! guest in a menu about processes.
//!
//! Everything is pure, so the rules are testable in vitest's node environment.
//! `App` renders the rows and decides nothing.

import { featureEnabled, type FeatureKey } from "./featuresLogic";
import { PLUGIN_LABELS } from "../shortcutLogic";
import type { FeatureInfo } from "../ipc/types";

/** What clicking a plugin row does. One variant per plugin. */
export type PluginAction =
  | { kind: "sql" }
  | { kind: "ask" }
  | { kind: "mcp" }
  | { kind: "browser" };

export interface PluginRow {
  /** The command id this row corresponds to, and the React key. */
  id: string;
  label: string;
  /** Why the row is unavailable, or what it does — always something to show. */
  title: string;
  disabled: boolean;
  /** `null` exactly when `disabled`, so a caller cannot act on a refused row. */
  action: PluginAction | null;
}

/**
 * One plugin's entry in the menu.
 *
 * A table rather than a chain of `if`s so that adding a plugin is a row here and
 * nothing else — which is the whole reason this menu exists rather than a
 * hard-coded SQL button.
 */
interface PluginEntry {
  feature: FeatureKey;
  /** The command that opens it; also the row's id, so Settings and this agree. */
  commandId: string;
  action: PluginAction;
  /** Whether it acts on the open codebase, and so needs one. */
  needsWorkspace: boolean;
  /** Shown when it can be opened. */
  ready: string;
  /** Shown when it needs a codebase and there is none. */
  noWorkspace: string;
}

const PLUGINS: PluginEntry[] = [
  {
    feature: "sqlConsole",
    commandId: "view.sql",
    action: { kind: "sql" },
    needsWorkspace: true,
    ready: "Open the SQL console for the active codebase",
    noWorkspace: "Open a codebase to query its databases",
  },
  {
    // Reachable only by its chord (Ctrl+/) until now, despite already having a
    // `PLUGIN_LABELS` entry and a `plugin:` tag on its command — so the menu
    // that exists to give the optional features a surface silently omitted the
    // one feature nothing else advertises. The chord keeps working.
    feature: "askCodebase",
    commandId: "agent.ask",
    action: { kind: "ask" },
    needsWorkspace: true,
    ready: "Ask an agent a question about the active codebase",
    noWorkspace: "Open a codebase to ask a question about it",
  },
  {
    // Needs a codebase because the project-scope install writes `.mcp.json` at a
    // repository root and the status is read per repository. The panel itself
    // could open without one, but it would then be able to offer only half of
    // what it exists to offer, with no way to say why.
    feature: "mcpSqlServer",
    commandId: "plugin.mcp",
    action: { kind: "mcp" },
    needsWorkspace: true,
    ready: "Let a coding agent read the databases you expose, over MCP",
    noWorkspace: "Open a codebase to install the SQL MCP server for it",
  },
  {
    // The first row with `needsWorkspace: false`, and the reason is the same one
    // that makes the panel app-level rather than per-`WorkspaceTab`: "does my
    // deployment work" is not a question about a repository, and there is one
    // browser for the whole application. So this row is enabled on the welcome
    // screen, which is a state no other plugin has ever been openable in —
    // `a_plugin_that_needs_no_workspace_is_enabled_with_none_open` is the test.
    feature: "webBrowser",
    commandId: "plugin.browser",
    action: { kind: "browser" },
    needsWorkspace: false,
    ready: "Open a web page inside the app",
    // Unreachable while `needsWorkspace` is false, and kept rather than made
    // optional: the field is what a future change to that flag would need, and
    // an empty string would render as a row with no tooltip if it ever were.
    noWorkspace: "Open a web page inside the app",
  },
];

export interface PluginMenuState {
  /** The optional features, or `null` while the startup read is in flight. */
  features: FeatureInfo[] | null;
  /** A codebase is in the foreground. */
  workspaceOpen: boolean;
}

/**
 * The rows for the Plugins menu.
 *
 * A plugin whose feature is **off** is omitted, not disabled. The distinction is
 * the same one the terminal menu draws: "you switched this off" is a decision
 * the user already made and does not need arguing with, whereas "no codebase is
 * open" is a state they are one click from leaving and want explained. So the
 * first disappears and the second is a disabled row with a reason.
 *
 * `features` being `null` means *not read yet*, which is why it is not treated
 * as "none are on": flashing an empty menu during startup and then filling it
 * would be a wrong answer shown confidently.
 */
export function pluginMenuRows(state: PluginMenuState): PluginRow[] {
  if (state.features === null) return [];
  const rows: PluginRow[] = [];
  for (const plugin of PLUGINS) {
    if (!featureEnabled(state.features, plugin.feature)) continue;
    const blocked = plugin.needsWorkspace && !state.workspaceOpen;
    rows.push({
      id: plugin.commandId,
      label: PLUGIN_LABELS[plugin.feature] ?? plugin.feature,
      title: blocked ? plugin.noWorkspace : plugin.ready,
      disabled: blocked,
      action: blocked ? null : plugin.action,
    });
  }
  return rows;
}

/**
 * Whether the Plugins button is worth showing at all.
 *
 * Hidden when every plugin is switched off, rather than opening onto nothing. An
 * empty menu is a dead end that still costs a click to discover, and the
 * titlebar is the one place in the app where space is genuinely contested.
 *
 * Note this asks whether any row *exists*, not whether any is enabled: a
 * disabled row still explains itself, so a menu holding only those is worth
 * opening.
 */
export function pluginMenuAvailable(state: PluginMenuState): boolean {
  return pluginMenuRows(state).length > 0;
}
