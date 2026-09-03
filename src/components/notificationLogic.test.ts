import { describe, expect, it } from "vitest";
import {
  describeUnexpectedStop,
  dismissNotification,
  MAX_NOTIFICATIONS,
  notifiesUnexpectedStop,
  notificationTtlMs,
  pushNotification,
  type AppNotification,
} from "./notificationLogic";
import type { ProcessEvent } from "../ipc/types";

const exited = (over: Partial<Extract<ProcessEvent, { type: "exited" }>> = {}): ProcessEvent => ({
  type: "exited",
  code: 1,
  success: false,
  durationMs: 10,
  cancelled: false,
  ...over,
});
const failed = (message = "boom"): ProcessEvent => ({ type: "failed", message });

const note = (over: Partial<AppNotification> = {}): AppNotification => ({
  id: "n1",
  kind: "error",
  title: "t",
  ...over,
});

describe("notifiesUnexpectedStop", () => {
  it("says nothing about a command that was never declared a service", () => {
    // A one-shot command finishing is not news, and notifying about it would
    // make the whole surface worthless.
    expect(notifiesUnexpectedStop(exited(), false)).toBe(false);
    expect(notifiesUnexpectedStop(failed(), false)).toBe(false);
  });

  it("reports a persistent service that died", () => {
    expect(notifiesUnexpectedStop(exited(), true)).toBe(true);
    expect(notifiesUnexpectedStop(failed(), true)).toBe(true);
  });

  it("reports a persistent service that exited cleanly", () => {
    // The case the whole flag exists for: exit 0 is correct behaviour by the
    // process and unexpected behaviour by the user's account of it.
    expect(notifiesUnexpectedStop(exited({ code: 0, success: true }), true)).toBe(true);
  });

  it("stays quiet when the user pressed Stop", () => {
    // Telling someone the thing they just stopped has stopped is the noise that
    // teaches people to ignore notifications.
    expect(notifiesUnexpectedStop(exited({ cancelled: true }), true)).toBe(false);
    expect(notifiesUnexpectedStop(exited({ cancelled: true, code: 0, success: true }), true)).toBe(
      false,
    );
  });

  it("ignores everything that is not an ending", () => {
    expect(notifiesUnexpectedStop({ type: "output", stream: "stdout", text: "x" }, true)).toBe(false);
    expect(
      notifiesUnexpectedStop(
        { type: "started", pid: 1, program: "p", args: [], cwd: "/" },
        true,
      ),
    ).toBe(false);
  });
});

describe("describeUnexpectedStop", () => {
  it("names the service and the exit code", () => {
    const d = describeUnexpectedStop("Redis", exited({ code: 137 }));
    expect(d.kind).toBe("error");
    expect(d.title).toContain("Redis");
    expect(d.detail).toContain("137");
  });

  it("omits the code rather than printing 'null'", () => {
    // A signalled process reports no code; "exit code null" is worse than not
    // mentioning it.
    const d = describeUnexpectedStop("Redis", exited({ code: null }));
    expect(d.detail).not.toContain("null");
    expect(d.detail).toContain("unexpectedly");
  });

  it("says out loud that a clean exit was still not expected", () => {
    const d = describeUnexpectedStop("Worker", exited({ code: 0, success: true }));
    // Warning, not error — nothing went wrong — but it must not read as good
    // news, which is exactly how a bare "exited cleanly" would read.
    expect(d.kind).toBe("warning");
    expect(d.detail).toContain("expected to keep running");
  });

  it("distinguishes never-started from stopped", () => {
    const d = describeUnexpectedStop("Redis", failed("program not found: redis-server"));
    expect(d.kind).toBe("error");
    expect(d.title).toContain("could not start");
    expect(d.detail).toContain("redis-server");
  });
});

describe("notificationTtlMs", () => {
  it("never auto-dismisses an error", () => {
    // The service exists because something died while the user was looking
    // elsewhere; a message that removes itself loses that case entirely.
    expect(notificationTtlMs("error")).toBeNull();
  });

  it("does auto-dismiss the quieter kinds", () => {
    expect(notificationTtlMs("warning")).toBeGreaterThan(0);
    expect(notificationTtlMs("info")).toBeGreaterThan(0);
    expect(notificationTtlMs("info")).toBeLessThan(notificationTtlMs("warning")!);
  });
});

describe("pushNotification", () => {
  it("appends", () => {
    expect(pushNotification([note({ id: "a" })], note({ id: "b" })).map((n) => n.id)).toEqual([
      "a",
      "b",
    ]);
  });

  it("replaces an earlier notification about the same thing", () => {
    // A crash-looping service must be one updating entry, not a new toast every
    // restart.
    const list = [note({ id: "a", dedupeKey: "svc" }), note({ id: "b" })];
    const next = pushNotification(list, note({ id: "c", dedupeKey: "svc" }));
    expect(next.map((n) => n.id)).toEqual(["b", "c"]);
  });

  it("moves a replacement to the end rather than leaving it in place", () => {
    // It is the most recent thing that happened; leaving it where it was lets a
    // service that failed an hour ago and again just now sit at the bottom.
    const list = [note({ id: "a", dedupeKey: "svc" }), note({ id: "b" }), note({ id: "c" })];
    expect(pushNotification(list, note({ id: "d", dedupeKey: "svc" })).map((n) => n.id)).toEqual([
      "b",
      "c",
      "d",
    ]);
  });

  it("keeps entries without a dedupe key apart", () => {
    const list = [note({ id: "a" }), note({ id: "b" })];
    expect(pushNotification(list, note({ id: "c" }))).toHaveLength(3);
  });

  it("drops the oldest when full, never the newest", () => {
    let list: AppNotification[] = [];
    for (let i = 0; i < MAX_NOTIFICATIONS + 2; i += 1) {
      list = pushNotification(list, note({ id: `n${i}` }));
    }
    expect(list).toHaveLength(MAX_NOTIFICATIONS);
    expect(list[list.length - 1]?.id).toBe(`n${MAX_NOTIFICATIONS + 1}`);
    expect(list.some((n) => n.id === "n0")).toBe(false);
  });
});

describe("dismissNotification", () => {
  it("removes by id", () => {
    expect(dismissNotification([note({ id: "a" }), note({ id: "b" })], "a").map((n) => n.id)).toEqual(
      ["b"],
    );
  });

  it("returns the same array when nothing matched, so a stale dismiss costs no render", () => {
    const list = [note({ id: "a" })];
    expect(dismissNotification(list, "gone")).toBe(list);
  });
});
