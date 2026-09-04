/**
 * The DOM half of window transparency — one custom property, written in one
 * place.
 *
 * Separate from `appearance.ts` on purpose, and `applyAppearance` deliberately
 * does **not** write this var: applying it needs to know whether anything is
 * open to read, which the appearance layer cannot know. Two writers on one
 * property would fight, and the last render would win nondeterministically.
 *
 * The preference still reaches this writer through machinery that already
 * exists — `applyAppearance` dispatches `APPEARANCE_CHANGE_EVENT` carrying the
 * settings, so `App` re-decides from the event. That is what makes the
 * Settings dialog's *unpersisted* preview work with no change to its
 * buffered-draft model.
 */

/** Paint the app background at `percent`. See `styles.css`'s `--app-bg-opacity`. */
export function applyWindowOpacity(percent: number): void {
  document.documentElement.style.setProperty("--app-bg-opacity", `${percent}%`);
}
