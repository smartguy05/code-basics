import { describe, expect, it } from "vitest";
import type { Launchable, RunningRecord, RunningReport, ShellInfo } from "../ipc/types";
import {
  liveKeysByEntry,
  newTerminalButton,
  offersStop,
  SHELLS_SECTION,
  SHORTCUTS_SECTION,
  shortcutActionLabel,
  shortcutEntries,
  terminalMenuRows,
  type TerminalMenuRow,
  type TerminalMenuState,
} from "./terminalMenuLogic";

function entry(over: Partial<Launchable> = {}): Launchable {
  return {
    id: "e1",
    command: "redis-server",
    cwd: "/repo",
    env: {},
    label: null,
    shell: false,
    pinned: false,
    shortcut: false,
    persistent: false,
    headless: false,
    lastRunMs: 0,
    runCount: 1,
    ...over,
  };
}

function record(over: Partial<RunningRecord> = {}): RunningRecord {
  return {
    pid: 100,
    kind: "external",
    label: "redis-server",
    root: "/repo",
    key: "ext:1",
    program: "redis-server",
    startedAtMs: 0,
    ...over,
  };
}

function report(over: Partial<RunningReport> = {}): RunningReport {
  return { live: [], orphans: [], warnings: [], ...over };
}

function shell(over: Partial<ShellInfo> = {}): ShellInfo {
  return { id: "pwsh", label: "PowerShell (pwsh)", program: "C:\\pwsh.exe", args: [], ...over };
}

function state(over: Partial<TerminalMenuState> = {}): TerminalMenuState {
  return {
    workspaceOpen: true,
    runningCount: 0,
    appTabCount: 0,
    liveAppCount: 0,
    shortcuts: [],
    liveKeys: new Map(),
    shortcutsLoading: false,
    shells: [],
    shellsLoading: false,
    ...over,
  };
}

const row = (rows: TerminalMenuRow[], id: string) => rows.find((r) => r.id === id);

describe("shortcutEntries", () => {
  it("takes only entries marked as a shortcut, this codebase first", () => {
    const groups = {
      thisCodebase: [entry({ id: "a", shortcut: true }), entry({ id: "b" })],
      global: [entry({ id: "c" }), entry({ id: "d", shortcut: true })],
    };
    expect(shortcutEntries(groups).map((e) => e.id)).toEqual(["a", "d"]);
  });

  it("never reads a pin as a shortcut — they are different facts", () => {
    const groups = {
      thisCodebase: [entry({ id: "pinned-only", pinned: true })],
      global: [],
    };
    expect(shortcutEntries(groups)).toEqual([]);
  });
});

describe("liveKeysByEntry", () => {
  it("has nothing to say before a report has been read", () => {
    expect(liveKeysByEntry(null, new Map([["ext:1", "e1"]])).size).toBe(0);
  });

  it("joins live external processes back to the entry they were launched from", () => {
    const live = report({ live: [record({ key: "ext:1" }), record({ key: "ext:2", pid: 101 })] });
    const map = liveKeysByEntry(live, new Map([["ext:1", "e1"], ["ext:2", "e1"]]));
    expect(map.get("e1")).toEqual(["ext:1", "ext:2"]);
  });

  it("ignores processes of every other kind", () => {
    const live = report({ live: [record({ key: "ext:1", kind: "terminal" })] });
    expect(liveKeysByEntry(live, new Map([["ext:1", "e1"]])).size).toBe(0);
  });

  it("ignores a launch whose process is no longer live", () => {
    const map = liveKeysByEntry(report(), new Map([["ext:1", "e1"]]));
    expect(map.get("e1")).toBe(undefined);
  });

  it("ignores a live process the app did not launch from the launcher", () => {
    const live = report({ live: [record({ key: "ext:stranger" })] });
    expect(liveKeysByEntry(live, new Map()).size).toBe(0);
  });
});

describe("newTerminalButton", () => {
  it("is enabled with a codebase open", () => {
    expect(newTerminalButton({ workspaceOpen: true }).disabled).toBe(false);
  });

  it("says why it is disabled with none open", () => {
    const button = newTerminalButton({ workspaceOpen: false });
    expect(button.disabled).toBe(true);
    expect(button.title).toMatch(/Open a codebase/);
  });
});

describe("terminalMenuRows", () => {
  it("lists the four folded-in actions in order", () => {
    const rows = terminalMenuRows(state());
    expect(rows.slice(0, 4).map((r) => r.id)).toEqual([
      "terminal.new",
      "panel.launch",
      "panel.running",
      "panel.apps",
    ]);
  });

  it("disables New Terminal with no codebase open, and says why", () => {
    const rows = terminalMenuRows(state({ workspaceOpen: false }));
    expect(row(rows, "terminal.new")?.disabled).toBe(true);
    expect(row(rows, "terminal.new")?.action).toBe(null);
  });

  it("leaves Launch and Running usable with no codebase open", () => {
    const rows = terminalMenuRows(state({ workspaceOpen: false }));
    expect(row(rows, "panel.launch")?.disabled).toBe(false);
    expect(row(rows, "panel.running")?.disabled).toBe(false);
  });

  it("no longer offers the SQL console — that moved to the Plugins menu", () => {
    // This menu is about *running things*; a database console is not one, and
    // the next plugin would have been a second guest here. Its rules now live
    // in `pluginMenuLogic`, tested there.
    expect(row(terminalMenuRows(state()), "view.sql")).toBe(undefined);
  });

  it("badges the running count, and shows no badge for none", () => {
    expect(row(terminalMenuRows(state({ runningCount: 3 })), "panel.running")?.badge).toBe(3);
    expect(row(terminalMenuRows(state()), "panel.running")?.badge).toBe(null);
  });

  it("disables App output until something has been launched", () => {
    const empty = row(terminalMenuRows(state()), "panel.apps");
    expect(empty?.disabled).toBe(true);
    expect(empty?.title).toMatch(/Nothing launched/);
    const some = row(terminalMenuRows(state({ appTabCount: 2, liveAppCount: 1 })), "panel.apps");
    expect(some?.disabled).toBe(false);
    expect(some?.badge).toBe(1);
  });

  it("badges only the apps still running, not every tab", () => {
    const rows = terminalMenuRows(state({ appTabCount: 3, liveAppCount: 0 }));
    expect(row(rows, "panel.apps")?.badge).toBe(null);
    expect(row(rows, "panel.apps")?.disabled).toBe(false);
  });

  it("draws no shortcut separator and no shortcut section when there are none", () => {
    const rows = terminalMenuRows(state());
    expect(rows.some((r) => r.section === SHORTCUTS_SECTION)).toBe(false);
    expect(rows.some((r) => r.id.startsWith("shortcut:"))).toBe(false);
  });

  it("says it is still reading rather than claiming there are none", () => {
    const rows = terminalMenuRows(state({ shortcutsLoading: true }));
    const loading = row(rows, "shortcuts.loading");
    expect(loading?.disabled).toBe(true);
    expect(loading?.action).toBe(null);
    expect(loading?.section).toBe(SHORTCUTS_SECTION);
  });

  it("opens the shortcut section once, on the first shortcut row", () => {
    const rows = terminalMenuRows(
      state({ shortcuts: [entry({ id: "a", shortcut: true }), entry({ id: "b", shortcut: true })] }),
    );
    expect(row(rows, "shortcut:a")?.separator).toBe(true);
    expect(row(rows, "shortcut:a")?.section).toBe(SHORTCUTS_SECTION);
    expect(row(rows, "shortcut:b")?.separator).toBe(false);
    expect(row(rows, "shortcut:b")?.section).toBe(undefined);
  });

  it("labels a shortcut by its rename, falling back to the command", () => {
    const rows = terminalMenuRows(
      state({
        shortcuts: [
          entry({ id: "a", shortcut: true, label: "Redis" }),
          entry({ id: "b", shortcut: true, command: "docker compose up" }),
        ],
      }),
    );
    expect(row(rows, "shortcut:a")?.label).toBe("Redis");
    expect(row(rows, "shortcut:b")?.label).toBe("docker compose up");
  });

  it("runs a shortcut that is not running", () => {
    const rows = terminalMenuRows(state({ shortcuts: [entry({ id: "a", shortcut: true })] }));
    expect(row(rows, "shortcut:a")?.live).toBe(false);
    expect(row(rows, "shortcut:a")?.action).toEqual({
      kind: "runShortcut",
      entry: entry({ id: "a", shortcut: true }),
    });
  });

  it("offers Stop for a persistent shortcut that is up, with every live key", () => {
    const persistent = entry({ id: "a", shortcut: true, persistent: true });
    const rows = terminalMenuRows(
      state({
        shortcuts: [persistent],
        liveKeys: new Map([["a", ["ext:1", "ext:2"]]]),
      }),
    );
    const shortcut = row(rows, "shortcut:a");
    expect(shortcut?.live).toBe(true);
    expect(shortcut?.action).toEqual({
      kind: "stopShortcut",
      entry: persistent,
      keys: ["ext:1", "ext:2"],
    });
    expect(shortcutActionLabel(shortcut as TerminalMenuRow)).toBe("Stop");
  });

  it("still offers Run for an ordinary shortcut that is up, but shows it as live", () => {
    const ordinary = entry({ id: "a", shortcut: true });
    const rows = terminalMenuRows(
      state({ shortcuts: [ordinary], liveKeys: new Map([["a", ["ext:1"]]]) }),
    );
    const shortcut = row(rows, "shortcut:a");
    expect(shortcut?.live).toBe(true);
    expect(shortcut?.action?.kind).toBe("runShortcut");
    expect(shortcutActionLabel(shortcut as TerminalMenuRow)).toBe("Run");
  });
});

describe("terminalMenuRows — the shells section", () => {
  it("puts the shell rows after the four base rows and before the commands", () => {
    const rows = terminalMenuRows(
      state({
        shells: [shell(), shell({ id: "cmd", label: "Command Prompt" })],
        shortcuts: [entry({ id: "a", shortcut: true })],
      }),
    );
    expect(rows.map((r) => r.id)).toEqual([
      "terminal.new",
      "panel.launch",
      "panel.running",
      "panel.apps",
      "shell:pwsh",
      "shell:cmd",
      "shortcut:a",
    ]);
  });

  it("opens its own section once, on the first shell row", () => {
    const rows = terminalMenuRows(
      state({ shells: [shell(), shell({ id: "cmd", label: "Command Prompt" })] }),
    );
    expect(row(rows, "shell:pwsh")?.separator).toBe(true);
    expect(row(rows, "shell:pwsh")?.section).toBe(SHELLS_SECTION);
    expect(row(rows, "shell:cmd")?.separator).toBe(false);
    expect(row(rows, "shell:cmd")?.section).toBe(undefined);
  });

  it("shows the label, the resolved path as the tooltip, no badge and no live dot", () => {
    const rows = terminalMenuRows(state({ shells: [shell()] }));
    const pwsh = row(rows, "shell:pwsh");
    expect(pwsh?.label).toBe("PowerShell (pwsh)");
    expect(pwsh?.title).toBe("C:\\pwsh.exe");
    expect(pwsh?.badge).toBe(null);
    expect(pwsh?.live).toBe(undefined);
  });

  it("carries the whole ShellInfo in its action, so nothing is re-resolved later", () => {
    const cmd = shell({ id: "cmd", label: "Command Prompt", program: "C:\\cmd.exe", args: ["/K"] });
    const rows = terminalMenuRows(state({ shells: [cmd] }));
    expect(row(rows, "shell:cmd")?.action).toEqual({ kind: "newTerminalIn", shell: cmd });
  });

  it("marks no shell as the preferred one — these rows are one-off launches", () => {
    const rows = terminalMenuRows(state({ shells: [shell(), shell({ id: "cmd" })] }));
    const shellRows = rows.filter((r) => r.id.startsWith("shell:"));
    expect(shellRows.every((r) => !r.label.includes("✓"))).toBe(true);
    expect(shellRows.every((r) => r.badge === null)).toBe(true);
  });

  it("disables every shell row with the same reason New Terminal gives", () => {
    // A shell row must never claim it can open a terminal when New Terminal says
    // it cannot, so both read one rule.
    const rows = terminalMenuRows(state({ workspaceOpen: false, shells: [shell()] }));
    const expected = newTerminalButton({ workspaceOpen: false });
    const pwsh = row(rows, "shell:pwsh");
    expect(pwsh?.disabled).toBe(true);
    expect(pwsh?.title).toBe(expected.title);
    expect(pwsh?.action).toBe(null);
  });

  it("says it is still detecting rather than claiming none were found", () => {
    const rows = terminalMenuRows(state({ shellsLoading: true }));
    const loading = row(rows, "shells.loading");
    expect(loading?.disabled).toBe(true);
    expect(loading?.action).toBe(null);
    expect(loading?.section).toBe(SHELLS_SECTION);
    expect(rows.some((r) => r.id.startsWith("shell:"))).toBe(false);
  });

  it("ignores a stale list while a fresh detection is in flight", () => {
    const rows = terminalMenuRows(state({ shellsLoading: true, shells: [shell()] }));
    expect(row(rows, "shell:pwsh")).toBe(undefined);
    expect(row(rows, "shells.loading")).not.toBe(undefined);
  });

  it("reports an empty detection as a disabled row with a reason, never an omitted section", () => {
    // "We found no shell on this machine" is a machine fact the user may need to
    // act on; a silently missing section is indistinguishable from a bug.
    const rows = terminalMenuRows(state({ shells: [] }));
    const none = row(rows, "shells.none");
    expect(none?.disabled).toBe(true);
    expect(none?.action).toBe(null);
    expect(none?.section).toBe(SHELLS_SECTION);
    expect(none?.title.toLowerCase()).toContain("system default");
  });
});

describe("offersStop", () => {
  it("needs both a persistent entry and a live process", () => {
    expect(offersStop(entry({ persistent: true }), true)).toBe(true);
    expect(offersStop(entry({ persistent: true }), false)).toBe(false);
    expect(offersStop(entry(), true)).toBe(false);
  });
});
