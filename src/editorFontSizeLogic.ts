/**
 * The one font size every CodeMirror editor in the app reads.
 *
 * The size is applied as a CSS custom property on the document element rather
 * than as a CodeMirror theme, so changing it never rebuilds an editor — a
 * rebuilt diff loses its scroll position, its selection and the lines the user
 * had picked for staging, which is a high price for making text bigger.
 *
 * Everything here is pure: vitest runs in a node environment with no DOM, so
 * the keybinding is decided from a structural event (see `ShortcutEvent` in
 * `components/searchLogic.ts`) and the storage read takes the raw string.
 */
import type { ShortcutEvent } from "./components/searchLogic";

/** Where the chosen size is remembered between sessions. */
export const EDITOR_FONT_SIZE_KEY = "code-basics.editorFontSize";

/** The size every editor used before this was configurable. */
export const DEFAULT_EDITOR_FONT_SIZE = 12.5;

/** Below this, line numbers and the change gutter stop being legible. */
export const MIN_EDITOR_FONT_SIZE = 8;

/** Above this, a side-by-side diff no longer fits a useful amount of code. */
export const MAX_EDITOR_FONT_SIZE = 32;

/** The CSS custom property `styles.css` reads. */
export const EDITOR_FONT_SIZE_PROPERTY = "--editor-font-size";

/** What a font-size keystroke asked for. */
export type FontSizeAction = "increase" | "decrease" | "reset";

/**
 * Hold a size inside the usable range.
 *
 * A value that is not a finite number becomes the default rather than
 * propagating `NaN` into a CSS property, where it would silently drop the rule
 * and leave the editor at whatever it inherited.
 */
export function clampFontSize(size: number): number {
  if (!Number.isFinite(size)) return DEFAULT_EDITOR_FONT_SIZE;
  return Math.min(MAX_EDITOR_FONT_SIZE, Math.max(MIN_EDITOR_FONT_SIZE, size));
}

/**
 * Move the size by whole points.
 *
 * Rounded towards the step direction so the fractional default (12.5px) lands
 * on a round number on the first press instead of carrying the half through
 * every step after it.
 */
export function stepFontSize(size: number, steps: number): number {
  const from = clampFontSize(size);
  const rounded = steps > 0 ? Math.floor(from) : Math.ceil(from);
  return clampFontSize(rounded + steps);
}

/**
 * The stored size, or the default.
 *
 * localStorage is user-editable and survives a downgrade, so the stored value
 * is untrusted input rather than something this app necessarily wrote.
 */
export function readFontSize(raw: string | null): number {
  if (raw == null || raw.trim() === "") return DEFAULT_EDITOR_FONT_SIZE;
  return clampFontSize(Number(raw));
}

/**
 * Which font-size action a keystroke asked for, if any.
 *
 * `Ctrl` (or `Cmd`) is required: these are keys people type into the commit
 * message box, and resizing every editor in the app as a side effect of typing
 * "0" would be worse than having no shortcut at all. `Alt` excludes the
 * combination outright rather than being ignored, so a future Alt binding
 * cannot silently fire this one too.
 */
export function recogniseFontSizeShortcut(event: ShortcutEvent): FontSizeAction | null {
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return null;

  switch (event.key) {
    // "+" is what Ctrl+Shift+= and the numpad both report.
    case "=":
    case "+":
      return "increase";
    case "-":
    case "_":
      return "decrease";
    case "0":
      return "reset";
    default:
      return null;
  }
}
