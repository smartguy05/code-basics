import {
  activeTheme, APPEARANCE_STORAGE_KEY, COLOR_KEYS, DEFAULT_APPEARANCE,
  readAppearance, type AppearanceSettings, type ThemeColorKey,
} from "./appearanceLogic";
import { EDITOR_FONT_SIZE_KEY } from "./editorFontSizeLogic";

export const APPEARANCE_CHANGE_EVENT = "code-basics:appearance";

const cssNames: Record<ThemeColorKey, string> = {
  bg: "--bg", bgRaised: "--bg-raised", bgInset: "--bg-inset", border: "--border",
  borderStrong: "--border-strong", text: "--text", textDim: "--text-dim", textFaint: "--text-faint",
  accent: "--accent", accentDim: "--accent-dim", pass: "--pass", fail: "--fail", skip: "--skip",
  addBg: "--add-bg", delBg: "--del-bg", diffAddLine: "--diff-add-line", diffAddWord: "--diff-add-word",
  diffDelLine: "--diff-del-line", diffDelWord: "--diff-del-word", diffAddEdge: "--diff-add-edge",
  diffDelEdge: "--diff-del-edge", diffModEdge: "--diff-mod-edge", syntaxKeyword: "--syntax-keyword",
  syntaxString: "--syntax-string", syntaxComment: "--syntax-comment", syntaxNumber: "--syntax-number",
  syntaxLiteral: "--syntax-literal", syntaxFunction: "--syntax-function", syntaxType: "--syntax-type",
  syntaxProperty: "--syntax-property", syntaxTag: "--syntax-tag", syntaxOperator: "--syntax-operator",
  syntaxBracket: "--syntax-bracket", syntaxRegexp: "--syntax-regexp", syntaxMeta: "--syntax-meta",
  syntaxInvalid: "--syntax-invalid", syntaxLink: "--syntax-link",
};

export function loadAppearance(): AppearanceSettings {
  return readAppearance(localStorage.getItem(APPEARANCE_STORAGE_KEY), localStorage.getItem(EDITOR_FONT_SIZE_KEY));
}

export function validThemeColors(settings: AppearanceSettings): boolean {
  const colors = activeTheme(settings).colors;
  return COLOR_KEYS.every((key) => CSS.supports("color", colors[key]));
}

export function applyAppearance(settings: AppearanceSettings, persist = false): void {
  const root = document.documentElement;
  const theme = activeTheme(settings);
  for (const key of COLOR_KEYS) root.style.setProperty(cssNames[key], theme.colors[key]);
  root.style.setProperty("--font", theme.fonts.ui);
  root.style.setProperty("--mono", theme.fonts.code);
  root.style.setProperty("--ui-font-size", `${settings.uiFontSize}px`);
  root.style.setProperty("--editor-font-size", `${settings.codeFontSize}px`);
  root.dataset.theme = theme.mode;
  root.style.colorScheme = theme.mode;
  if (persist) localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(settings));
  window.dispatchEvent(new CustomEvent(APPEARANCE_CHANGE_EVENT, { detail: settings }));
  window.dispatchEvent(new CustomEvent("code-basics:editor-font-size"));
}

export function resetAppearance(): AppearanceSettings {
  applyAppearance(DEFAULT_APPEARANCE, true);
  return DEFAULT_APPEARANCE;
}

export function onAppearanceChange(listener: () => void): () => void {
  window.addEventListener(APPEARANCE_CHANGE_EVENT, listener);
  return () => window.removeEventListener(APPEARANCE_CHANGE_EVENT, listener);
}

export function terminalAppearance(): { fontFamily: string; fontSize: number; theme: { background: string; foreground: string; cursor: string; selectionBackground: string } } {
  const style = getComputedStyle(document.documentElement);
  const read = (name: string) => style.getPropertyValue(name).trim();
  return {
    fontFamily: read("--mono"),
    fontSize: loadAppearance().codeFontSize,
    theme: {
      background: read("--bg-inset"), foreground: read("--text"), cursor: read("--text"),
      selectionBackground: read("--accent-dim"),
    },
  };
}
