import { describe, expect, it } from "vitest";
import {
  isDebugTerminal,
  isProcessTerminal,
  isSqlTerminal,
  isTerminalStreamTerminal,
} from "./channelLogic";
import type { DebugEvent, ProcessEvent, SqlEvent, TerminalEvent } from "./types";

describe("isProcessTerminal", () => {
  it("is true for exited and failed, false for started and output", () => {
    const exited: ProcessEvent = {
      type: "exited",
      code: 0,
      success: true,
      durationMs: 1,
      cancelled: false,
    };
    const failed: ProcessEvent = { type: "failed", message: "boom" };
    const started: ProcessEvent = { type: "started", pid: 1, program: "x", args: [], cwd: "/" };
    const output: ProcessEvent = { type: "output", stream: "stdout", text: "hi" };
    expect(isProcessTerminal(exited)).toBe(true);
    expect(isProcessTerminal(failed)).toBe(true);
    expect(isProcessTerminal(started)).toBe(false);
    expect(isProcessTerminal(output)).toBe(false);
  });
});

describe("isTerminalStreamTerminal", () => {
  it("is true only when the shell exits or the spawn fails", () => {
    const exited: TerminalEvent = { type: "exited", code: 0, success: true };
    const failed: TerminalEvent = { type: "failed", message: "no shell" };
    const output: TerminalEvent = { type: "output", text: "$ " };
    expect(isTerminalStreamTerminal(exited)).toBe(true);
    expect(isTerminalStreamTerminal(failed)).toBe(true);
    expect(isTerminalStreamTerminal(output)).toBe(false);
  });
});

describe("isDebugTerminal", () => {
  it("releases only on session-ending states", () => {
    const exited: DebugEvent = { type: "state", state: { kind: "exited", code: null } };
    const failed: DebugEvent = { type: "state", state: { kind: "failed", detail: "x" } };
    const notInstalled: DebugEvent = {
      type: "state",
      state: { kind: "notInstalled", lookedFor: [], hint: "" },
    };
    const running: DebugEvent = { type: "state", state: { kind: "running" } };
    const starting: DebugEvent = { type: "state", state: { kind: "starting" } };
    const paused: DebugEvent = {
      type: "state",
      state: { kind: "paused", reason: "bp", threadId: 1, description: null },
    };
    const output: DebugEvent = { type: "output", stream: "stdout", text: "log" };
    expect(isDebugTerminal(exited)).toBe(true);
    expect(isDebugTerminal(failed)).toBe(true);
    expect(isDebugTerminal(notInstalled)).toBe(true);
    expect(isDebugTerminal(running)).toBe(false);
    expect(isDebugTerminal(starting)).toBe(false);
    expect(isDebugTerminal(paused)).toBe(false);
    expect(isDebugTerminal(output)).toBe(false);
  });
});

describe("isSqlTerminal", () => {
  it("is true only for the whole-query finished, not a per-statement completed", () => {
    const finished: SqlEvent = { kind: "finished", cancelled: false };
    const rows: SqlEvent = { kind: "rows", statementIndex: 0, rows: [] };
    const failed: SqlEvent = { kind: "failed", statementIndex: 0, message: "syntax" };
    expect(isSqlTerminal(finished)).toBe(true);
    expect(isSqlTerminal(rows)).toBe(false);
    expect(isSqlTerminal(failed)).toBe(false);
  });
});
