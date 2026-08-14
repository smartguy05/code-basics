/**
 * Whether the Run tab's console panel is put away, and how tall it is when it
 * is not — remembered per workspace.
 *
 * # Why per workspace
 *
 * The divider position was already stored, under one global key
 * (`code-basics.editorSplit`), which is wrong for the same reason the
 * environment picker is keyed per root (`RunView`'s `environmentsKey`): how much
 * of the window a developer gives the terminal is a property of what they are
 * doing in *that* repository. A service you run and watch wants the console; a
 * library you are only reading wants it gone. One global fraction makes those
 * two settings fight.
 *
 * The legacy global key is still read as a fallback by {@link loadSplit}, so
 * the first open after this change lands on the divider the user last dragged
 * rather than snapping back to the default. It is never written again, and it is
 * deliberately not deleted — a stale key costs nothing, and removing it would
 * break the fallback for every workspace not yet opened.
 *
 * # Why every value is checked on the way back in
 *
 * `localStorage` is not this app's private memory: it is editable by hand, it
 * survives builds with different limits, and it survives builds that stored a
 * different shape. The failure modes here are quiet rather than loud — a `NaN`
 * reaching a flex-basis collapses the editor pane to nothing with no error
 * anywhere, and a truthy-looking string for the collapsed flag hides the
 * terminal on startup with no way to tell why. So stored values are untrusted
 * input: the fraction is repaired by {@link clampSplit} (an out-of-range value
 * from an older build should not be unrecoverable), and the collapsed flag is
 * refused unless it is exactly what this module writes, because there is no
 * sensible repair for "some other string" and the safe direction is visible.
 *
 * This mirrors `views/architecture/viewportLogic.ts`, which reaches the same
 * conclusions for the same reasons.
 */

/** The slice of `Storage` this needs (localStorage in the app, a map in tests). */
export type PanelStorage = Pick<Storage, "getItem" | "setItem">;

/** Namespace for every stored collapsed flag, so one sweep could find them. */
export const COLLAPSED_KEY_PREFIX = "code-basics.consoleCollapsed";

/** Namespace for every stored divider position. */
export const SPLIT_KEY_PREFIX = "code-basics.editorSplit";

/**
 * The single global key this app used for the divider before it was keyed per
 * workspace. Read as a fallback, never written.
 */
export const LEGACY_SPLIT_KEY = "code-basics.editorSplit";

/** Editor pane height as a fraction of the split when nothing is stored. */
export const DEFAULT_SPLIT = 0.55;

/** The narrowest either pane may be dragged to, as a fraction of the split. */
const MIN_SPLIT = 0.1;
const MAX_SPLIT = 0.9;

/**
 * The storage key for one workspace.
 *
 * The root is percent-encoded before being joined. A Windows root contains a
 * colon (`C:/repo`), which is also the separator, so a plain join has a movable
 * boundary and two different roots could collapse onto one key — the same trap
 * `viewportKey` documents. Encoding makes the separator the only unescaped
 * colon in the string.
 *
 * Note that `SPLIT_KEY_PREFIX` equals {@link LEGACY_SPLIT_KEY}: the per-workspace
 * key is that string plus `:<encoded root>`, so the two can never collide.
 */
export function collapsedKey(root: string): string {
  return `${COLLAPSED_KEY_PREFIX}:${encodeURIComponent(root)}`;
}

export function splitKey(root: string): string {
  return `${SPLIT_KEY_PREFIX}:${encodeURIComponent(root)}`;
}

/** Read a key, treating an unavailable storage as an absent value. */
function read(storage: PanelStorage, key: string): string | null {
  try {
    // A storage that throws on read is a real state (disabled, or full), and it
    // is not worth taking the tab down for.
    return storage.getItem(key);
  } catch {
    return null;
  }
}

/** Write a key, treating an unavailable storage as nothing worth reporting. */
function write(storage: PanelStorage, key: string, value: string): void {
  try {
    storage.setItem(key, value);
  } catch {
    /* the panel is on screen and behaving; only its memory is not */
  }
}

/**
 * Hold a fraction inside the range where both panes stay visible.
 *
 * A non-number becomes {@link DEFAULT_SPLIT} rather than propagating: `Math.min`
 * and `Math.max` pass `NaN` straight through, and a `NaN` flex-basis makes the
 * editor pane vanish with nothing logged.
 */
export function clampSplit(split: number): number {
  if (Number.isNaN(split)) return DEFAULT_SPLIT;
  return Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, split));
}

/**
 * Whether the console panel was last left put away in this workspace.
 *
 * Anything other than the exact string this module writes is declined, and
 * declining means the panel is visible — the state in which every control that
 * could change it is reachable.
 */
export function loadCollapsed(storage: PanelStorage, root: string): boolean {
  return read(storage, collapsedKey(root)) === "true";
}

export function saveCollapsed(storage: PanelStorage, root: string, collapsed: boolean): void {
  write(storage, collapsedKey(root), collapsed ? "true" : "false");
}

/**
 * The divider position for this workspace, falling back to the pre-existing
 * global key and then to {@link DEFAULT_SPLIT}.
 *
 * `Number("")` is `0` and `Number(null)` is `0`, so an empty or absent value
 * would clamp to `MIN_SPLIT` and look like a deliberate setting. Both are
 * rejected by the finiteness test before the clamp sees them.
 */
export function loadSplit(storage: PanelStorage, root: string): number {
  for (const key of [splitKey(root), LEGACY_SPLIT_KEY]) {
    const raw = read(storage, key);
    if (raw === null || raw.trim() === "") continue;
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) continue;
    return clampSplit(parsed);
  }
  return DEFAULT_SPLIT;
}

/**
 * Remember the divider position.
 *
 * Called from a pointer-move path, so it must not throw — a full quota would
 * otherwise abort the drag. The value is clamped before it is written, so a
 * fraction that could not be read back is never stored in the first place.
 */
export function saveSplit(storage: PanelStorage, root: string, split: number): void {
  if (!Number.isFinite(split)) return;
  write(storage, splitKey(root), String(clampSplit(split)));
}
