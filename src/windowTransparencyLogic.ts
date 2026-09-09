/**
 * When the window is translucent, and by how much.
 *
 * The stored preference lives in `appearanceLogic` with the rest of the
 * appearance settings. This module owns the *runtime* half — whether anything
 * is being read right now, and whether this platform can show anything behind
 * the window at all — because that answer spans open codebases and the settings
 * module must not learn about editor tabs.
 *
 * It sits at `src/` root beside `appearanceLogic`, and deliberately **not** in
 * `components/projectViewLogic`: that module's whole premise is deciding one
 * Project tab's shape from one tab's state, and this needs the active root plus
 * a per-root map.
 *
 * The governing rule, and the one every case below is an instance of: **a
 * codebase that has not said whether its editor is empty has not said it is
 * empty.** Unknown resolves to opaque, so the window can only ever become
 * see-through on a positive answer.
 */

/** Fully opaque. The one value that means "the feature is doing nothing". */
export const OPAQUE_PERCENT = 100;

export interface WindowTranslucency {
  /** The user's chosen background opacity, already clamped by `appearanceLogic`. */
  opacity: number;
  /** Whether this platform and build can show anything behind the window. */
  supported: boolean;
  /** The foreground codebase, or `null` for the welcome screen. */
  activeRoot: string | null;
  /**
   * Per open codebase: does its editor area currently hold a tab — a file or a
   * diff?
   *
   * A root **absent from this map has not reported yet**, which is a different
   * fact from reporting `false`. A tab is mounted for a render or two before its
   * effect runs, and a closed codebase's entry is deleted while it may still be
   * `activeRoot` for one render.
   */
  editorTabsByRoot: Record<string, boolean>;
}

/**
 * The percentage to paint the app's background layer at, right now.
 *
 * Only the **active** codebase is consulted. A background one with a file open
 * must not opaque the window, and a background one with nothing open must not
 * make it see-through — the user is looking at neither.
 *
 * Floating panels (terminals, Notes, SQL, Review) are not considered, and not by
 * omission: they paint their own opaque surface and stay perfectly readable over
 * a translucent window, which is the point of the feature rather than a gap in
 * it. They are also not in `openFiles`, so this is true by construction.
 */
export function windowBackgroundOpacity(state: WindowTranslucency): number {
  if (!state.supported) return OPAQUE_PERCENT;
  // A second belt behind `clampWindowOpacity`. Nothing invalid may reach a CSS
  // custom property, where an unparseable value takes the whole declaration with
  // it — and the declaration it would take is the one painting the background.
  if (!Number.isFinite(state.opacity)) return OPAQUE_PERCENT;

  // The welcome screen is the emptiest editor area there is, and it has no
  // editor to open. Making it the one opaque state would mean the app starts
  // opaque, flashes translucent when a folder is opened, and goes opaque again
  // on the first file — three changes to arrive where it should have started.
  if (state.activeRoot === null) return state.opacity;

  const open = state.editorTabsByRoot[state.activeRoot];
  // Not yet reported, or reported and then deleted on unmount. Guessing "empty"
  // here is precisely what produces a translucent flash on startup and on every
  // codebase that is opened.
  if (open === undefined) return OPAQUE_PERCENT;

  return open ? OPAQUE_PERCENT : state.opacity;
}

/** Whether the opacity control can do anything here, and why not when it cannot. */
export interface TransparencySupport {
  supported: boolean;
  /** What to tell the user, or `null` when there is nothing worth saying. */
  reason: string | null;
}

/**
 * Whether this platform can show anything behind the window.
 *
 * The single seam for switching the feature off: if Win11 compositing turns out
 * to paint the transparent region black on some driver, this is the one function
 * to change — the CSS and the predicate above stay as they are.
 *
 * @param os `std::env::consts::OS` as `about_info` reports it, or `null` while
 * that read is still in flight.
 */
export function transparencySupport(os: string | null): TransparencySupport {
  // Not asked yet. Unsupported, but with **no reason**: "we have not looked" is
  // not something to tell a user, so the control renders disabled and silent for
  // the few milliseconds the read takes rather than accusing the platform.
  if (os === null) return { supported: false, reason: null };
  switch (os) {
    case "windows":
      return { supported: true, reason: null };
    case "macos":
      // `transparent: true` needs the private `macos-private-api` feature, which
      // this build does not enable — it blocks App Store distribution, and there
      // is no macOS toolchain here to verify the result on.
      return {
        supported: false,
        reason: "Requires the macOS private API, which this build does not enable.",
      };
    case "linux":
      // Needs a compositing window manager. Without one the region paints
      // garbage rather than the desktop, and that is not detectable from here —
      // so this refuses rather than offering a control that might ruin the
      // window.
      return {
        supported: false,
        reason: "Requires a compositing window manager, which cannot be detected from here.",
      };
    default:
      // Never optimistic about a platform nobody has run this on.
      return {
        supported: false,
        reason: `Window transparency has not been verified on ${os || "this platform"}.`,
      };
  }
}
