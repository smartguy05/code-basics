import { describe, expect, it } from "vitest";
import { pluginMenuAvailable, pluginMenuRows } from "./pluginMenuLogic";
import { PLUGIN_LABELS } from "../shortcutLogic";
import type { FeatureInfo } from "../ipc/types";

const feature = (id: string, enabled: boolean): FeatureInfo => ({
  id,
  label: id,
  description: "",
  enabled,
});

const ALL_KEYS = ["sqlConsole", "askCodebase", "mcpSqlServer", "webBrowser", "tasks"];
const ON = ALL_KEYS.map((f) => feature(f, true));
const OFF = ALL_KEYS.map((f) => feature(f, false));
/** Exactly one feature on, so a test can tell the rows apart. */
const only = (id: string) => ALL_KEYS.map((f) => feature(f, f === id));
/**
 * The row ids with the always-on Roslyn server dropped, so a test naming one
 * optional feature can assert exactly its rows. The Roslyn row's own behaviour
 * is covered by its dedicated block.
 */
const optionalIds = (rows: { id: string }[]) =>
  rows.map((r) => r.id).filter((id) => id !== "plugin.roslyn");

const SQL_ONLY = only("sqlConsole");
const ASK_ONLY = only("askCodebase");
const MCP_ONLY = only("mcpSqlServer");
const BROWSER_ONLY = only("webBrowser");
const TASKS_ONLY = only("tasks");

describe("pluginMenuRows", () => {
  it("offers the SQL console when its feature is on and a codebase is open", () => {
    const rows = pluginMenuRows({ features: SQL_ONLY, workspaceOpen: true });
    expect(optionalIds(rows)).toEqual(["view.sql"]);
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "sql" });
  });

  it("offers Ask the codebase, which was reachable only by its chord before", () => {
    const rows = pluginMenuRows({ features: ASK_ONLY, workspaceOpen: true });
    expect(optionalIds(rows)).toEqual(["agent.ask"]);
    expect(rows[0]?.label).toBe("Ask the codebase");
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "ask" });
  });

  it("lists every plugin when all are on", () => {
    // The Roslyn MCP server is always-on (the LSP is not an optional feature),
    // so it is always the last row regardless of which features are enabled.
    expect(pluginMenuRows({ features: ON, workspaceOpen: true }).map((r) => r.id)).toEqual([
      "view.sql",
      "agent.ask",
      "plugin.mcp",
      "plugin.browser",
      "plugin.tasks",
      "plugin.roslyn",
    ]);
  });

  it("offers the SQL MCP server when its feature is on and a codebase is open", () => {
    const rows = pluginMenuRows({ features: MCP_ONLY, workspaceOpen: true });
    expect(optionalIds(rows)).toEqual(["plugin.mcp"]);
    expect(rows[0]?.label).toBe("SQL MCP server");
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "mcp" });
  });

  it("omits the SQL MCP server when its feature is off", () => {
    expect(pluginMenuRows({ features: SQL_ONLY, workspaceOpen: true }).map((r) => r.id)).not.toContain(
      "plugin.mcp",
    );
  });

  it("disables the SQL MCP server, with a reason, when no codebase is open", () => {
    // A project-scope install writes .mcp.json at a repository root, so there is
    // no repository to write it to.
    const rows = pluginMenuRows({ features: MCP_ONLY, workspaceOpen: false });
    expect(rows[0]?.disabled).toBe(true);
    expect(rows[0]?.action).toBe(null);
    expect(rows[0]?.title).toContain("Open a codebase");
  });

  it("takes every label from PLUGIN_LABELS rather than showing a raw id", () => {
    // A plugin with a command tagged for it but no label entry would otherwise
    // render as `mcpSqlServer` in the menu and read as a bug.
    const labels = Object.values(PLUGIN_LABELS);
    for (const row of pluginMenuRows({ features: ON, workspaceOpen: true })) {
      expect(labels).toContain(row.label);
    }
  });

  it("omits Ask the codebase on its own when only that feature is off", () => {
    expect(optionalIds(pluginMenuRows({ features: SQL_ONLY, workspaceOpen: true }))).toEqual([
      "view.sql",
    ]);
  });

  it("disables Ask the codebase, with a reason, when no codebase is open", () => {
    // It asks *about the open codebase*, so there is nothing for it to read.
    const rows = pluginMenuRows({ features: ASK_ONLY, workspaceOpen: false });
    expect(rows[0]?.disabled).toBe(true);
    expect(rows[0]?.action).toBe(null);
    expect(rows[0]?.title).toContain("Open a codebase");
  });

  it("uses the same name Settings uses", () => {
    // The shortcut list and this menu read one label map, so a plugin cannot end
    // up called two different things in two places.
    expect(pluginMenuRows({ features: SQL_ONLY, workspaceOpen: true })[0]?.label).toBe(
      "SQL Console",
    );
  });

  it("omits a switched-off plugin rather than disabling it", () => {
    // "You switched this off" is a decision the user already made and does not
    // need arguing with. Only the always-on Roslyn server survives every
    // optional feature being off.
    expect(pluginMenuRows({ features: OFF, workspaceOpen: true }).map((r) => r.id)).toEqual([
      "plugin.roslyn",
    ]);
  });

  it("disables — and explains — a plugin that needs a codebase when none is open", () => {
    // Unlike a switched-off feature, this is a state the user is one click from
    // leaving, so it is worth showing with a reason.
    const rows = pluginMenuRows({ features: SQL_ONLY, workspaceOpen: false });
    expect(optionalIds(rows)).toEqual(["view.sql"]);
    expect(rows[0]?.disabled).toBe(true);
    expect(rows[0]?.title).toContain("Open a codebase");
  });

  it("never leaves an action on a disabled row", () => {
    // So a caller cannot act on a row the menu is refusing.
    for (const workspaceOpen of [true, false]) {
      for (const row of pluginMenuRows({ features: ON, workspaceOpen })) {
        expect(row.action === null).toBe(row.disabled);
      }
    }
  });

  it("always gives a row something to say", () => {
    for (const workspaceOpen of [true, false]) {
      for (const row of pluginMenuRows({ features: ON, workspaceOpen })) {
        expect(row.title.length).toBeGreaterThan(0);
      }
    }
  });

  it("enables the browser when a codebase is open", () => {
    // The browser is now truly per-workspace: each open codebase keeps its own
    // live page and only the active one is visible. So it needs a codebase to
    // attach to, like every other plugin.
    const rows = pluginMenuRows({ features: BROWSER_ONLY, workspaceOpen: true });
    expect(optionalIds(rows)).toEqual(["plugin.browser"]);
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "browser" });
    expect(rows[0]?.label).toBe("Web browser");
  });

  it("disables the browser, with a reason, when no codebase is open", () => {
    // A per-workspace browser has no workspace to open into on the welcome
    // screen, so it becomes a disabled row that explains itself rather than an
    // opener that acts on nothing.
    const rows = pluginMenuRows({ features: BROWSER_ONLY, workspaceOpen: false });
    expect(optionalIds(rows)).toEqual(["plugin.browser"]);
    expect(rows[0]?.disabled).toBe(true);
    expect(rows[0]?.action).toBe(null);
    expect(rows[0]?.title).toContain("Open a codebase");
  });

  it("offers the Tasks panel when its feature is on and a codebase is open", () => {
    const rows = pluginMenuRows({ features: TASKS_ONLY, workspaceOpen: true });
    expect(optionalIds(rows)).toEqual(["plugin.tasks"]);
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "tasks" });
    expect(rows[0]?.label).toBe("Tasks");
  });

  it("disables the Tasks panel, with a reason, when no codebase is open", () => {
    // The task store is per-repository, so there must be a codebase to read it.
    const rows = pluginMenuRows({ features: TASKS_ONLY, workspaceOpen: false });
    expect(optionalIds(rows)).toEqual(["plugin.tasks"]);
    expect(rows[0]?.disabled).toBe(true);
    expect(rows[0]?.action).toBe(null);
    expect(rows[0]?.title).toContain("Open a codebase");
  });

  it("still shows the Plugins button with only the browser on and nothing open", () => {
    // A disabled row still explains itself, so the menu is worth opening.
    expect(pluginMenuAvailable({ features: BROWSER_ONLY, workspaceOpen: false })).toBe(true);
  });

  it("shows nothing while the features are still being read", () => {
    // `null` is "not read yet", not "none are on". Flashing an empty menu on
    // startup and then filling it would be a wrong answer shown confidently.
    expect(pluginMenuRows({ features: null, workspaceOpen: true })).toEqual([]);
  });
});

describe("Roslyn MCP server (always-on)", () => {
  it("offers the Roslyn server even when every optional feature is off", () => {
    // The LSP is always running, so its installer has no feature gate: it is
    // present whatever the optional features are set to.
    const rows = pluginMenuRows({ features: OFF, workspaceOpen: true });
    const roslyn = rows.find((r) => r.id === "plugin.roslyn");
    expect(roslyn?.label).toBe("Roslyn MCP server");
    expect(roslyn?.disabled).toBe(false);
    expect(roslyn?.action).toEqual({ kind: "roslynMcp" });
  });

  it("disables the Roslyn server, with a reason, when no codebase is open", () => {
    // A project-scope install writes .mcp.json at a repository root, and the
    // status is read per repository, so there must be a codebase.
    const rows = pluginMenuRows({ features: OFF, workspaceOpen: false });
    const roslyn = rows.find((r) => r.id === "plugin.roslyn");
    expect(roslyn?.disabled).toBe(true);
    expect(roslyn?.action).toBe(null);
    expect(roslyn?.title).toContain("Open a codebase");
  });

  it("is still gone while the features are being read", () => {
    // `null` is "not read yet", and short-circuits before any row — including
    // the always-on one — so the menu never flashes on startup.
    expect(pluginMenuRows({ features: null, workspaceOpen: true })).toEqual([]);
  });
});

describe("pluginMenuAvailable", () => {
  it("shows the button even when every optional feature is off", () => {
    // The always-on Roslyn server means there is always at least one row, so the
    // Plugins button is now always worth showing once the features have loaded.
    expect(pluginMenuAvailable({ features: OFF, workspaceOpen: true })).toBe(true);
  });

  it("hides it before the features have been read", () => {
    expect(pluginMenuAvailable({ features: null, workspaceOpen: true })).toBe(false);
  });

  it("shows it for a row that exists but is currently refused", () => {
    // A disabled row still explains itself, so the menu is worth opening.
    expect(pluginMenuAvailable({ features: ON, workspaceOpen: false })).toBe(true);
  });
});
