import { describe, expect, it } from "vitest";

import type { DebugEvent, RunConfig } from "../ipc/types";
import { debugAvailability, debugEffects } from "./debugLogic";

function config(over: Partial<RunConfig> & Pick<RunConfig, "id">): RunConfig {
  return {
    name: over.id,
    kind: "app",
    ecosystem: "dotnet",
    compound: [],
    args: [],
    env: {},
    warnings: [],
    source: "userFile",
    ignoreLaunchSettings: false,
    ...over,
  } as RunConfig;
}

describe("debugAvailability", () => {
  it("offers Debug for the two ecosystems that have an adapter", () => {
    for (const ecosystem of ["dotnet", "node"]) {
      expect(debugAvailability(config({ id: "a", ecosystem }), [])).toEqual({ available: true });
    }
  });

  it("names the ecosystem it cannot debug rather than disabling silently", () => {
    const result = debugAvailability(config({ id: "a", ecosystem: "cargo" }), []);
    expect(result.available).toBe(false);
    expect(result.available === false && result.reason).toContain("cargo");
  });

  it("refuses a test configuration, as start_debug does", () => {
    expect(debugAvailability(config({ id: "t", kind: "test" }), []).available).toBe(false);
  });

  it("allows a compound whose every member has an adapter", () => {
    const api = config({ id: "api", ecosystem: "dotnet" });
    const web = config({ id: "web", ecosystem: "node" });
    const all = config({ id: "all", ecosystem: "compound", compound: ["api", "web"] });

    expect(debugAvailability(all, [api, web, all])).toEqual({ available: true });
  });

  it("names the one member that makes a compound undebuggable", () => {
    // The backend preflights every member and launches none if one fails; the
    // button must not invite a click that builds first and then refuses.
    const api = config({ id: "api", ecosystem: "dotnet" });
    const rust = config({ id: "rust", name: "Engine", ecosystem: "cargo" });
    const all = config({ id: "all", ecosystem: "compound", compound: ["api", "rust"] });

    const result = debugAvailability(all, [api, rust, all]);
    expect(result.available).toBe(false);
    expect(result.available === false && result.reason).toContain("Engine");
  });

  it("reports a compound member that no longer exists", () => {
    const all = config({ id: "all", ecosystem: "compound", compound: ["gone"] });
    const result = debugAvailability(all, [all]);
    expect(result.available).toBe(false);
    expect(result.available === false && result.reason).toContain("gone");
  });

  it("says an empty compound launches nothing", () => {
    const all = config({ id: "all", ecosystem: "compound" });
    expect(debugAvailability(all, [all]).available).toBe(false);
  });

  it("has nothing to offer with no configuration selected", () => {
    expect(debugAvailability(null, []).available).toBe(false);
  });
});

const root = "C:/repo";

describe("debugEffects", () => {
  it("passes adapter output through to the console", () => {
    const event: DebugEvent = { type: "output", stream: "stderr", text: "boom\n" };
    expect(debugEffects(event, root)).toEqual([
      { kind: "process", event: { type: "output", stream: "stderr", text: "boom\n" } },
    ]);
  });

  it("opens the session on starting", () => {
    const effects = debugEffects({ type: "state", state: { kind: "starting" } }, root);
    expect(effects[0]).toEqual({
      kind: "process",
      event: { type: "started", pid: null, program: "debug adapter", args: [], cwd: root },
    });
    expect(effects[1]).toEqual({ kind: "status", status: "running" });
  });

  it("marks the tab up once the debuggee is running", () => {
    expect(debugEffects({ type: "state", state: { kind: "running" } }, root)).toEqual([
      { kind: "status", status: "ok" },
    ]);
  });

  it("treats a pause as the debugger working, not as a failure", () => {
    const state = {
      kind: "paused" as const,
      reason: "breakpoint",
      threadId: 1,
      description: null,
    };
    expect(debugEffects({ type: "state", state }, root)).toEqual([
      { kind: "status", status: "ok" },
    ]);
  });

  it("reports a non-zero exit as a failure and a zero exit as success", () => {
    const fail = debugEffects({ type: "state", state: { kind: "exited", code: 3 } }, root);
    expect(fail).toEqual([
      {
        kind: "process",
        event: { type: "exited", code: 3, success: false, durationMs: 0, cancelled: false },
      },
    ]);
    const ok = debugEffects({ type: "state", state: { kind: "exited", code: 0 } }, root);
    expect(ok[0]).toMatchObject({ event: { success: true } });
  });

  it("does not paint a stop red: a null exit code is not a failure", () => {
    // `run_prepared` emits `Exited { code: None }` when the session was stopped
    // or replaced, which is the most common way this arrives.
    const effects = debugEffects({ type: "state", state: { kind: "exited", code: null } }, root);
    expect(effects[0]).toMatchObject({ event: { code: null, success: true } });
  });

  it("keeps everything a missing adapter was looked for under", () => {
    const state = {
      kind: "notInstalled" as const,
      lookedFor: ["netcoredbg (not on PATH)", "CB_DAP_DOTNET (unset)"],
      hint: "Install NetCoreDbg",
    };
    const effects = debugEffects({ type: "state", state }, root);
    const message = (effects[0] as { event: { message: string } }).event.message;
    expect(message).toContain("Install NetCoreDbg");
    expect(message).toContain("netcoredbg (not on PATH)");
    expect(message).toContain("CB_DAP_DOTNET (unset)");
  });

  it("keeps a failure's detail", () => {
    const state = { kind: "failed" as const, detail: "MSBuild could not resolve TargetPath" };
    expect(debugEffects({ type: "state", state }, root)).toEqual([
      { kind: "process", event: { type: "failed", message: state.detail } },
    ]);
  });

  it("invents nothing for notRunning", () => {
    expect(debugEffects({ type: "state", state: { kind: "notRunning" } }, root)).toEqual([]);
  });
});
