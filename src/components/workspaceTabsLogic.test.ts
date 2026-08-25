import { describe, expect, it } from "vitest";
import {
  addOpenWorkspace,
  closeOpenWorkspace,
  tabLabels,
} from "./workspaceTabsLogic";
import type { Workspace } from "../ipc/types";

/** A minimal Workspace stub — only `root` and `name` matter to these helpers. */
function ws(root: string, name?: string): Workspace {
  return {
    root,
    name: name ?? root.split(/[\\/]/).filter(Boolean).pop() ?? root,
    projects: [],
    configs: [],
    solutions: [],
    favorites: [],
    order: [],
  };
}

describe("addOpenWorkspace", () => {
  it("appends a new workspace and makes it active", () => {
    const a = ws("/a");
    const b = ws("/b");
    const { list, activeRoot } = addOpenWorkspace([a], b);
    expect(list.map((w) => w.root)).toEqual(["/a", "/b"]);
    expect(activeRoot).toBe("/b");
  });

  it("opening an already-open folder focuses it rather than duplicating", () => {
    const a = ws("/a");
    const b = ws("/b");
    // Re-opening /a returns a fresh object (a rescan) — it must replace, not append.
    const reopened = ws("/a", "a-rescanned");
    const { list, activeRoot } = addOpenWorkspace([a, b], reopened);
    expect(list.map((w) => w.root)).toEqual(["/a", "/b"]);
    expect(list[0]?.name).toBe("a-rescanned"); // replaced in place
    expect(activeRoot).toBe("/a");
  });

  it("adding to an empty list makes the first tab active", () => {
    const { list, activeRoot } = addOpenWorkspace([], ws("/only"));
    expect(list.map((w) => w.root)).toEqual(["/only"]);
    expect(activeRoot).toBe("/only");
  });
});

describe("closeOpenWorkspace", () => {
  const a = ws("/a");
  const b = ws("/b");
  const c = ws("/c");

  it("closing the active tab activates the neighbour that slid into its slot", () => {
    const { list, activeRoot } = closeOpenWorkspace([a, b, c], "/b", "/b");
    expect(list.map((w) => w.root)).toEqual(["/a", "/c"]);
    expect(activeRoot).toBe("/c"); // the note-panel rule: note now at the deleted index
  });

  it("closing the active last tab activates the new last tab", () => {
    const { list, activeRoot } = closeOpenWorkspace([a, b, c], "/c", "/c");
    expect(list.map((w) => w.root)).toEqual(["/a", "/b"]);
    expect(activeRoot).toBe("/b");
  });

  it("closing a background tab leaves the active tab alone", () => {
    const { list, activeRoot } = closeOpenWorkspace([a, b, c], "/a", "/c");
    expect(list.map((w) => w.root)).toEqual(["/b", "/c"]);
    expect(activeRoot).toBe("/c");
  });

  it("closing the last remaining tab yields no active tab (welcome screen)", () => {
    const { list, activeRoot } = closeOpenWorkspace([a], "/a", "/a");
    expect(list).toEqual([]);
    expect(activeRoot).toBeNull();
  });
});

describe("tabLabels", () => {
  it("uses the bare name when names are unique", () => {
    const labels = tabLabels([ws("/x/api", "api"), ws("/y/web", "web")]);
    expect(labels).toEqual(["api", "web"]);
  });

  it("disambiguates duplicate names with a trailing path segment", () => {
    const labels = tabLabels([ws("/one/api", "api"), ws("/two/api", "api")]);
    expect(labels[0]).not.toBe(labels[1]);
    expect(labels[0]).toContain("api");
    expect(labels[1]).toContain("api");
    // The disambiguator is the parent directory the roots differ by.
    expect(labels[0]).toContain("one");
    expect(labels[1]).toContain("two");
  });

  it("leaves a single workspace's name untouched", () => {
    expect(tabLabels([ws("/x/api", "api")])).toEqual(["api"]);
  });
});
