import { describe, expect, it } from "vitest";
import {
  OPAQUE_PERCENT,
  transparencySupport,
  windowBackgroundOpacity,
  type WindowTranslucency,
} from "./windowTransparencyLogic";

/** A translucent-capable state with one codebase open and nothing in its editor. */
const base: WindowTranslucency = {
  opacity: 60,
  supported: true,
  activeRoot: "C:/code/a",
  editorTabsByRoot: { "C:/code/a": false },
};

describe("windowBackgroundOpacity", () => {
  it("is indistinguishable from the feature being off at 100", () => {
    // The hard guarantee, and the reason for the exhaustive form: a user who has
    // not touched the slider must get *exactly* today's window in every state
    // the app can be in, not one that merely looks the same in the common case.
    for (const supported of [true, false]) {
      for (const activeRoot of [null, "C:/code/a", "C:/code/gone"]) {
        const maps: Record<string, boolean>[] = [{}, { "C:/code/a": true }, { "C:/code/a": false }];
        for (const editorTabsByRoot of maps) {
          expect(
            windowBackgroundOpacity({
              opacity: OPAQUE_PERCENT,
              supported,
              activeRoot,
              editorTabsByRoot,
            }),
          ).toBe(OPAQUE_PERCENT);
        }
      }
    }
  });

  it("is translucent with a codebase open and nothing in its editor", () => {
    expect(windowBackgroundOpacity(base)).toBe(60);
  });

  it("is opaque the moment the active codebase shows a tab", () => {
    expect(
      windowBackgroundOpacity({ ...base, editorTabsByRoot: { "C:/code/a": true } }),
    ).toBe(OPAQUE_PERCENT);
  });

  it("is opaque on the welcome screen", () => {
    // No codebase is open, so there is nothing whose editor could report empty.
    // Translucency begins only once an OPEN codebase reports an empty editor;
    // the welcome screen stays fully opaque so the app does not start
    // see-through before any folder is opened.
    expect(windowBackgroundOpacity({ ...base, activeRoot: null, editorTabsByRoot: {} })).toBe(
      OPAQUE_PERCENT,
    );
  });

  it("is opaque for a codebase that has not reported its editor yet", () => {
    // The abstain case. A tab is mounted for a render or two before its effect
    // runs, and "no entry" is not "no tabs" — guessing empty here is exactly
    // what makes the window flash translucent on startup and on every open.
    expect(windowBackgroundOpacity({ ...base, editorTabsByRoot: {} })).toBe(OPAQUE_PERCENT);
  });

  it("consults only the active codebase", () => {
    // One map, two answers. A background codebase with a file open must not
    // opaque the window, and a background one with nothing open must not make
    // it translucent.
    const editorTabsByRoot = { "C:/code/a": false, "C:/code/b": true };
    expect(windowBackgroundOpacity({ ...base, activeRoot: "C:/code/a", editorTabsByRoot })).toBe(60);
    expect(
      windowBackgroundOpacity({ ...base, activeRoot: "C:/code/b", editorTabsByRoot }),
    ).toBe(OPAQUE_PERCENT);
  });

  it("is opaque for a codebase closed while it was still the active one", () => {
    // Its entry is deleted on unmount, so for a render the active root names
    // nothing. Opaque is the safe reading of that, and it is the same rule as
    // the not-yet-reported case rather than a second one.
    expect(
      windowBackgroundOpacity({ ...base, editorTabsByRoot: { "C:/code/b": false } }),
    ).toBe(OPAQUE_PERCENT);
  });

  it("is opaque at every slider position on a platform that cannot do it", () => {
    for (const opacity of [30, 55, 99]) {
      expect(windowBackgroundOpacity({ ...base, supported: false, opacity })).toBe(OPAQUE_PERCENT);
    }
  });

  it("never lets a non-finite opacity reach the window", () => {
    // A second belt behind `clampWindowOpacity`: nothing invalid may be written
    // into a CSS custom property, where it would take the whole declaration
    // with it.
    for (const opacity of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
      expect(windowBackgroundOpacity({ ...base, opacity })).toBe(OPAQUE_PERCENT);
    }
  });
});

describe("transparencySupport", () => {
  it("abstains, with nothing to say, before the platform is known", () => {
    // "We have not asked yet" is not a reason to show a user. The control is
    // disabled and silent for the few milliseconds the read takes.
    expect(transparencySupport(null)).toEqual({ supported: false, reason: null });
  });

  it("supports Windows", () => {
    expect(transparencySupport("windows").supported).toBe(true);
  });

  it("refuses every other platform with a reason rather than optimistically", () => {
    for (const os of ["macos", "linux", "freebsd", ""]) {
      const support = transparencySupport(os);
      expect(support.supported).toBe(false);
      expect(support.reason === null).toBe(false);
      expect((support.reason ?? "").length > 0).toBe(true);
    }
  });
});
