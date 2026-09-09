import { describe, expect, it } from "vitest";
import {
  BUILTIN_THEMES,
  DEFAULT_APPEARANCE,
  activeTheme,
  clampUnfocusedOpacity,
  clampWindowOpacity,
  parseThemeFile,
  readAppearance,
} from "./appearanceLogic";

describe("appearance persistence", () => {
  it("migrates the old editor size into the shared code size", () => {
    expect(readAppearance(null, "18").codeFontSize).toBe(18);
  });

  it("bounds malformed stored sizes and falls back from a missing theme", () => {
    const read = readAppearance(JSON.stringify({ ...DEFAULT_APPEARANCE, uiFontSize: 100, codeFontSize: -1, activeThemeId: "gone" }));
    expect(read.uiFontSize).toBe(24);
    expect(read.codeFontSize).toBe(8);
    expect(activeTheme(read).id).toBe(BUILTIN_THEMES[0]!.id);
  });

  it("ships the window fully opaque", () => {
    // The feature is off until the user turns it on.
    expect(DEFAULT_APPEARANCE.windowOpacity).toBe(100);
  });

  it("resolves a malformed window opacity to a readable window, not to the floor", () => {
    // The deliberate asymmetry with the font clamps: garbage here can make the
    // app unusable, so the non-finite answer is the *safe* one rather than the
    // nearest bound.
    expect(clampWindowOpacity(Number.NaN)).toBe(100);
    expect(clampWindowOpacity(250)).toBe(100);
    expect(clampWindowOpacity(1)).toBe(30);
  });

  it("reads a blob written before the slider existed as opaque, keeping everything else", () => {
    // The whole no-migration claim in one test. `readAppearance` re-derives each
    // field rather than spreading the parsed object, so an absent key is
    // `undefined` -> `NaN` -> 100, which is exactly "the user never chose this".
    // Bumping the schema version instead would make the gate reject this blob
    // and discard the custom theme and font sizes with it.
    const theme = { ...BUILTIN_THEMES[1]!, id: "mine", name: "Mine" };
    const read = readAppearance(
      JSON.stringify({
        version: 1,
        activeThemeId: "mine",
        customThemes: [theme],
        uiFontSize: 15,
        codeFontSize: 14,
      }),
    );
    expect(read.windowOpacity).toBe(100);
    expect(read.customThemes).toHaveLength(1);
    expect(read.uiFontSize).toBe(15);
    expect(activeTheme(read).id).toBe("mine");
  });

  it("clamps a stored window opacity rather than rejecting the whole blob", () => {
    const read = readAppearance(JSON.stringify({ ...DEFAULT_APPEARANCE, windowOpacity: 5 }));
    expect(read.windowOpacity).toBe(30);
    expect(read.uiFontSize).toBe(DEFAULT_APPEARANCE.uiFontSize);
  });

  it("ships the unfocused-opacity effect off, and resolves garbage to off", () => {
    expect(DEFAULT_APPEARANCE.unfocusedOpacity).toBe(100);
    expect(clampUnfocusedOpacity(Number.NaN)).toBe(100);
    expect(clampUnfocusedOpacity(250)).toBe(100);
    expect(clampUnfocusedOpacity(1)).toBe(30);
  });

  it("reads a blob written before unfocused opacity existed as off", () => {
    const read = readAppearance(
      JSON.stringify({ version: 1, activeThemeId: BUILTIN_THEMES[0]!.id, customThemes: [] }),
    );
    expect(read.unfocusedOpacity).toBe(100);
  });

  it("clamps a stored unfocused opacity", () => {
    expect(readAppearance(JSON.stringify({ ...DEFAULT_APPEARANCE, unfocusedOpacity: 5 })).unfocusedOpacity).toBe(30);
  });

  it("round trips an exported theme file", () => {
    const theme = BUILTIN_THEMES[1]!;
    expect(parseThemeFile(JSON.stringify({ version: 1, theme }))).toEqual(theme);
    expect(parseThemeFile(JSON.stringify({ version: 2, theme }))).toBeNull();
  });
});
