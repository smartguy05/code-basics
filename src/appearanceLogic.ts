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
};

export function clampUiFontSize(value: number): number {
  return Number.isFinite(value) ? Math.min(24, Math.max(10, value)) : 13;
}
export function clampCodeFontSize(value: number): number {
  return Number.isFinite(value) ? Math.min(32, Math.max(8, value)) : 12.5;
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
