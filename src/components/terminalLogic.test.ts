import { describe, expect, it } from "vitest";
import {
  cascadeShift,
  makeTerminal,
  outputNeedsAttention,
  terminalLayoutKey,
} from "./terminalLogic";

describe("makeTerminal", () => {
  it("derives a stable key, a human title, and carries the workspace cwd", () => {
    expect(makeTerminal(1, "/ws")).toEqual({ key: "term-1", title: "Terminal 1", cwd: "/ws" });
    expect(makeTerminal(42, "/other")).toEqual({
      key: "term-42",
      title: "Terminal 42",
      cwd: "/other",
    });
  });

  it("gives distinct keys to distinct sequence numbers", () => {
    expect(makeTerminal(2, "/ws").key).not.toBe(makeTerminal(3, "/ws").key);
  });
});

describe("cascadeShift", () => {
  it("does not offset the first terminal", () => {
    expect(cascadeShift(0)).toBe(0);
  });

  it("offsets each subsequent terminal by a fixed step", () => {
    expect(cascadeShift(1, 28)).toBe(28);
    expect(cascadeShift(3, 28)).toBe(84);
  });

  it("wraps so a long-lived session never marches a terminal off-screen", () => {
    // Six steps then back to zero: the seventh terminal lands where the first
    // did rather than continuing off the edge.
    expect(cascadeShift(6, 28)).toBe(0);
    expect(cascadeShift(7, 28)).toBe(28);
  });
});

describe("outputNeedsAttention", () => {
  it("never flashes while the panel is visible", () => {
    expect(outputNeedsAttention(false, "lots of output")).toBe(false);
    expect(outputNeedsAttention(false, String.fromCharCode(7))).toBe(false);
  });

  it("flashes for real output while minimized", () => {
    expect(outputNeedsAttention(true, "build finished")).toBe(true);
  });

  it("does not flash for an empty chunk while minimized", () => {
    // The stream can carry an empty string; that is not a reason to flash.
    expect(outputNeedsAttention(true, "")).toBe(false);
  });

  it("flashes on the bell even if that is all that was sent", () => {
    expect(outputNeedsAttention(true, String.fromCharCode(7))).toBe(true);
  });
});

describe("terminalLayoutKey", () => {
  it("scopes the layout by workspace root so two codebases do not share geometry", () => {
    expect(terminalLayoutKey("/a")).toBe("cb.terminal.layout:/a");
    expect(terminalLayoutKey("/a")).not.toBe(terminalLayoutKey("/b"));
  });

  it("is distinct from the agent panel's key so their layouts do not collide", () => {
    expect(terminalLayoutKey("/a")).not.toBe("cb.agentPanel.layout");
  });
});
