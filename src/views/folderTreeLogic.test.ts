import { describe, expect, it } from "vitest";
import { buildFileTree, flattenFileTree } from "./folderTreeLogic";
import type { FileChange } from "../ipc/types";

/** A minimal changed file; only `path` matters to the tree. */
function change(path: string): FileChange {
  return { path, oldPath: null, staged: null, unstaged: "modified", isBinary: false };
}

describe("buildFileTree", () => {
  it("nests files under their folder segments", () => {
    const root = buildFileTree([
      change("src/views/ChangesView.tsx"),
      change("src/views/changesLogic.ts"),
      change("README.md"),
    ]);

    // A top-level file sits directly on the root.
    expect(root.files.map((f) => f.label)).toEqual(["README.md"]);

    const src = root.folders.find((f) => f.path === "src");
    expect(src).toBeDefined();
    const views = src?.folders.find((f) => f.path === "src/views");
    expect(views?.files.map((f) => f.label).sort()).toEqual([
      "ChangesView.tsx",
      "changesLogic.ts",
    ]);
    // The folder label is the last segment, not the full path.
    expect(views?.label).toBe("views");
  });

  it("counts every file beneath each folder, recursively", () => {
    const root = buildFileTree([
      change("src/a/one.ts"),
      change("src/a/two.ts"),
      change("src/b/three.ts"),
      change("top.ts"),
    ]);

    expect(root.fileCount).toBe(4);
    expect(root.folders.find((f) => f.path === "src")?.fileCount).toBe(3);
    expect(
      root.folders
        .find((f) => f.path === "src")
        ?.folders.find((f) => f.path === "src/a")?.fileCount,
    ).toBe(2);
  });

  it("reuses a folder node shared by several files", () => {
    const root = buildFileTree([change("src/x.ts"), change("src/y.ts")]);
    expect(root.folders).toHaveLength(1);
    expect(root.folders[0]?.files).toHaveLength(2);
  });
});

describe("flattenFileTree", () => {
  const root = buildFileTree([
    change("src/views/ChangesView.tsx"),
    change("src/api.ts"),
    change("README.md"),
  ]);

  it("emits folders before files at each level, each sorted alphabetically", () => {
    const rows = flattenFileTree(root, () => false);
    expect(rows.map((r) => (r.kind === "folder" ? `[${r.label}]` : r.label))).toEqual([
      "[src]",
      "[views]",
      "ChangesView.tsx",
      "api.ts",
      "README.md",
    ]);
  });

  it("carries the nesting depth for indentation", () => {
    const rows = flattenFileTree(root, () => false);
    const src = rows.find((r) => r.kind === "folder" && r.label === "src");
    const views = rows.find((r) => r.kind === "folder" && r.label === "views");
    const changesView = rows.find((r) => r.kind === "file" && r.label === "ChangesView.tsx");
    expect(src?.depth).toBe(0);
    expect(views?.depth).toBe(1);
    expect(changesView?.depth).toBe(2);
  });

  it("hides the descendants of a collapsed folder but still emits the folder", () => {
    const rows = flattenFileTree(root, (path) => path === "src");
    const labels = rows.map((r) => (r.kind === "folder" ? `[${r.label}]` : r.label));
    // "src" is present and marked collapsed; nothing beneath it is emitted.
    expect(labels).toEqual(["[src]", "README.md"]);
    const src = rows.find((r) => r.kind === "folder" && r.label === "src");
    expect(src?.kind === "folder" && src.collapsed).toBe(true);
  });
});
