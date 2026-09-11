export const APPEARANCE_STORAGE_KEY = "code-basics.appearance.v1";
export const APPEARANCE_VERSION = 1 as const;

export const COLOR_KEYS = [
  "bg", "bgRaised", "bgInset", "border", "borderStrong", "text", "textDim", "textFaint",
  "accent", "accentDim", "pass", "fail", "skip", "addBg", "delBg", "diffAddLine",
  "diffAddWord", "diffDelLine", "diffDelWord", "diffAddEdge", "diffDelEdge", "diffModEdge",
  "syntaxKeyword", "syntaxString", "syntaxComment", "syntaxNumber", "syntaxLiteral",
  "syntaxFunction", "syntaxType", "syntaxProperty", "syntaxTag", "syntaxOperator",
  "syntaxBracket", "syntaxRegexp", "syntaxMeta", "syntaxInvalid", "syntaxLink",
] as const;

export type ThemeColorKey = (typeof COLOR_KEYS)[number];
export type ThemeColors = Record<ThemeColorKey, string>;
export interface ThemeFonts { ui: string; code: string }
export interface ThemeDefinition {
  id: string;
  name: string;
  mode: "dark" | "light";
  colors: ThemeColors;
  fonts: ThemeFonts;
}
export interface AppearanceSettings {
  version: typeof APPEARANCE_VERSION;
  activeThemeId: string;
  customThemes: ThemeDefinition[];
  uiFontSize: number;
  codeFontSize: number;
  /**
   * The window background's opacity, as a **percentage** (30-100).
   *
   * A percentage rather than a 0-1 fraction so one unit runs end to end: the
   * slider reads 60, the label reads `60%`, and the CSS custom property is
   * `60%`. 100 is "off", and is byte-identical to the window before the feature
   * existed — see `styles.css`'s `--app-bg-opacity`.
   *
   * Stored here because it is an appearance preference, but *applied* by
   * `windowTransparencyLogic`, which also knows whether anything is open to
   * read. `applyAppearance` deliberately does not write it.
   */
  windowOpacity: number;
  /**
   * The opacity of a file editor or terminal **while it does not have focus**,
   * as a percentage (30-100). 100 is "off" — the unfocused surface looks exactly
   * as it does today. Lower values fade the editors and terminals the user is
   * not typing in, so the active one stands out.
   *
   * A **separate** control from {@link windowOpacity}: that fades the whole
   * window's background, this fades individual inactive surfaces. Unlike
   * `windowOpacity` this is not runtime-gated on what is open, so it *is* written
   * by `applyAppearance` (as the `--unfocused-opacity` fraction) rather than by
   * `windowTransparencyLogic`.
   */
  unfocusedOpacity: number;
}

const darkColors: ThemeColors = {
  bg: "#16181d", bgRaised: "#1c1f26", bgInset: "#12141a", border: "#2a2e37",
  borderStrong: "#3a3f4b", text: "#d6dae2", textDim: "#8b93a3", textFaint: "#5f6675",
  accent: "#5a78dc", accentDim: "#3d55a8", pass: "#4fb573", fail: "#e05561",
  skip: "#c9a227", addBg: "rgba(79, 181, 115, 0.14)", delBg: "rgba(224, 85, 97, 0.14)",
  diffAddLine: "rgba(79, 181, 115, 0.16)", diffAddWord: "rgba(79, 181, 115, 0.4)",
  diffDelLine: "rgba(224, 85, 97, 0.16)", diffDelWord: "rgba(224, 85, 97, 0.4)",
  diffAddEdge: "#4fb573", diffDelEdge: "#e05561", diffModEdge: "#5a78dc",
  syntaxKeyword: "#c586c0", syntaxString: "#ce9178", syntaxComment: "#6a9955",
  syntaxNumber: "#b5cea8", syntaxLiteral: "#569cd6", syntaxFunction: "#dcdcaa",
  syntaxType: "#4ec9b0", syntaxProperty: "#9cdcfe", syntaxTag: "#569cd6",
  syntaxOperator: "#d4d4d4", syntaxBracket: "#f2c55c", syntaxRegexp: "#d16969",
  syntaxMeta: "#8b93a3", syntaxInvalid: "#e05561", syntaxLink: "#5a78dc",
};

const lightColors: ThemeColors = {
  bg: "#f5f6f8", bgRaised: "#ffffff", bgInset: "#eceff3", border: "#d8dce3",
  borderStrong: "#b8bec9", text: "#20242c", textDim: "#5b6472", textFaint: "#8a929e",
  accent: "#315fca", accentDim: "#dbe6ff", pass: "#287a45", fail: "#c73545",
  skip: "#8a6900", addBg: "rgba(40, 122, 69, 0.12)", delBg: "rgba(199, 53, 69, 0.12)",
  diffAddLine: "rgba(40, 122, 69, 0.14)", diffAddWord: "rgba(40, 122, 69, 0.3)",
  diffDelLine: "rgba(199, 53, 69, 0.14)", diffDelWord: "rgba(199, 53, 69, 0.3)",
  diffAddEdge: "#287a45", diffDelEdge: "#c73545", diffModEdge: "#315fca",
  syntaxKeyword: "#7a3e9d", syntaxString: "#a33b20", syntaxComment: "#4f7f3b",
  syntaxNumber: "#376a3f", syntaxLiteral: "#2458a6", syntaxFunction: "#795b00",
  syntaxType: "#08756b", syntaxProperty: "#135e96", syntaxTag: "#2458a6",
  syntaxOperator: "#30343b", syntaxBracket: "#9a6500", syntaxRegexp: "#a22929",
  syntaxMeta: "#5b6472", syntaxInvalid: "#c73545", syntaxLink: "#315fca",
};

const fonts: ThemeFonts = {
  ui: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Ubuntu, sans-serif',
  code: '"JetBrains Mono", "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
};

export const BUILTIN_THEMES: ThemeDefinition[] = [
  { id: "builtin-dark", name: "Dark", mode: "dark", colors: darkColors, fonts },
  { id: "builtin-light", name: "Light", mode: "light", colors: lightColors, fonts },
];

export const DEFAULT_APPEARANCE: AppearanceSettings = {
  version: APPEARANCE_VERSION,
  activeThemeId: BUILTIN_THEMES[0]!.id,
  customThemes: [],
  uiFontSize: 13,
  codeFontSize: 12.5,
  windowOpacity: 100,
  unfocusedOpacity: 100,
};

export function clampUiFontSize(value: number): number {
  return Number.isFinite(value) ? Math.min(24, Math.max(10, value)) : 13;
}
export function clampCodeFontSize(value: number): number {
  return Number.isFinite(value) ? Math.min(32, Math.max(8, value)) : 12.5;
}
/**
 * The window's background opacity, as a percentage.
 *
 * Note the deliberate asymmetry with the two clamps above: theirs fall back to
 * their own default, and so does this one — but here the default is also the
 * *safe* answer. A stored value that is not a number must resolve to a readable
 * window, never to the floor, because this is the one preference in the file
 * that can make the application hard to use.
 *
 * The floor is 30 rather than 0 for the same reason. The chrome paints itself
 * opaque so the window would still be findable at 0, but the file tree and the
 * console labels sit on the see-through layer, and the only route back from an
 * unreadable one is a dialog rendered on top of it.
 */
export function clampWindowOpacity(value: number): number {
  return Number.isFinite(value) ? Math.min(100, Math.max(30, value)) : 100;
}

/**
 * The unfocused-surface opacity, as a percentage. Same shape and safe-default
 * reasoning as {@link clampWindowOpacity}: a non-number resolves to 100 (off),
 * the floor is 30 so a faded editor never becomes fully invisible, and focusing
 * a surface always restores it to full opacity regardless of this value.
 */
export function clampUnfocusedOpacity(value: number): number {
  return Number.isFinite(value) ? Math.min(100, Math.max(30, value)) : 100;
}

export function allThemes(settings: AppearanceSettings): ThemeDefinition[] {
  return [...BUILTIN_THEMES, ...settings.customThemes];
}

export function activeTheme(settings: AppearanceSettings): ThemeDefinition {
  return allThemes(settings).find((theme) => theme.id === settings.activeThemeId) ?? BUILTIN_THEMES[0]!;
}

export function isThemeDefinition(value: unknown): value is ThemeDefinition {
  if (!value || typeof value !== "object") return false;
  const theme = value as Partial<ThemeDefinition>;
  if (typeof theme.id !== "string" || !theme.id || typeof theme.name !== "string" || !theme.name.trim()) return false;
  if (theme.mode !== "dark" && theme.mode !== "light") return false;
  if (!theme.fonts || typeof theme.fonts.ui !== "string" || typeof theme.fonts.code !== "string") return false;
  if (!theme.fonts.ui.trim() || !theme.fonts.code.trim() || !theme.colors) return false;
  return COLOR_KEYS.every((key) => typeof theme.colors?.[key] === "string" && theme.colors[key].trim() !== "");
}

export function readAppearance(raw: string | null, legacyCodeSize?: string | null): AppearanceSettings {
  try {
    const parsed = raw ? JSON.parse(raw) as Partial<AppearanceSettings> : null;
    if (parsed?.version === APPEARANCE_VERSION) {
      const customThemes = Array.isArray(parsed.customThemes) ? parsed.customThemes.filter(isThemeDefinition) : [];
      const requested = typeof parsed.activeThemeId === "string" ? parsed.activeThemeId : DEFAULT_APPEARANCE.activeThemeId;
      const activeThemeId = [...BUILTIN_THEMES, ...customThemes].some((theme) => theme.id === requested)
        ? requested : DEFAULT_APPEARANCE.activeThemeId;
      return {
        version: APPEARANCE_VERSION,
        activeThemeId,
        customThemes,
        uiFontSize: clampUiFontSize(Number(parsed.uiFontSize)),
        codeFontSize: clampCodeFontSize(Number(parsed.codeFontSize)),
        // Absent in every blob written before the slider existed:
        // `undefined` -> `NaN` -> 100, which is exactly "the user never chose
        // this". That is why adding this field needed no version bump — and a
        // bump would have been destructive, since the gate above rejects a blob
        // of any other version and the fallback discards its custom themes.
        windowOpacity: clampWindowOpacity(Number(parsed.windowOpacity)),
        // Absent in blobs written before this control existed: `undefined` ->
        // `NaN` -> 100 (off), the same safe-absent path as `windowOpacity`, so
        // adding it needs no version bump.
        unfocusedOpacity: clampUnfocusedOpacity(Number(parsed.unfocusedOpacity)),
      };
    }
  } catch { /* malformed user storage falls back below */ }
  const migrated = legacyCodeSize == null || legacyCodeSize.trim() === "" ? 12.5 : Number(legacyCodeSize);
  return { ...DEFAULT_APPEARANCE, codeFontSize: clampCodeFontSize(migrated) };
}

export interface ThemeFile { version: 1; theme: ThemeDefinition }
export function parseThemeFile(raw: string): ThemeDefinition | null {
  try {
    const file = JSON.parse(raw) as Partial<ThemeFile>;
    return file.version === 1 && isThemeDefinition(file.theme) ? file.theme : null;
  } catch { return null; }
}
