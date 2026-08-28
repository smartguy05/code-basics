import { describe, expect, it } from "vitest";
import type { Launchable, LauncherGroups } from "../ipc/types";
import {
  canRun,
  displayLabel,
  filterGroups,
  moveSelection,
  needsShell,
  pickerKeyAction,
  pickerRows,
  shortCwd,
} from "./launcherLogic";

function entry(over: Partial<Launchable> = {}): Launchable {
  return {
    id: "id-1",
    command: "docker compose up",
    cwd: "/repo",
    env: {},
    label: null,
    shell: false,
    pinned: false,
    lastRunMs: 1000,
    runCount: 1,
    ...over,
  };
}

function groups(over: Partial<LauncherGroups> = {}): LauncherGroups {
  return { thisCodebase: [], global: [], ...over };
}

describe("needsShell", () => {
  it("spots the metacharacters a bare argv spawn would mangle", () => {
    // The default for the checkbox: without a shell, `|` reaches the program as
    // an ordinary argument, so the command silently does something else.
    for (const command of [
      "echo hi | findstr hi",
      "app > out.log",
      "app < in.txt",
      "build && test",
      "build || echo failed",
      "a ; b",
      "app &",
    ]) {
      expect(needsShell(command), command).toBe(true);
    }
  });

  it("leaves an ordinary command line alone", () => {
    expect(needsShell("docker compose up -d")).toBe(false);
    expect(needsShell("")).toBe(false);
    expect(needsShell(String.raw`"C:\Program Files\redis\redis-server.exe" --port 6380`)).toBe(
      false,
    );
  });

  it("ignores a metacharacter inside quotes", () => {
    // Quoted, it is just text — the same rule the Rust tokeniser applies, so the
    // checkbox default and the backend's refusal cannot disagree.
    expect(needsShell('grep "a|b" file.txt')).toBe(false);
    expect(needsShell('grep "a|b" file.txt > out')).toBe(true);
  });

  it("treats an escaped quote as content, not as a quote", () => {
    expect(needsShell(String.raw`node -e "console.log(\"hi\")"`)).toBe(false);
    expect(needsShell(String.raw`node -e \"a\" | b`)).toBe(true);
  });
});

describe("displayLabel", () => {
  it("prefers the rename and falls back to the command", () => {
    expect(displayLabel(entry({ label: "Redis" }))).toBe("Redis");
    expect(displayLabel(entry({ label: null }))).toBe("docker compose up");
  });

  it("never renders as empty", () => {
    expect(displayLabel(entry({ label: "   ", command: "  redis  " }))).toBe("redis");
  });
});

describe("canRun", () => {
  it("requires a non-blank command line", () => {
    expect(canRun("node -e 1")).toBe(true);
    expect(canRun("   ")).toBe(false);
    expect(canRun("")).toBe(false);
  });
});

describe("shortCwd", () => {
  it("says nothing when the command runs at the codebase root", () => {
    expect(shortCwd("/repo", "/repo")).toBe("");
  });

  it("shows the path relative to the root when inside it", () => {
    expect(shortCwd("/repo/src/api", "/repo")).toBe("src/api");
    expect(shortCwd(String.raw`C:\repo\src`, "C:/repo")).toBe("src");
  });

  it("shows the whole path when outside the root, or with no root", () => {
    expect(shortCwd("/elsewhere/tools", "/repo")).toBe("/elsewhere/tools");
    expect(shortCwd("/elsewhere", null)).toBe("/elsewhere");
    // A sibling sharing a prefix is outside, not inside.
    expect(shortCwd("/repo2/x", "/repo")).toBe("/repo2/x");
  });
});

describe("filterGroups", () => {
  const populated = groups({
    thisCodebase: [entry({ id: "a", command: "npm run dev" })],
    global: [
      entry({ id: "b", command: "redis-server", label: "Local Redis" }),
      entry({ id: "c", command: "ngrok http 5000" }),
    ],
  });

  it("returns everything for a blank query", () => {
    const result = filterGroups(populated, "  ");
    expect(result.thisCodebase).toHaveLength(1);
    expect(result.global).toHaveLength(2);
  });

  it("matches the command and the rename, case-insensitively", () => {
    expect(filterGroups(populated, "REDIS").global.map((e) => e.id)).toEqual(["b"]);
    expect(filterGroups(populated, "local").global.map((e) => e.id)).toEqual(["b"]);
    expect(filterGroups(populated, "npm").thisCodebase.map((e) => e.id)).toEqual(["a"]);
  });

  it("drops a group entirely when nothing in it matches", () => {
    const result = filterGroups(populated, "ngrok");
    expect(result.thisCodebase).toEqual([]);
    expect(result.global.map((e) => e.id)).toEqual(["c"]);
  });
});

describe("pickerRows / moveSelection", () => {
  const populated = groups({
    thisCodebase: [entry({ id: "a" })],
    global: [entry({ id: "b" }), entry({ id: "c" })],
  });

  it("flattens both groups in display order", () => {
    expect(pickerRows(populated).map((r) => r.entry.id)).toEqual(["a", "b", "c"]);
  });

  it("moves within bounds and stops at the ends", () => {
    const rows = pickerRows(populated);
    expect(moveSelection(rows, 0, 1)).toBe(1);
    expect(moveSelection(rows, 2, 1)).toBe(2);
    expect(moveSelection(rows, 0, -1)).toBe(0);
    expect(moveSelection(rows, 1, -1)).toBe(0);
  });

  it("clamps a selection the filter has invalidated", () => {
    // The list shrinks as the user types; a stale index must not address a row
    // that is no longer there.
    expect(moveSelection(pickerRows(groups({ global: [entry()] })), 7, 1)).toBe(0);
    expect(moveSelection([], 3, 1)).toBe(-1);
  });
});

describe("pickerKeyAction", () => {
  it("maps the keys the picker handles and ignores the rest", () => {
    expect(pickerKeyAction("Enter")).toBe("run");
    expect(pickerKeyAction("Escape")).toBe("close");
    expect(pickerKeyAction("ArrowDown")).toBe("next");
    expect(pickerKeyAction("ArrowUp")).toBe("prev");
    expect(pickerKeyAction("a")).toBe(null);
    expect(pickerKeyAction("Tab")).toBe(null);
  });
});
