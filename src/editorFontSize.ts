/**
 * Applying the editor font size, and telling live editors it moved.
 *
 * Split from `editorFontSizeLogic.ts` because everything here touches the DOM
 * and the test suite runs in a node environment. The decisions — clamping,
 * stepping, reading storage, recognising the keystroke — live there.
 *
 * Editors subscribe rather than being handed the value, because the size is a
 * property of the whole app and the editors are scattered across three
 * unrelated views. CodeMirror caches character metrics, so an editor that is
 * not told to re-measure keeps laying out at the old size until something else
 * disturbs it: its own `ResizeObserver` watches `.cm-scroller`, whose box does
 * not change when a fixed-height editor's text does.
 */
import {
  clampFontSize,
  EDITOR_FONT_SIZE_KEY,
  EDITOR_FONT_SIZE_PROPERTY,
  readFontSize,
} from "./editorFontSizeLogic";

/** Fired on `window` after the size changes. */
const CHANGE_EVENT = "code-basics:editor-font-size";

/** The size chosen last time the app ran. */
export function loadEditorFontSize(): number {
  return readFontSize(localStorage.getItem(EDITOR_FONT_SIZE_KEY));
}

/**
 * Apply a size: set the CSS property, remember it, and wake every editor.
 *
 * Returns what was actually applied, which is the clamped value rather than
 * the requested one.
 */
export function applyEditorFontSize(size: number): number {
  const applied = clampFontSize(size);

  document.documentElement.style.setProperty(EDITOR_FONT_SIZE_PROPERTY, `${applied}px`);
  localStorage.setItem(EDITOR_FONT_SIZE_KEY, String(applied));
  window.dispatchEvent(new CustomEvent(CHANGE_EVENT));

  return applied;
}

/**
 * Run `onChange` whenever the size moves. Returns the unsubscribe function.
 */
export function onEditorFontSizeChange(onChange: () => void): () => void {
  window.addEventListener(CHANGE_EVENT, onChange);
  return () => window.removeEventListener(CHANGE_EVENT, onChange);
}
