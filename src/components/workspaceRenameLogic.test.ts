import { describe, expect, it } from "vitest";
import {
  clearLabel,
  decodeLabels,
  encodeLabels,
  labelFor,
  loadLabels,
  MAX_LABEL_LENGTH,
  normalizeLabel,
  saveLabels,
  setLabel,
  WORKSPACE_LABELS_KEY,
  type WorkspaceLabels,
} from "./workspaceRenameLogic";

/**
 * A `Storage` stand-in — vitest runs in the node environment, so there is no
 * real `localStorage`. `throwing` reproduces the private-mode / quota case,
 * which is the only reason the load and save wrappers have try/catch at all.
 */
function fakeStorage(initial: Record<string, string> = {}, throwing = false): Storage {
  const map = new Map(Object.entries(initial));
  const boom = () => {
    throw new Error("storage unavailable");
  };
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    key: (i: number) => [...map.keys()][i] ?? null,
    getItem: (k: string) => (throwing ? boom() : (map.get(k) ?? null)),
    setItem: (k: string, v: string) => {
      if (throwing) boom();
      map.set(k, v);
    },
    removeItem: (k: string) => {
      map.delete(k);
    },
  } as Storage;
}

describe("normalizeLabel", () => {
  it("trims surrounding whitespace", () => {
    expect(normalizeLabel("  api  ")).toBe("api");
  });

  it("rejects an empty or whitespace-only name", () => {
    // An empty tab label is unclickable and reads as a bug, so we abstain and
    // let the caller keep the scanned name.
    expect(normalizeLabel("")).toBeNull();
    expect(normalizeLabel("   ")).toBeNull();
    expect(normalizeLabel("\t\n")).toBeNull();
  });

  it("caps the length so a pasted path or paragraph cannot become a label", () => {
    const long = "x".repeat(MAX_LABEL_LENGTH + 25);
    expect(normalizeLabel(long)).toHaveLength(MAX_LABEL_LENGTH);
  });

  it("leaves a name at exactly the cap untouched", () => {
    const exact = "y".repeat(MAX_LABEL_LENGTH);
    expect(normalizeLabel(exact)).toBe(exact);
  });

  it("trims before capping, so trailing spaces do not eat the budget", () => {
    const padded = `${"z".repeat(MAX_LABEL_LENGTH)}          `;
    expect(normalizeLabel(padded)).toHaveLength(MAX_LABEL_LENGTH);
  });
});

describe("encodeLabels / decodeLabels", () => {
  it("round-trips a map", () => {
    const labels: WorkspaceLabels = { "/a": "Alpha", "/b": "Beta" };
    expect(decodeLabels(encodeLabels(labels))).toEqual(labels);
  });

  it("sorts keys so an unchanged map writes an unchanged string", () => {
    expect(encodeLabels({ "/b": "Beta", "/a": "Alpha" })).toBe(
      encodeLabels({ "/a": "Alpha", "/b": "Beta" }),
    );
  });

  it("yields an empty map for a missing value", () => {
    expect(decodeLabels(null)).toEqual({});
  });

  it("yields an empty map for unparseable or wrongly-shaped stored data", () => {
    // The tolerance rule: a corrupt preference must never stop the strip
    // drawing, and the scanned names are always a correct answer.
    expect(decodeLabels("{not json")).toEqual({});
    expect(decodeLabels("[]")).toEqual({});
    expect(decodeLabels("null")).toEqual({});
    expect(decodeLabels('"a string"')).toEqual({});
    expect(decodeLabels("7")).toEqual({});
  });

  it("drops entries whose value is not a string", () => {
    expect(decodeLabels('{"/a":"Alpha","/b":3,"/c":null,"/d":{"x":1}}')).toEqual({ "/a": "Alpha" });
  });

  it("re-validates values on the way in", () => {
    // A hand-edited or older-build entry cannot put a blank or unbounded
    // string on a tab.
    const raw = JSON.stringify({ "/a": "   ", "/b": "w".repeat(MAX_LABEL_LENGTH + 10), "/c": " ok " });
    const decoded = decodeLabels(raw);
    expect(decoded["/a"]).toBeUndefined();
    expect(decoded["/b"]).toHaveLength(MAX_LABEL_LENGTH);
    expect(decoded["/c"]).toBe("ok");
  });

  it("drops an empty root key", () => {
    expect(decodeLabels('{"":"nowhere","/a":"Alpha"}')).toEqual({ "/a": "Alpha" });
  });

  it("treats a stored __proto__ key as ordinary data, not a prototype", () => {
    const decoded = decodeLabels('{"__proto__":"pwned","/a":"Alpha"}');
    expect(decoded["/a"]).toBe("Alpha");
    expect(Object.getPrototypeOf(decoded)).toBe(Object.prototype);
    expect(Object.prototype.hasOwnProperty.call(decoded, "__proto__")).toBe(true);
  });
});

describe("loadLabels / saveLabels", () => {
  it("round-trips through a storage", () => {
    const storage = fakeStorage();
    saveLabels(storage, { "/a": "Alpha" });
    expect(storage.getItem(WORKSPACE_LABELS_KEY)).toBe('{"/a":"Alpha"}');
    expect(loadLabels(storage)).toEqual({ "/a": "Alpha" });
  });

  it("loads an empty map when nothing was stored", () => {
    expect(loadLabels(fakeStorage())).toEqual({});
  });

  it("survives a storage that throws", () => {
    const storage = fakeStorage({}, true);
    expect(loadLabels(storage)).toEqual({});
    expect(() => saveLabels(storage, { "/a": "Alpha" })).not.toThrow();
  });
});

describe("labelFor", () => {
  it("prefers the user's name", () => {
    expect(labelFor({ "/a": "Alpha" }, "/a", "a")).toBe("Alpha");
  });

  it("falls back to the derived name when there is no rename", () => {
    expect(labelFor({}, "/a", "a")).toBe("a");
  });

  it("ignores an inherited Object member rather than labelling a tab with it", () => {
    expect(labelFor({} as WorkspaceLabels, "toString", "derived")).toBe("derived");
  });
});

describe("setLabel", () => {
  it("records a normalized rename without mutating the map", () => {
    const before: WorkspaceLabels = { "/a": "Alpha" };
    const after = setLabel(before, "/b", "  Beta  ");
    expect(after).toEqual({ "/a": "Alpha", "/b": "Beta" });
    expect(before).toEqual({ "/a": "Alpha" });
  });

  it("replaces an existing rename", () => {
    expect(setLabel({ "/a": "Alpha" }, "/a", "Second")).toEqual({ "/a": "Second" });
  });

  it("refuses an unusable name so the caller keeps the scanned one", () => {
    expect(setLabel({ "/a": "Alpha" }, "/a", "   ")).toBeNull();
    expect(setLabel({}, "/a", "")).toBeNull();
  });

  it("stores a rename that equals the scanned name, pinning the tab to it", () => {
    // Deliberate: a later rescan or a colliding second codebase must not move
    // a label the user explicitly chose. `clearLabel` is the way back.
    expect(setLabel({}, "/x/api", "api")).toEqual({ "/x/api": "api" });
  });
});

describe("clearLabel", () => {
  it("drops the rename, returning the tab to its scanned name", () => {
    const after = clearLabel({ "/a": "Alpha", "/b": "Beta" }, "/a");
    expect(after).toEqual({ "/b": "Beta" });
    expect(labelFor(after, "/a", "a")).toBe("a");
  });

  it("does not mutate the map it was given", () => {
    const before: WorkspaceLabels = { "/a": "Alpha" };
    clearLabel(before, "/a");
    expect(before).toEqual({ "/a": "Alpha" });
  });

  it("returns the same map when there was nothing to clear", () => {
    // Identity is the cheap signal React uses to skip a render.
    const before: WorkspaceLabels = { "/a": "Alpha" };
    expect(clearLabel(before, "/nope")).toBe(before);
  });
});

describe("normalizeLabel rejects what cannot be read on a tab", () => {
  it("caps by code point, not by code unit", () => {
    // slice() cuts UTF-16 units, so a name whose last kept unit is half a
    // surrogate pair is stored as a lone surrogate: it draws as U+FFFD, and
    // JSON.stringify escapes rather than rejects it, so the corruption survives
    // every reload.
    const emoji = "\ud83c\udf89";
    const label = normalizeLabel("a".repeat(MAX_LABEL_LENGTH - 1) + emoji);
    expect(label).not.toBeNull();
    const kept = Array.from(label as string);
    expect(kept.length).toBe(MAX_LABEL_LENGTH);
    expect(kept[kept.length - 1]).toBe(emoji);
    // No half-character left at the end.
    const last = (label as string).charCodeAt((label as string).length - 1);
    expect(last >= 0xd800 && last <= 0xdbff).toBe(false);
  });

  it("collapses the newlines a pasted paragraph carries", () => {
    expect(normalizeLabel("  billing\nservice  ")).toBe("billing service");
  });

  it("strips a bidi override rather than letting it reverse the tab", () => {
    // U+202E makes the text after it draw right-to-left, so this reads as
    // "apiexe.png" on the tab strip.
    const label = normalizeLabel("api\u202egnp.exe");
    expect(label).not.toBeNull();
    expect(label).not.toContain("\u202e");
  });

  it("strips zero-width characters, which are invisible in the box too", () => {
    expect(normalizeLabel("bil\u200bling")).toBe("bil ling");
  });

  it("refuses a name made only of characters that render as nothing", () => {
    // Not a name: storing it would put an apparently blank, unclickable label
    // on the tab, which is the case the null return exists for.
    expect(normalizeLabel("\u200b\u202e\t")).toBeNull();
  });

  it("round-trips a capped label through storage unchanged", () => {
    // The cap runs again inside decodeLabels, so a value it mangled would be
    // re-mangled on every launch rather than settling.
    const once = normalizeLabel("x".repeat(80) + "\ud83c\udf89") as string;
    expect(decodeLabels(encodeLabels({ "/a": once }))["/a"]).toBe(once);
  });
});
