import { describe, expect, it } from "vitest";
import { BUILTIN_THEMES, DEFAULT_APPEARANCE, activeTheme, parseThemeFile, readAppearance } from "./appearanceLogic";

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

  it("round trips an exported theme file", () => {
    const theme = BUILTIN_THEMES[1]!;
    expect(parseThemeFile(JSON.stringify({ version: 1, theme }))).toEqual(theme);
    expect(parseThemeFile(JSON.stringify({ version: 2, theme }))).toBeNull();
  });
});
