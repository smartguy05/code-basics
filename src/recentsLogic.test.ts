import { describe, expect, it } from "vitest";
import { RECENTS_KEY, loadRecents, rememberRecent, type RecentsStorage } from "./recentsLogic";

/** Minimal in-memory stand-in for the two `Storage` methods recents uses. */
function stubStorage(initial: Record<string, string> = {}): RecentsStorage & {
  map: Map<string, string>;
} {
  const map = new Map(Object.entries(initial));
  return {
    map,
    getItem: (key: string) => (map.has(key) ? (map.get(key) as string) : null),
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

describe("loadRecents", () => {
  it("returns an empty list when nothing has been stored", () => {
    expect(loadRecents(stubStorage())).toEqual([]);
  });

  it("returns an empty list for an empty stored string", () => {
    expect(loadRecents(stubStorage({ [RECENTS_KEY]: "" }))).toEqual([]);
  });

  it("reads back a stored list", () => {
    const storage = stubStorage({ [RECENTS_KEY]: JSON.stringify(["/a", "/b"]) });
    expect(loadRecents(storage)).toEqual(["/a", "/b"]);
  });

  it("tolerates corrupt JSON rather than throwing", () => {
    const storage = stubStorage({ [RECENTS_KEY]: "{not json" });
    expect(loadRecents(storage)).toEqual([]);
  });

  it("tolerates a storage that throws on read", () => {
    const storage: RecentsStorage = {
      getItem: () => {
        throw new Error("access denied");
      },
      setItem: () => {},
    };
    expect(loadRecents(storage)).toEqual([]);
  });
});

describe("rememberRecent", () => {
  it("stores under the pinned key name", () => {
    const storage = stubStorage();
    rememberRecent(storage, "/w");
    expect(RECENTS_KEY).toBe("code-basics.recentWorkspaces");
    expect(storage.map.get(RECENTS_KEY)).toBe(JSON.stringify(["/w"]));
  });

  it("puts the newest path first", () => {
    const storage = stubStorage({ [RECENTS_KEY]: JSON.stringify(["/a", "/b"]) });
    rememberRecent(storage, "/c");
    expect(loadRecents(storage)).toEqual(["/c", "/a", "/b"]);
  });

  it("dedups an existing path and moves it to the front", () => {
    const storage = stubStorage({ [RECENTS_KEY]: JSON.stringify(["/a", "/b", "/c"]) });
    rememberRecent(storage, "/c");
    expect(loadRecents(storage)).toEqual(["/c", "/a", "/b"]);
  });

  it("caps the list at 8 entries, dropping the oldest", () => {
    const storage = stubStorage();
    for (let i = 1; i <= 10; i++) rememberRecent(storage, `/p${i}`);
    expect(loadRecents(storage)).toEqual([
      "/p10",
      "/p9",
      "/p8",
      "/p7",
      "/p6",
      "/p5",
      "/p4",
      "/p3",
    ]);
  });

  it("starts a fresh list when the stored value is corrupt", () => {
    const storage = stubStorage({ [RECENTS_KEY]: "]]]" });
    rememberRecent(storage, "/a");
    expect(loadRecents(storage)).toEqual(["/a"]);
  });
});
