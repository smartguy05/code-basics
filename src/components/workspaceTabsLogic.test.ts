import { describe, expect, it } from "vitest";
import {
  addOpenWorkspace,
  closeOpenWorkspace,
  mergeSignal,
  shouldFlashWorkspaceTab,
  tabLabels,
  tabSignalClass,
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

describe("shouldFlashWorkspaceTab", () => {
  it("flashes a background tab whose terminal wants attention", () => {
    expect(shouldFlashWorkspaceTab("/a", "/b", true)).toBe(true);
  });

  it("never flashes the active tab, even with attention pending", () => {
    // The active codebase's terminals are on screen; its own pill flashes there.
    expect(shouldFlashWorkspaceTab("/a", "/a", true)).toBe(false);
  });

  it("does not flash a tab with no attention", () => {
    expect(shouldFlashWorkspaceTab("/a", "/b", false)).toBe(false);
  });

  it("flashes a background tab when nothing is active (defensive)", () => {
    expect(shouldFlashWorkspaceTab("/a", null, true)).toBe(true);
  });
});

describe("mergeSignal", () => {
  it("takes the incoming signal when the tab is showing nothing", () => {
    expect(mergeSignal(null, "done")).toBe("done");
    expect(mergeSignal(undefined, "error")).toBe("error");
  });

  it("never lets a weaker signal mask a stronger one", () => {
    // The case this exists for: a terminal finishing after the build broke must
    // not turn the tab from red to green — the build is still broken.
    expect(mergeSignal("error", "done")).toBe("error");
    expect(mergeSignal("error", "success")).toBe("error");
    expect(mergeSignal("error", "attention")).toBe("error");
    expect(mergeSignal("attention", "success")).toBe("attention");
    expect(mergeSignal("success", "done")).toBe("success");
  });

  it("upgrades to a stronger signal", () => {
    expect(mergeSignal("done", "success")).toBe("success");
    expect(mergeSignal("success", "attention")).toBe("attention");
    expect(mergeSignal("attention", "error")).toBe("error");
  });

  it("keeps the current signal when the same one arrives again", () => {
    expect(mergeSignal("attention", "attention")).toBe("attention");
  });
});

describe("tabSignalClass", () => {
  it("dresses a background tab in its signal's classes", () => {
    expect(tabSignalClass("/a", "/b", "error")).toBe(" signal signal-error");
    expect(tabSignalClass("/a", "/b", "attention")).toBe(" signal signal-attn");
    expect(tabSignalClass("/a", "/b", "success")).toBe(" signal signal-success");
    expect(tabSignalClass("/a", "/b", "done")).toBe(" signal signal-done");
  });

  it("never flashes the active tab", () => {
    // Its terminals flash their own pills and its build output is on screen.
    expect(tabSignalClass("/a", "/a", "error")).toBe("");
  });

  it("is empty with no signal", () => {
    expect(tabSignalClass("/a", "/b", null)).toBe("");
    expect(tabSignalClass("/a", "/b", undefined)).toBe("");
  });
});
