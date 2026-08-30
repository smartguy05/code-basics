import { describe, expect, it } from "vitest";
import type { RunningRecord, RunningReport } from "../ipc/types";
import {
  formatAge,
  hasOutput,
  isEmpty,
  killRequest,
  kindIcon,
  kindLabel,
  liveCount,
  rootBasename,
  sameRoot,
  stopMenuCount,
  stopMenuGroups,
  stopRowLabel,
} from "./runningLogic";

function rec(over: Partial<RunningRecord> = {}): RunningRecord {
  return {
    pid: 100,
    kind: "run",
    label: "MyApi",
    root: "/home/me/proj",
    key: "cfg-1",
    program: "dotnet.exe",
    startedAtMs: 1000,
    ...over,
  };
}

/**
 * A report with everything empty unless overridden.
 *
 * Module-scoped rather than inside one `describe`, because the Stop-menu tests
 * below need it too — the earlier version was local to `liveCount / isEmpty`.
 */
function report(over: Partial<RunningReport> = {}): RunningReport {
  return { live: [], orphans: [], warnings: [], ...over };
}

describe("kindIcon / kindLabel", () => {
  it("gives every kind a distinct icon and a human label", () => {
    const kinds = ["run", "build", "terminal", "review", "behavioral", "external"] as const;
    const icons = kinds.map(kindIcon);
    expect(new Set(icons).size).toBe(kinds.length);
    expect(kindLabel("behavioral")).toBe("Behavioral");
    expect(kindLabel("terminal")).toBe("Terminal");
    expect(kindLabel("external")).toBe("App");
  });
});

describe("hasOutput", () => {
  it("offers View output only for a launched app", () => {
    // A configuration run's output is in its Run-tab console and a terminal is
    // its own window; only a launched app has a tab in the output panel that the
    // panel can focus, and claiming otherwise would give a button that does
    // nothing.
    expect(hasOutput(rec({ kind: "external" }))).toBe(true);
    for (const kind of ["run", "build", "terminal", "review", "behavioral"] as const) {
      expect(hasOutput(rec({ kind })), kind).toBe(false);
    }
  });
});

describe("rootBasename", () => {
  it("takes the last segment of a posix path", () => {
    expect(rootBasename("/home/me/proj")).toBe("proj");
  });
  it("takes the last segment of a windows path", () => {
    expect(rootBasename("C:\\Users\\me\\Code\\app")).toBe("app");
  });
  it("ignores a trailing separator", () => {
    expect(rootBasename("/home/me/proj/")).toBe("proj");
    expect(rootBasename("C:\\app\\")).toBe("app");
  });
  it("falls back to the whole string when there is no segment", () => {
    expect(rootBasename("")).toBe("");
  });
});

describe("formatAge", () => {
  it("shows seconds under a minute", () => {
    expect(formatAge(0, 5_000)).toBe("5s");
    expect(formatAge(0, 59_000)).toBe("59s");
  });
  it("shows whole minutes under an hour", () => {
    expect(formatAge(0, 60_000)).toBe("1m");
    expect(formatAge(0, 59 * 60_000)).toBe("59m");
  });
  it("shows hours and minutes past an hour", () => {
    expect(formatAge(0, 60 * 60_000)).toBe("1h");
    expect(formatAge(0, 125 * 60_000)).toBe("2h 5m");
  });
  it("never goes negative on clock skew", () => {
    expect(formatAge(10_000, 0)).toBe("0s");
  });
});

describe("liveCount / isEmpty", () => {

  it("counts only live processes for the badge", () => {
    expect(liveCount(null)).toBe(0);
    expect(liveCount(report({ live: [rec(), rec()], orphans: [rec()] }))).toBe(2);
  });

  it("is empty with no live and no orphans", () => {
    expect(isEmpty(null)).toBe(true);
    expect(isEmpty(report())).toBe(true);
    expect(isEmpty(report({ orphans: [rec()] }))).toBe(false);
    expect(isEmpty(report({ live: [rec()] }))).toBe(false);
  });
});

describe("killRequest", () => {
  it("carries the routing fields and the orphan flag", () => {
    expect(killRequest(rec({ pid: 42, kind: "terminal", key: "sess-9" }), true)).toEqual({
      pid: 42,
      kind: "terminal",
      root: "/home/me/proj",
      key: "sess-9",
      orphan: true,
    });
    expect(killRequest(rec(), false).orphan).toBe(false);
  });
});

describe("sameRoot", () => {
  it("ignores a trailing separator and which separator it is", () => {
    expect(sameRoot("C:/repo/app", "C:\\repo\\app")).toBe(true);
    expect(sameRoot("C:/repo/app/", "C:/repo/app")).toBe(true);
  });

  it("ignores case, which the two sources routinely disagree on", () => {
    expect(sameRoot("C:/Repo/App", "c:/repo/app")).toBe(true);
  });

  it("still distinguishes different codebases", () => {
    expect(sameRoot("C:/repo/app", "C:/repo/apple")).toBe(false);
  });
});

describe("stopMenuGroups", () => {
  it("has nothing to show before the first poll comes back", () => {
    expect(stopMenuGroups(null, "/home/me/proj")).toEqual([]);
    expect(stopMenuCount(stopMenuGroups(null, "/home/me/proj"))).toBe(0);
  });

  it("groups by kind, runs and launched apps first", () => {
    const groups = stopMenuGroups(
      report({
        live: [
          rec({ kind: "terminal", key: "t1" }),
          rec({ kind: "build", key: "b1" }),
          rec({ kind: "external", key: "e1" }),
          rec({ kind: "run", key: "r1" }),
        ],
      }),
      "/home/me/proj",
    );

    expect(groups.map((g) => g.key)).toEqual(["run", "external", "build", "terminal"]);
  });

  it("omits a kind that has nothing running", () => {
    const groups = stopMenuGroups(report({ live: [rec({ kind: "run" })] }), "/home/me/proj");
    expect(groups.map((g) => g.key)).toEqual(["run"]);
  });

  it("puts this codebase's processes first inside a group", () => {
    const groups = stopMenuGroups(
      report({
        live: [
          rec({ label: "AAA elsewhere", root: "/home/me/other", key: "a" }),
          rec({ label: "ZZZ here", root: "/home/me/proj", key: "z" }),
        ],
      }),
      "/home/me/proj",
    );

    expect(groups[0]?.rows.map((r) => r.record.label)).toEqual(["ZZZ here", "AAA elsewhere"]);
    expect(groups[0]?.rows.map((r) => r.here)).toEqual([true, false]);
  });

  it("orders by label within a codebase, and by pid when labels tie", () => {
    const groups = stopMenuGroups(
      report({
        live: [
          rec({ label: "b", key: "b" }),
          rec({ label: "a", pid: 9, key: "a9" }),
          rec({ label: "a", pid: 2, key: "a2" }),
        ],
      }),
      "/home/me/proj",
    );

    expect(groups[0]?.rows.map((r) => `${r.record.label}${r.record.pid}`)).toEqual([
      "a2",
      "a9",
      "b100",
    ]);
  });

  it("keeps orphans in a group of their own, last, flagged for the extra confirm", () => {
    const groups = stopMenuGroups(
      report({ live: [rec()], orphans: [rec({ pid: 7, key: "o" })] }),
      "/home/me/proj",
    );

    expect(groups.map((g) => g.key)).toEqual(["run", "orphans"]);
    expect(groups[1]?.rows.every((r) => r.orphan)).toBe(true);
    expect(groups[0]?.rows.every((r) => !r.orphan)).toBe(true);
  });

  it("counts every row across every group", () => {
    const groups = stopMenuGroups(
      report({ live: [rec({ key: "a" }), rec({ kind: "build", key: "b" })], orphans: [rec({ key: "c" })] }),
      "/home/me/proj",
    );
    expect(stopMenuCount(groups)).toBe(3);
  });
});

describe("stopRowLabel", () => {
  it("names only the process for something started here", () => {
    const groups = stopMenuGroups(report({ live: [rec({ label: "MyApi" })] }), "/home/me/proj");
    expect(stopRowLabel(groups[0]!.rows[0]!)).toBe("MyApi");
  });

  it("names the codebase for something started elsewhere", () => {
    const groups = stopMenuGroups(
      report({ live: [rec({ label: "MyApi", root: "/home/me/other" })] }),
      "/home/me/proj",
    );
    expect(stopRowLabel(groups[0]!.rows[0]!)).toBe("MyApi — other");
  });
});
