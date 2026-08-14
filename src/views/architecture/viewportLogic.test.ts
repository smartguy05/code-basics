import { describe, expect, it } from "vitest";
import { MAX_SCALE, MIN_SCALE } from "./panZoomLogic";
import {
  loadViewport,
  saveViewport,
  viewportKey,
  VIEWPORT_KEY_PREFIX,
} from "./viewportLogic";

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

describe("viewportKey", () => {
  it("is stable for the same workspace and diagram", () => {
    expect(viewportKey("C:/repo", "builtin:project")).toBe(
      viewportKey("C:/repo", "builtin:project"),
    );
  });

  it("separates diagrams within one workspace", () => {
    expect(viewportKey("C:/repo", "builtin:project")).not.toBe(
      viewportKey("C:/repo", "builtin:component"),
    );
  });

  it("separates workspaces holding the same diagram", () => {
    expect(viewportKey("C:/a", "builtin:project")).not.toBe(
      viewportKey("C:/b", "builtin:project"),
    );
  });

  it("cannot be made to collide by moving the boundary between its parts", () => {
    // The failure this rules out: a plain `root + sep + id` join lets a root
    // ending in the separator produce the same key as a different root with a
    // longer id, so one diagram silently opens at another's pan and zoom.
    expect(viewportKey("a:b", "c")).not.toBe(viewportKey("a", "b:c"));
  });

  it("stays inside its own namespace", () => {
    expect(viewportKey("C:/repo", "builtin:project").startsWith(VIEWPORT_KEY_PREFIX)).toBe(
      true,
    );
  });
});

describe("saveViewport / loadViewport", () => {
  it("round-trips a view", () => {
    const storage = memory();
    const key = viewportKey("C:/repo", "builtin:project");
    saveViewport(storage, key, { x: 12.5, y: -30, k: 1.75 });
    expect(loadViewport(storage, key)).toEqual({ x: 12.5, y: -30, k: 1.75 });
  });

  it("keeps each diagram's view apart", () => {
    const storage = memory();
    const project = viewportKey("C:/repo", "builtin:project");
    const component = viewportKey("C:/repo", "builtin:component");
    saveViewport(storage, project, { x: 1, y: 2, k: 1 });
    saveViewport(storage, component, { x: 3, y: 4, k: 2 });
    expect(loadViewport(storage, project)).toEqual({ x: 1, y: 2, k: 1 });
    expect(loadViewport(storage, component)).toEqual({ x: 3, y: 4, k: 2 });
  });

  it("has nothing to say about a diagram never opened", () => {
    expect(loadViewport(memory(), viewportKey("C:/repo", "builtin:project"))).toBeNull();
  });

  it("declines a stored value that is not JSON", () => {
    const storage = memory({ k: "{not json" });
    expect(loadViewport(storage, "k")).toBeNull();
  });

  it.each([
    ["a bare number", "3"],
    ["null", "null"],
    ["an array", "[1,2,3]"],
    ["a string", '"1,2,3"'],
  ])("declines %s", (_name, raw) => {
    expect(loadViewport(memory({ k: raw }), "k")).toBeNull();
  });

  it.each([
    ["a missing field", '{"x":1,"y":2}'],
    ["a field that is not a number", '{"x":1,"y":2,"k":"1"}'],
    ["a NaN", '{"x":1,"y":null,"k":1}'],
  ])("declines %s rather than transforming by it", (_name, raw) => {
    // A non-finite number in a transform does not throw: the diagram vanishes
    // with no error anywhere. Declining shows it un-panned instead.
    expect(loadViewport(memory({ k: raw }), "k")).toBeNull();
  });

  it("declines a scale of zero or less", () => {
    expect(loadViewport(memory({ k: '{"x":0,"y":0,"k":0}' }), "k")).toBeNull();
    expect(loadViewport(memory({ k: '{"x":0,"y":0,"k":-2}' }), "k")).toBeNull();
  });

  it("holds a stored scale inside the range the canvas can zoom to", () => {
    // Storage is editable by hand and survives a version of this app that had
    // different limits; a restored view outside them cannot be zoomed back.
    expect(loadViewport(memory({ k: '{"x":0,"y":0,"k":9999}' }), "k")?.k).toBe(MAX_SCALE);
    expect(loadViewport(memory({ k: '{"x":0,"y":0,"k":0.0001}' }), "k")?.k).toBe(MIN_SCALE);
  });

  it("ignores extra fields rather than carrying them into the view", () => {
    expect(loadViewport(memory({ k: '{"x":1,"y":2,"k":1,"scale":9}' }), "k")).toEqual({
      x: 1,
      y: 2,
      k: 1,
    });
  });

  it("survives a storage that refuses to be read", () => {
    expect(loadViewport(hostile, "k")).toBeNull();
  });

  it("survives a storage that refuses to be written", () => {
    // Losing a pan position is not worth an exception through a pointer move
    // handler, which would take the whole diagram down with it.
    expect(() => saveViewport(hostile, "k", { x: 1, y: 2, k: 1 })).not.toThrow();
  });

  it("declines to store a view it could never load back", () => {
    const storage = memory();
    saveViewport(storage, "k", { x: Number.NaN, y: 0, k: 1 });
    expect(storage.size()).toBe(0);
  });
});
