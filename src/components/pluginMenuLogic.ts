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
  | { kind: "browser" }
  | { kind: "tasks" }
  | { kind: "roslynMcp" }
  | { kind: "editorMcp" }
  | { kind: "redis" };

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
  /**
   * The optional feature that gates the row, or `null` for an **always-on**
   * plugin — one backed by something the app always runs, so there is no feature
   * to switch off. The Roslyn/LSP server is the first of these: the language
   * server is always warm, so its installer is unconditional.
   */
  feature: FeatureKey | null;
  /** The command that opens it; also the row's id, so Settings and this agree. */
  commandId: string;
  action: PluginAction;
  /** Whether it acts on the open codebase, and so needs one. */
  needsWorkspace: boolean;
  /** Shown when it can be opened. */
  ready: string;
  /** Shown when it needs a codebase and there is none. */
  noWorkspace: string;
  /**
   * The key into `PLUGIN_LABELS` for the row's name. Defaults to `feature` (a
   * `FeatureKey` is always a `PLUGIN_LABELS` key), but an always-on plugin has
   * no feature, so it names its own key here.
   */
  labelKey?: string;
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
    // Needs a codebase because the browser is truly per-workspace now (bugs
    // 6+7): each open codebase keeps its own live page and only the active one
    // is visible, so there must be a codebase to open the page into. On the
    // welcome screen it is therefore a disabled row with a reason, like every
    // other plugin, rather than an opener that would act on nothing.
    feature: "webBrowser",
    commandId: "plugin.browser",
    action: { kind: "browser" },
    needsWorkspace: true,
    ready: "Open a web page inside the active codebase",
    noWorkspace: "Open a codebase to open a web page in it",
  },
  {
    // Needs a codebase because the task list is per-repository: the file lives
    // under the opened workspace's `.code-basics/` and is gitignored, so there
    // must be a codebase for the panel to read and write.
    feature: "tasks",
    commandId: "plugin.tasks",
    action: { kind: "tasks" },
    needsWorkspace: true,
    ready: "Open the task list for the active codebase",
    noWorkspace: "Open a codebase to manage its tasks",
  },
  {
    // Feature-gated on `editorContextMcp` (unlike the always-on Roslyn server):
    // the same feature gates whether the frontend pushes editor state at all, so
    // with it off there is nothing to install against and the row is omitted.
    // Needs a codebase because the install is scoped to it (`--workspace <root>`
    // is baked into the entry) and the status is read per repository.
    feature: "editorContextMcp",
    commandId: "plugin.editorMcp",
    action: { kind: "editorMcp" },
    needsWorkspace: true,
    ready: "Let a coding agent see what you have open and selected, over MCP",
    noWorkspace: "Open a codebase to install the Editor context MCP server for it",
  },
  {
    // Always-on like the Roslyn server below (no `FeatureId`): the Redis panel
    // ships enabled and is not one of the four installer-selectable features.
    // Needs a codebase because discovery scans the open workspace's
    // appsettings/secrets for connections.
    feature: null,
    labelKey: "redisConsole",
    commandId: "view.redis",
    action: { kind: "redis" },
    needsWorkspace: true,
    ready: "Browse and edit Redis for the active codebase",
    noWorkspace: "Open a codebase to browse its Redis",
  },
  {
    // Always-on: the app keeps a warm per-workspace Roslyn/LSP session, so the
    // installer that points an agent at it has no optional feature to gate on —
    // it is present whatever the other plugins are set to. It still needs a
    // codebase, because a project-scope install writes `.mcp.json` at a
    // repository root and the status is read per repository.
    feature: null,
    labelKey: "mcpRoslyn",
    commandId: "plugin.roslyn",
    action: { kind: "roslynMcp" },
    needsWorkspace: true,
    ready: "Let a coding agent read this codebase's semantic model, over MCP",
    noWorkspace: "Open a codebase to install the Roslyn MCP server for it",
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
    // A `null` feature is an always-on plugin: it has nothing to switch off, so
    // it is never filtered out here.
    if (plugin.feature !== null && !featureEnabled(state.features, plugin.feature)) continue;
    const labelKey = plugin.labelKey ?? plugin.feature ?? plugin.commandId;
    const blocked = plugin.needsWorkspace && !state.workspaceOpen;
    rows.push({
      id: plugin.commandId,
      label: PLUGIN_LABELS[labelKey] ?? labelKey,
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
