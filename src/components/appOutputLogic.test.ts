import { describe, expect, it } from "vitest";
import type { LaunchedApp, ProcessEvent } from "../ipc/types";
import {
  addTab,
  applyEvent,
  canStop,
  closeTab,
  liveTabCount,
  makeTab,
  setTabSeverity,
  statusText,
  tabTitle,
} from "./appOutputLogic";
import type { AppTab } from "./appOutputLogic";

function launched(over: Partial<LaunchedApp> = {}): LaunchedApp {
  return { key: "ext:1", id: "id-1", label: "redis-server", cwd: "/repo", ...over };
}

function tab(over: Partial<AppTab> = {}): AppTab {
  return { ...makeTab(launched()), ...over };
}

describe("makeTab", () => {
  it("starts a launched app as running", () => {
    const created = makeTab(launched());
    expect(created.key).toBe("ext:1");
    expect(created.entryId).toBe("id-1");
    expect(created.label).toBe("redis-server");
    expect(created.cwd).toBe("/repo");
    expect(created.status).toEqual({ kind: "running" });
    expect(created.severity).toBe("all");
  });
});

describe("addTab / closeTab", () => {
  it("appends and focuses the new tab", () => {
    const first = addTab([], makeTab(launched()));
    const second = addTab(first.tabs, makeTab(launched({ key: "ext:2" })));
    expect(second.tabs.map((t) => t.key)).toEqual(["ext:1", "ext:2"]);
    expect(second.activeKey).toBe("ext:2");
  });

  it("keeps two runs of the same command as two tabs", () => {
    // Each launch has its own supervisor key, so the same command run twice is
    // genuinely two processes and must not collapse into one tab.
    const result = addTab([tab()], makeTab(launched({ key: "ext:2" })));
    expect(result.tabs).toHaveLength(2);
  });

  it("focuses the neighbour when the active tab closes", () => {
    const tabs = [tab({ key: "a" }), tab({ key: "b" }), tab({ key: "c" })];
    expect(closeTab(tabs, "b", "b").activeKey).toBe("c");
    expect(closeTab(tabs, "c", "c").activeKey).toBe("b");
  });

  it("leaves the active tab alone when another closes", () => {
    const tabs = [tab({ key: "a" }), tab({ key: "b" })];
    const result = closeTab(tabs, "a", "b");
    expect(result.tabs.map((t) => t.key)).toEqual(["b"]);
    expect(result.activeKey).toBe("b");
  });

  it("has no active tab once the last one closes", () => {
    expect(closeTab([tab({ key: "a" })], "a", "a").activeKey).toBe(null);
  });
});

describe("applyEvent", () => {
  const exited = (over: Partial<Extract<ProcessEvent, { type: "exited" }>> = {}) =>
    ({
      type: "exited",
      code: 0,
      success: true,
      durationMs: 10,
      cancelled: false,
      ...over,
    }) as ProcessEvent;

  it("records the pid from the started event", () => {
    const tabs = applyEvent([tab()], "ext:1", {
      type: "started",
      pid: 4242,
      program: "redis-server",
      args: [],
      cwd: "/repo",
    } as ProcessEvent);
    expect(tabs[0]!.pid).toBe(4242);
    expect(tabs[0]!.status).toEqual({ kind: "running" });
  });

  it("keeps a clean exit visible with its code", () => {
    // The Running panel row disappears the moment a process exits; the tab is
    // the only place the exit and the output survive, so it must not vanish.
    const tabs = applyEvent([tab()], "ext:1", exited());
    expect(tabs[0]!.status).toEqual({
      kind: "exited",
      code: 0,
      success: true,
      cancelled: false,
    });
  });

  it("distinguishes a failure, a cancellation and a signal", () => {
    const failedExit = applyEvent([tab()], "ext:1", exited({ code: 1, success: false }));
    expect(statusText(failedExit[0]!.status)).toBe("exited 1");

    const cancelled = applyEvent([tab()], "ext:1", exited({ code: null, cancelled: true }));
    expect(statusText(cancelled[0]!.status)).toBe("stopped");

    const signalled = applyEvent([tab()], "ext:1", exited({ code: null, success: false }));
    expect(statusText(signalled[0]!.status)).toBe("exited");

    const failed = applyEvent([tab()], "ext:1", {
      type: "failed",
      message: "program not found",
    } as ProcessEvent);
    expect(statusText(failed[0]!.status)).toBe("failed: program not found");
  });

  it("ignores output events and events for an unknown tab", () => {
    const before = [tab()];
    expect(
      applyEvent(before, "ext:1", {
        type: "output",
        stream: "stdout",
        text: "hello",
      } as ProcessEvent),
    ).toEqual(before);
    expect(applyEvent(before, "gone", exited())).toEqual(before);
  });
});

describe("statusText / canStop / liveTabCount", () => {
  it("only offers Stop while a tab is running", () => {
    expect(canStop(tab())).toBe(true);
    expect(
      canStop(tab({ status: { kind: "exited", code: 0, success: true, cancelled: false } })),
    ).toBe(false);
    expect(canStop(tab({ status: { kind: "failed", message: "nope" } }))).toBe(false);
  });

  it("says running with no code", () => {
    expect(statusText({ kind: "running" })).toBe("running");
  });

  it("counts only the live tabs", () => {
    const tabs = [
      tab({ key: "a" }),
      tab({ key: "b", status: { kind: "exited", code: 0, success: true, cancelled: false } }),
    ];
    expect(liveTabCount(tabs)).toBe(1);
  });
});

describe("tabTitle", () => {
  it("is the label when it is unambiguous", () => {
    const tabs = [tab({ key: "a", label: "redis" }), tab({ key: "b", label: "ngrok" })];
    expect(tabTitle(tabs, tabs[0]!)).toBe("redis");
  });

  it("numbers repeats so two runs of one command can be told apart", () => {
    const tabs = [
      tab({ key: "a", label: "redis" }),
      tab({ key: "b", label: "redis" }),
      tab({ key: "c", label: "ngrok" }),
    ];
    expect(tabTitle(tabs, tabs[0]!)).toBe("redis (1)");
    expect(tabTitle(tabs, tabs[1]!)).toBe("redis (2)");
    expect(tabTitle(tabs, tabs[2]!)).toBe("ngrok");
  });
});

describe("setTabSeverity", () => {
  it("narrows one tab and leaves the others alone", () => {
    const tabs = [tab(), tab({ key: "ext:2" })];
    const next = setTabSeverity(tabs, "ext:2", "error");

    expect(next[0]!.severity).toBe("all");
    expect(next[1]!.severity).toBe("error");
  });

  it("returns the same array when the level is already showing", () => {
    // Every console in the panel stays mounted; a no-op must not re-render them.
    const tabs = [tab()];
    expect(setTabSeverity(tabs, "ext:1", "all")).toBe(tabs);
  });

  it("returns the same array for a tab that is not there", () => {
    const tabs = [tab()];
    expect(setTabSeverity(tabs, "ext:missing", "error")).toBe(tabs);
  });
});
