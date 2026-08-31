import { describe, expect, it } from "vitest";
import {
  COLLAPSED_KEY_PREFIX,
  DEFAULT_SPLIT,
  LEGACY_SPLIT_KEY,
  SPLIT_KEY_PREFIX,
  clampSplit,
  collapsedKey,
  loadCollapsed,
  loadSplit,
  saveCollapsed,
  saveSplit,
  splitKey,
  isBuildSession,
  shouldCloseBuildSession,
  shouldForceExpand,
} from "./consolePanelLogic";

/** A `Storage` slice backed by a plain map, so nothing here needs a browser. */
function memory(initial: Record<string, string> = {}) {
  const store = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
    read: (key: string) => store.get(key) ?? null,
    size: () => store.size,
  };
}

/** A storage that fails the way a full or disabled one does. */
const hostile = {
  getItem() {
    throw new Error("storage is not available");
  },
  setItem() {
    throw new Error("quota exceeded");
  },
};

const ROOT = "C:/Users/dev/repo";

describe("the storage keys", () => {
  it("names the workspace in the key so two workspaces cannot share a state", () => {
    expect(collapsedKey(ROOT)).not.toEqual(collapsedKey("C:/Users/dev/other"));
    expect(splitKey(ROOT)).not.toEqual(splitKey("C:/Users/dev/other"));
  });

  it("carries the documented prefixes, so a sweep could find every entry", () => {
    expect(collapsedKey(ROOT).startsWith(`${COLLAPSED_KEY_PREFIX}:`)).toBe(true);
    expect(splitKey(ROOT).startsWith(`${SPLIT_KEY_PREFIX}:`)).toBe(true);
  });

  it("encodes the root, so a root containing the separator cannot move the boundary", () => {
    // A Windows root always contains a colon, which is also the separator.
    expect(collapsedKey(ROOT)).toBe(`${COLLAPSED_KEY_PREFIX}:${encodeURIComponent(ROOT)}`);
    // The pathological case the encoding exists for: two different roots that a
    // plain join would map onto one key.
    expect(collapsedKey("a:b")).not.toEqual(collapsedKey("a%3Ab"));
  });

  it("keeps the split key distinct from the legacy global one it falls back to", () => {
    expect(splitKey(ROOT)).not.toBe(LEGACY_SPLIT_KEY);
  });
});

describe("loadCollapsed", () => {
  it("starts expanded, because a first-run pane the user cannot see is a bug report", () => {
    expect(loadCollapsed(memory(), ROOT)).toBe(false);
  });

  it("reads back what was stored, per workspace", () => {
    const storage = memory();
    saveCollapsed(storage, ROOT, true);
    expect(loadCollapsed(storage, ROOT)).toBe(true);
    // The other workspace was never told, so it keeps the default.
    expect(loadCollapsed(storage, "C:/Users/dev/other")).toBe(false);
  });

  it("round-trips false as well, rather than treating it as absent", () => {
    const storage = memory();
    saveCollapsed(storage, ROOT, true);
    saveCollapsed(storage, ROOT, false);
    expect(loadCollapsed(storage, ROOT)).toBe(false);
  });

  it("declines anything that is not the exact stored spelling", () => {
    // localStorage is editable by hand and survives older builds of this app.
    for (const raw of ["", "yes", "1", "TRUE", "null", "{}"]) {
      expect(loadCollapsed(memory({ [collapsedKey(ROOT)]: raw }), ROOT)).toBe(false);
    }
  });

  it("expands rather than throwing when storage cannot be read", () => {
    expect(loadCollapsed(hostile, ROOT)).toBe(false);
  });
});

describe("saveCollapsed", () => {
  it("does not throw when the quota is exhausted", () => {
    expect(() => saveCollapsed(hostile, ROOT, true)).not.toThrow();
  });
});

describe("clampSplit", () => {
  it("keeps a usable fraction untouched", () => {
    expect(clampSplit(0.5)).toBe(0.5);
  });

  it("holds both panes visible at the extremes", () => {
    expect(clampSplit(0)).toBe(0.1);
    expect(clampSplit(1)).toBe(0.9);
    expect(clampSplit(-4)).toBe(0.1);
    expect(clampSplit(1000)).toBe(0.9);
  });

  it("refuses a non-number by falling back to the default", () => {
    // `Math.min(Math.max(NaN, …), …)` is NaN, and a NaN flex-basis silently
    // collapses the editor pane to nothing.
    expect(clampSplit(NaN)).toBe(DEFAULT_SPLIT);
    expect(clampSplit(Infinity)).toBe(0.9);
    expect(clampSplit(-Infinity)).toBe(0.1);
  });
});

describe("loadSplit", () => {
  it("defaults when nothing has been stored", () => {
    expect(loadSplit(memory(), ROOT)).toBe(DEFAULT_SPLIT);
  });

  it("reads back what was stored, per workspace", () => {
    const storage = memory();
    saveSplit(storage, ROOT, 0.3);
    expect(loadSplit(storage, ROOT)).toBeCloseTo(0.3);
    expect(loadSplit(storage, "C:/Users/dev/other")).toBe(DEFAULT_SPLIT);
  });

  it("falls back to the pre-existing global key, so nobody's divider jumps once", () => {
    const storage = memory({ [LEGACY_SPLIT_KEY]: "0.25" });
    expect(loadSplit(storage, ROOT)).toBeCloseTo(0.25);
  });

  it("prefers the per-workspace value over the legacy global one", () => {
    const storage = memory({ [LEGACY_SPLIT_KEY]: "0.25", [splitKey(ROOT)]: "0.7" });
    expect(loadSplit(storage, ROOT)).toBeCloseTo(0.7);
  });

  it("clamps a stored value from an older build instead of refusing it", () => {
    expect(loadSplit(memory({ [splitKey(ROOT)]: "0.99" }), ROOT)).toBe(0.9);
    expect(loadSplit(memory({ [splitKey(ROOT)]: "0.01" }), ROOT)).toBe(0.1);
  });

  it("declines a value that is not a number at all", () => {
    for (const raw of ["", "abc", "null", "{}"]) {
      expect(loadSplit(memory({ [splitKey(ROOT)]: raw }), ROOT)).toBe(DEFAULT_SPLIT);
    }
  });

  it("defaults rather than throwing when storage cannot be read", () => {
    expect(loadSplit(hostile, ROOT)).toBe(DEFAULT_SPLIT);
  });
});

describe("saveSplit", () => {
  it("stores the clamped fraction, so an unusable one can never be read back", () => {
    const storage = memory();
    saveSplit(storage, ROOT, 3);
    expect(Number(storage.read(splitKey(ROOT)))).toBe(0.9);
  });

  it("writes nothing at all for a non-number", () => {
    const storage = memory();
    saveSplit(storage, ROOT, NaN);
    expect(storage.size()).toBe(0);
  });

  it("does not throw when the quota is exhausted", () => {
    expect(() => saveSplit(hostile, ROOT, 0.5)).not.toThrow();
  });
});

describe("shouldForceExpand", () => {
  it("forces the panel open when the last file closes while it is collapsed", () => {
    // The trap: the collapse control is only rendered with a file open, so a
    // collapsed panel with no files is hidden and has no way back.
    expect(shouldForceExpand(0, true)).toBe(true);
  });

  it("leaves an expanded panel alone", () => {
    expect(shouldForceExpand(0, false)).toBe(false);
  });

  it("leaves a collapsed panel alone while a file is open", () => {
    // There the toggle is on screen, so collapsed is a state the user can leave.
    expect(shouldForceExpand(1, true)).toBe(false);
    expect(shouldForceExpand(5, true)).toBe(false);
  });
});

describe("isBuildSession", () => {
  it("recognises the id RunView mints for a build", () => {
    expect(isBuildSession("MyApi:build")).toBe(true);
    expect(isBuildSession("MyApi")).toBe(false);
    expect(isBuildSession("build")).toBe(false);
    expect(isBuildSession("MyApi:build:extra")).toBe(false);
  });
});

describe("shouldCloseBuildSession", () => {
  it("closes a build that succeeded", () => {
    expect(shouldCloseBuildSession("MyApi:build", true, false)).toBe(true);
  });

  it("keeps a failed build's tab — the errors are why it ran", () => {
    expect(shouldCloseBuildSession("MyApi:build", false, false)).toBe(false);
  });

  it("keeps a cancelled build's tab", () => {
    // Stopping a build is the user reporting something, not the build.
    expect(shouldCloseBuildSession("MyApi:build", false, true)).toBe(false);
    expect(shouldCloseBuildSession("MyApi:build", true, true)).toBe(false);
  });

  it("never closes a run session, however it ended", () => {
    expect(shouldCloseBuildSession("MyApi", true, false)).toBe(false);
  });
});
