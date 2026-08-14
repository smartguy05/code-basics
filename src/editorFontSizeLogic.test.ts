import { describe, expect, it } from "vitest";
import {
  clampFontSize,
  DEFAULT_EDITOR_FONT_SIZE,
  MAX_EDITOR_FONT_SIZE,
  MIN_EDITOR_FONT_SIZE,
  readFontSize,
  recogniseFontSizeShortcut,
  stepFontSize,
  type FontSizeAction,
} from "./editorFontSizeLogic";
import type { ShortcutEvent } from "./components/searchLogic";

function press(key: string, modifiers: Partial<ShortcutEvent> = {}): ShortcutEvent {
  return {
    key,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...modifiers,
  };
}

describe("clampFontSize", () => {
  it("leaves a size inside the range alone", () => {
    expect(clampFontSize(14)).toBe(14);
  });

  it("pulls a size below the floor up to it", () => {
    expect(clampFontSize(2)).toBe(MIN_EDITOR_FONT_SIZE);
  });

  it("pulls a size above the ceiling down to it", () => {
    expect(clampFontSize(400)).toBe(MAX_EDITOR_FONT_SIZE);
  });

  it("falls back to the default for a value that is not a number", () => {
    expect(clampFontSize(Number.NaN)).toBe(DEFAULT_EDITOR_FONT_SIZE);
    expect(clampFontSize(Number.POSITIVE_INFINITY)).toBe(DEFAULT_EDITOR_FONT_SIZE);
  });
});

describe("stepFontSize", () => {
  it("moves one point per step", () => {
    expect(stepFontSize(14, 1)).toBe(15);
    expect(stepFontSize(14, -1)).toBe(13);
  });

  it("lands on whole points from the fractional default", () => {
    // The default is 12.5px, and stepping from it should reach a round number
    // rather than carrying the half through every subsequent step.
    expect(stepFontSize(DEFAULT_EDITOR_FONT_SIZE, 1)).toBe(13);
    expect(stepFontSize(DEFAULT_EDITOR_FONT_SIZE, -1)).toBe(12);
  });

  it("stops at the ends of the range rather than running past them", () => {
    expect(stepFontSize(MAX_EDITOR_FONT_SIZE, 1)).toBe(MAX_EDITOR_FONT_SIZE);
    expect(stepFontSize(MIN_EDITOR_FONT_SIZE, -1)).toBe(MIN_EDITOR_FONT_SIZE);
  });
});

describe("readFontSize", () => {
  it("reads a stored size back", () => {
    expect(readFontSize("15")).toBe(15);
    expect(readFontSize("12.5")).toBe(12.5);
  });

  it("falls back to the default when nothing is stored", () => {
    expect(readFontSize(null)).toBe(DEFAULT_EDITOR_FONT_SIZE);
  });

  it("falls back to the default for a value that will not parse", () => {
    expect(readFontSize("enormous")).toBe(DEFAULT_EDITOR_FONT_SIZE);
    expect(readFontSize("")).toBe(DEFAULT_EDITOR_FONT_SIZE);
  });

  it("clamps a stored size that is out of range", () => {
    // localStorage is user-editable and survives a downgrade, so a stored value
    // is untrusted input rather than something this app necessarily wrote.
    expect(readFontSize("9999")).toBe(MAX_EDITOR_FONT_SIZE);
  });
});

describe("recogniseFontSizeShortcut", () => {
  const cases: [string, ShortcutEvent, FontSizeAction][] = [
    ["Ctrl+=", press("=", { ctrlKey: true }), "increase"],
    ["Ctrl++ (shifted)", press("+", { ctrlKey: true, shiftKey: true }), "increase"],
    ["Ctrl+-", press("-", { ctrlKey: true }), "decrease"],
    ["Ctrl+0", press("0", { ctrlKey: true }), "reset"],
    ["Cmd+= on macOS", press("=", { metaKey: true }), "increase"],
  ];

  for (const [name, event, action] of cases) {
    it(`recognises ${name}`, () => {
      expect(recogniseFontSizeShortcut(event)).toBe(action);
    });
  }

  it("ignores the same keys without a modifier", () => {
    // Otherwise typing "=" or "0" into the commit message would resize the app.
    expect(recogniseFontSizeShortcut(press("="))).toBeNull();
    expect(recogniseFontSizeShortcut(press("0"))).toBeNull();
    expect(recogniseFontSizeShortcut(press("-"))).toBeNull();
  });

  it("ignores the combination when Alt is held", () => {
    expect(recogniseFontSizeShortcut(press("=", { ctrlKey: true, altKey: true }))).toBeNull();
  });

  it("ignores unrelated keys", () => {
    expect(recogniseFontSizeShortcut(press("k", { ctrlKey: true }))).toBeNull();
  });
});
