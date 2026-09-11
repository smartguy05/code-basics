import { describe, expect, it } from "vitest";

import type { RedisCandidate, RedisConnectionView, RedisKeyInfo, RedisScanPage } from "../ipc/types";
import {
  candidateBlockedReason,
  connectionLabel,
  discoveredToSaved,
  isConnectable,
  mergeScan,
  ttlLabel,
} from "./redisLogic";

function view(over: Partial<RedisConnectionView>): RedisConnectionView {
  return {
    id: "c1",
    name: "",
    secret: { kind: "appSettings", path: "appsettings.json", key: "ConnectionStrings:Redis" },
    holdsASecret: false,
    workspaceRoot: null,
    allowWrites: false,
    exposeToAgents: false,
    userNamed: false,
    createdAtMs: 1,
    lastUsedMs: null,
    ...over,
  };
}

describe("connectionLabel", () => {
  it("uses the name when present", () => {
    expect(connectionLabel(view({ name: "My cache" }))).toBe("My cache");
  });
  it("falls back to host:port for a literal with no name", () => {
    expect(
      connectionLabel(
        view({
          name: "",
          secret: { kind: "literal", display: { host: "h", port: 6380, db: 0, usesTls: false, hasPassword: false } },
        }),
      ),
    ).toBe("h:6380");
  });
  it("falls back to the id otherwise", () => {
    expect(connectionLabel(view({ name: "", id: "abc" }))).toBe("abc");
  });
});

describe("ttlLabel", () => {
  it("says no expiry for null", () => {
    expect(ttlLabel(null)).toBe("no expiry");
  });
  it("shows sub-second in ms", () => {
    expect(ttlLabel(500)).toBe("500ms");
  });
  it("composes h/m/s", () => {
    expect(ttlLabel(90_000)).toBe("1m 30s");
    expect(ttlLabel(3_661_000)).toBe("1h 1m 1s");
    expect(ttlLabel(5000)).toBe("5s");
  });
});

describe("mergeScan", () => {
  const key = (k: string): RedisKeyInfo => ({ key: k, type: "string", ttlMs: null });
  const page = (keys: string[], cursor: string): RedisScanPage => ({
    cursor,
    keys: keys.map(key),
    complete: cursor === "0",
  });

  it("appends new keys and drops duplicates", () => {
    const first = mergeScan([], page(["a", "b"], "10"));
    expect(first.map((k) => k.key)).toEqual(["a", "b"]);
    const second = mergeScan(first, page(["b", "c"], "0"));
    expect(second.map((k) => k.key)).toEqual(["a", "b", "c"]);
  });
});

describe("candidates", () => {
  const candidate = (over: Partial<RedisCandidate>): RedisCandidate => ({
    id: "appsettings:x:Redis",
    name: "Redis",
    origin: "appsettings.json",
    project: "Api",
    source: { kind: "appSettings", path: "appsettings.json", key: "ConnectionStrings:Redis" },
    display: { host: "h", port: 6379, db: 0, usesTls: false, hasPassword: false },
    state: { kind: "ready" },
    ...over,
  });

  it("is connectable when ready", () => {
    expect(isConnectable(candidate({}))).toBe(true);
    expect(candidateBlockedReason(candidate({}))).toBeNull();
  });
  it("reports the reason when unresolved", () => {
    const c = candidate({ state: { kind: "unresolved", reason: "placeholder" } });
    expect(isConnectable(c)).toBe(false);
    expect(candidateBlockedReason(c)).toBe("placeholder");
  });
  it("builds a save payload without consent flags", () => {
    const payload = discoveredToSaved(candidate({}), "/repo", 42);
    expect(payload).not.toHaveProperty("allowWrites");
    expect(payload).not.toHaveProperty("exposeToAgents");
    expect(payload.workspaceRoot).toBe("/repo");
    expect(payload.createdAtMs).toBe(42);
    expect(payload.userNamed).toBe(false);
  });
});
