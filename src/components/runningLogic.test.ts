import { describe, expect, it } from "vitest";
import type { RunningRecord, RunningReport } from "../ipc/types";
import {
  formatAge,
  isEmpty,
  killRequest,
  kindIcon,
  kindLabel,
  liveCount,
  rootBasename,
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

describe("kindIcon / kindLabel", () => {
  it("gives every kind a distinct icon and a human label", () => {
    const kinds = ["run", "build", "terminal", "review", "behavioral"] as const;
    const icons = kinds.map(kindIcon);
    expect(new Set(icons).size).toBe(kinds.length);
    expect(kindLabel("behavioral")).toBe("Behavioral");
    expect(kindLabel("terminal")).toBe("Terminal");
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
  const report = (over: Partial<RunningReport> = {}): RunningReport => ({
    live: [],
    orphans: [],
    warnings: [],
    ...over,
  });

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
