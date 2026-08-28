import { describe, expect, it } from "vitest";
import {
  cascadeShift,
  makeTerminal,
  outputNeedsAttention,
  pillBottom,
  raiseTerminal,
  recolorTerminal,
  renameTerminal,
  stackOffset,
  syncStackOrder,
  TERMINAL_STACK_SPAN,
  terminalKeyAction,
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

describe("renameTerminal", () => {
  const list = [makeTerminal(1, "/ws"), makeTerminal(2, "/ws")];
  const titleOf = (l: ReturnType<typeof renameTerminal>, key: string) =>
    l.find((t) => t.key === key)?.title;

  it("renames the matching terminal and leaves the rest untouched", () => {
    const next = renameTerminal(list, "term-1", "Server");
    expect(titleOf(next, "term-1")).toBe("Server");
    expect(titleOf(next, "term-2")).toBe("Terminal 2");
  });

  it("trims surrounding whitespace", () => {
    expect(titleOf(renameTerminal(list, "term-2", "  Logs  "), "term-2")).toBe("Logs");
  });

  it("rejects a blank title, keeping the existing one", () => {
    expect(titleOf(renameTerminal(list, "term-1", "   "), "term-1")).toBe("Terminal 1");
    expect(titleOf(renameTerminal(list, "term-1", ""), "term-1")).toBe("Terminal 1");
  });

  it("is a no-op for an unknown key", () => {
    expect(renameTerminal(list, "term-9", "X")).toEqual(list);
  });
});

describe("recolorTerminal", () => {
  const list = [makeTerminal(1, "/ws"), makeTerminal(2, "/ws")];
  const colorOf = (l: ReturnType<typeof recolorTerminal>, key: string) =>
    l.find((t) => t.key === key)?.color;

  it("sets the colour of the matching terminal only", () => {
    const next = recolorTerminal(list, "term-2", "#7a4b00");
    expect(colorOf(next, "term-2")).toBe("#7a4b00");
    expect(colorOf(next, "term-1")).toBeUndefined();
  });

  it("clears the colour back to the theme default with undefined", () => {
    const colored = recolorTerminal(list, "term-1", "#123456");
    expect(colorOf(recolorTerminal(colored, "term-1", undefined), "term-1")).toBeUndefined();
  });
});

describe("pillBottom", () => {
  it("reserves the base slot (bottom:16) for the Notes bar", () => {
    // The first terminal pill sits one step above 16, never on it.
    expect(pillBottom(0)).toBe(64);
  });

  it("stacks each subsequent pill one step higher", () => {
    expect(pillBottom(1)).toBe(112);
    expect(pillBottom(2)).toBe(160);
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

  it("does not flash for ordinary output while minimized", () => {
    // A running terminal streams output constantly; that is not the terminal
    // asking for the user — only the bell is. Plain output must stay calm.
    expect(outputNeedsAttention(true, "build finished")).toBe(false);
  });

  it("does not flash for an empty chunk while minimized", () => {
    // The stream can carry an empty string; that is not a reason to flash.
    expect(outputNeedsAttention(true, "")).toBe(false);
  });

  it("flashes on the bell, even embedded in other output", () => {
    expect(outputNeedsAttention(true, String.fromCharCode(7))).toBe(true);
    expect(outputNeedsAttention(true, `done${String.fromCharCode(7)}`)).toBe(true);
  });
});

describe("terminalKeyAction", () => {
  const keydown = (over: Partial<Record<string, unknown>>) => ({
    type: "keydown",
    ctrlKey: false,
    shiftKey: false,
    metaKey: false,
    key: "",
    ...over,
  });

  it("copies on Ctrl+Shift+C", () => {
    expect(terminalKeyAction(keydown({ ctrlKey: true, shiftKey: true, key: "C" }), true)).toBe("copy");
    // The chord copies regardless of case reported by the platform.
    expect(terminalKeyAction(keydown({ ctrlKey: true, shiftKey: true, key: "c" }), false)).toBe("copy");
  });

  it("pastes on Ctrl+V and Ctrl+Shift+V", () => {
    // Plain Ctrl+V is the Windows-standard paste; Ctrl+Shift+V is the terminal
    // chord. Both paste — unlike Ctrl+C, paste has no interrupt to protect.
    expect(terminalKeyAction(keydown({ ctrlKey: true, key: "v" }), false)).toBe("paste");
    expect(terminalKeyAction(keydown({ ctrlKey: true, shiftKey: true, key: "v" }), false)).toBe("paste");
  });

  it("leaves plain Ctrl+C as the shell interrupt (passthrough)", () => {
    // The whole point: Ctrl+C must still interrupt, not copy.
    expect(terminalKeyAction(keydown({ ctrlKey: true, key: "c" }), true)).toBe("passthrough");
  });

  it("copies on Ctrl+Insert only when there is a selection", () => {
    expect(terminalKeyAction(keydown({ ctrlKey: true, key: "Insert" }), true)).toBe("copy");
    expect(terminalKeyAction(keydown({ ctrlKey: true, key: "Insert" }), false)).toBe("passthrough");
  });

  it("pastes on Shift+Insert", () => {
    expect(terminalKeyAction(keydown({ shiftKey: true, key: "Insert" }), false)).toBe("paste");
  });

  it("passes ordinary keystrokes through untouched", () => {
    expect(terminalKeyAction(keydown({ key: "a" }), false)).toBe("passthrough");
    expect(terminalKeyAction(keydown({ ctrlKey: true, key: "l" }), false)).toBe("passthrough");
  });

  it("only acts on keydown, never keyup/keypress", () => {
    expect(terminalKeyAction(keydown({ type: "keyup", ctrlKey: true, shiftKey: true, key: "c" }), true)).toBe(
      "passthrough",
    );
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

describe("raiseTerminal", () => {
  it("moves a key to the top, keeping the others in their order", () => {
    expect(raiseTerminal(["a", "b", "c"], "b")).toEqual(["a", "c", "b"]);
  });

  it("appends a key it has never seen, so an unreconciled terminal still raises", () => {
    expect(raiseTerminal(["a"], "z")).toEqual(["a", "z"]);
  });

  it("returns the same array when the key is already top", () => {
    // Load-bearing, not cosmetic: identity is what lets `setStackOrder` bail out,
    // so clicking the front terminal — the common case — re-renders nothing.
    const order = ["a", "b"];
    expect(raiseTerminal(order, "b")).toBe(order);
  });

  it("raising into an empty order yields just that key", () => {
    expect(raiseTerminal([], "a")).toEqual(["a"]);
  });
});

describe("syncStackOrder", () => {
  it("appends newly opened keys, so a fresh terminal starts on top", () => {
    expect(syncStackOrder(["a"], ["a", "b"])).toEqual(["a", "b"]);
  });

  it("drops keys that are no longer open", () => {
    expect(syncStackOrder(["a", "b", "c"], ["c", "a"])).toEqual(["a", "c"]);
  });

  it("never reorders to match the open list", () => {
    // This *is* the index-decoupling contract: the `terminals` array order drives
    // `pillBottom` and `cascadeShift`, and must never dictate stacking.
    const order = ["b", "a"];
    expect(syncStackOrder(order, ["a", "b"])).toBe(order);
  });

  it("returns the same array when nothing changed, so the reconciling effect cannot loop", () => {
    const order = ["a", "b"];
    expect(syncStackOrder(order, ["a", "b"])).toBe(order);
  });

  it("handles the top closing while a new one opens in the same commit", () => {
    expect(syncStackOrder(["a", "b"], ["a", "c"])).toEqual(["a", "c"]);
  });

  it("an empty open list yields an empty order", () => {
    expect(syncStackOrder(["a", "b"], [])).toEqual([]);
  });
});

describe("stackOffset", () => {
  it("numbers the order from the bottom, so the last key is the top", () => {
    expect(stackOffset(["a", "b", "c"], "a")).toBe(0);
    expect(stackOffset(["a", "b", "c"], "c")).toBe(2);
  });

  it("returns 0 for a key not in the order, never NaN", () => {
    // A terminal rendered in the commit before the reconciling effect runs must
    // still render somewhere.
    expect(stackOffset(["a"], "z")).toBe(0);
  });

  it("never exceeds the band the stylesheet reserves", () => {
    const keys = Array.from({ length: TERMINAL_STACK_SPAN + 5 }, (_, i) => `t${i}`);
    for (const key of keys) {
      const offset = stackOffset(keys, key);
      expect(offset).toBeGreaterThanOrEqual(0);
      expect(offset).toBeLessThanOrEqual(TERMINAL_STACK_SPAN - 1);
    }
    // `at(-1)` rather than an index: `noUncheckedIndexedAccess` widens an index
    // read to `string | undefined`, and the top key is what the assertion is about.
    expect(stackOffset(keys, keys.at(-1) ?? "")).toBe(TERMINAL_STACK_SPAN - 1);
  });

  it("stays non-decreasing along the order even while clamped", () => {
    const keys = Array.from({ length: TERMINAL_STACK_SPAN + 5 }, (_, i) => `t${i}`);
    // Walked with `for...of` rather than by index: `noUncheckedIndexedAccess`
    // would widen an indexed read to `number | undefined`.
    let previous = 0;
    for (const offset of keys.map((k) => stackOffset(keys, k))) {
      expect(offset).toBeGreaterThanOrEqual(previous);
      previous = offset;
    }
  });

  it("pins the span against the stylesheet's --z-panel-stack-span", () => {
    // Deliberate drift alarm: styles.css reserves this many steps for the
    // terminal band. Changing one without the other silently lets a terminal
    // climb into the Notes band.
    expect(TERMINAL_STACK_SPAN).toBe(100);
  });
});
