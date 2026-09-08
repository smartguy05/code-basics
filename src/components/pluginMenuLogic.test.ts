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

const ON = [
  feature("sqlConsole", true),
  feature("askCodebase", true),
  feature("mcpSqlServer", true),
];
const OFF = [
  feature("sqlConsole", false),
  feature("askCodebase", false),
  feature("mcpSqlServer", false),
];
/** Exactly one feature on, so a test can tell the rows apart. */
const only = (id: string) =>
  ["sqlConsole", "askCodebase", "mcpSqlServer"].map((f) => feature(f, f === id));
const SQL_ONLY = only("sqlConsole");
const ASK_ONLY = only("askCodebase");
const MCP_ONLY = only("mcpSqlServer");

describe("pluginMenuRows", () => {
  it("offers the SQL console when its feature is on and a codebase is open", () => {
    const rows = pluginMenuRows({ features: SQL_ONLY, workspaceOpen: true });
    expect(rows.map((r) => r.id)).toEqual(["view.sql"]);
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "sql" });
  });

  it("offers Ask the codebase, which was reachable only by its chord before", () => {
    const rows = pluginMenuRows({ features: ASK_ONLY, workspaceOpen: true });
    expect(rows.map((r) => r.id)).toEqual(["agent.ask"]);
    expect(rows[0]?.label).toBe("Ask the codebase");
    expect(rows[0]?.disabled).toBe(false);
    expect(rows[0]?.action).toEqual({ kind: "ask" });
  });

  it("lists every plugin when all are on", () => {
    expect(pluginMenuRows({ features: ON, workspaceOpen: true }).map((r) => r.id)).toEqual([
      "view.sql",
      "agent.ask",
      "plugin.mcp",
    ]);
  });

  it("offers the SQL MCP server when its feature is on and a codebase is open", () => {
    const rows = pluginMenuRows({ features: MCP_ONLY, workspaceOpen: true });
    expect(rows.map((r) => r.id)).toEqual(["plugin.mcp"]);
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
    expect(pluginMenuRows({ features: SQL_ONLY, workspaceOpen: true }).map((r) => r.id)).toEqual([
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
    // need arguing with.
    expect(pluginMenuRows({ features: OFF, workspaceOpen: true })).toEqual([]);
  });

  it("disables — and explains — a plugin that needs a codebase when none is open", () => {
    // Unlike a switched-off feature, this is a state the user is one click from
    // leaving, so it is worth showing with a reason.
    const rows = pluginMenuRows({ features: SQL_ONLY, workspaceOpen: false });
    expect(rows).toHaveLength(1);
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

  it("shows nothing while the features are still being read", () => {
    // `null` is "not read yet", not "none are on". Flashing an empty menu on
    // startup and then filling it would be a wrong answer shown confidently.
    expect(pluginMenuRows({ features: null, workspaceOpen: true })).toEqual([]);
  });
});

describe("pluginMenuAvailable", () => {
  it("hides the button when every plugin is off", () => {
    // An empty menu is a dead end that still costs a click to discover, and the
    // titlebar is where space is most contested.
    expect(pluginMenuAvailable({ features: OFF, workspaceOpen: true })).toBe(false);
  });

  it("hides it before the features have been read", () => {
    expect(pluginMenuAvailable({ features: null, workspaceOpen: true })).toBe(false);
  });

  it("shows it for a row that exists but is currently refused", () => {
    // A disabled row still explains itself, so the menu is worth opening.
    expect(pluginMenuAvailable({ features: ON, workspaceOpen: false })).toBe(true);
  });
});
