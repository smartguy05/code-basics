import type { FileChange } from "../ipc/types";

/**
 * A folder-tree view of a flat list of changed files, so the Files sidebar can
 * offer the changes as a collapsible directory structure instead of one long
 * list of full paths.
 *
 * Deliberately pure and separate from the section split (`changesLogic`): the
 * tree is built *per section* (Staged, each group, Unstaged) over that section's
 * own files, so the git partition — which is a fact — is never reshaped by the
 * display choice.
 */

/** One folder in the tree. `path` is the full slash path and is unique. */
export interface FileTreeFolder {
  /** Full segment path, e.g. `src/views`. `""` is the (unrendered) root. */
  path: string;
  /** The last segment, e.g. `views` — what the folder row shows. */
  label: string;
  folders: FileTreeFolder[];
  files: { change: FileChange; label: string }[];
  /** Total files anywhere beneath this folder, for the folder-row badge. */
  fileCount: number;
}

/**
 * Build the folder tree for one section's files.
 *
 * A file's path is split on `/`; every segment but the last is a folder, the
 * last is the leaf. Folders shared by several files are the same node (looked
 * up by full path), and each folder on a file's path has its `fileCount`
 * incremented, so a folder badge counts everything under it recursively.
 */
export function buildFileTree(files: FileChange[]): FileTreeFolder {
  const root: FileTreeFolder = { path: "", label: "", folders: [], files: [], fileCount: 0 };

  for (const change of files) {
    const parts = change.path.split("/");
    let node = root;
    node.fileCount++;
    for (let i = 0; i < parts.length - 1; i++) {
      const path = parts.slice(0, i + 1).join("/");
      let child = node.folders.find((folder) => folder.path === path);
      if (!child) {
        child = { path, label: parts[i] ?? "", folders: [], files: [], fileCount: 0 };
        node.folders.push(child);
      }
      child.fileCount++;
      node = child;
    }
    node.files.push({ change, label: parts[parts.length - 1] ?? change.path });
  }

  return root;
}

/** A row the tree renders: a collapsible folder, or a file leaf. */
export type FileTreeRow =
  | { kind: "folder"; path: string; label: string; depth: number; fileCount: number; collapsed: boolean }
  | { kind: "file"; change: FileChange; label: string; depth: number };

/**
 * Flatten the tree into an ordered list of rows for rendering.
 *
 * At every level folders come before files and each group is sorted
 * alphabetically by label, so the order is stable and independent of the order
 * `git status` reported the files in. `depth` drives indentation; the root
 * itself is never emitted (its children start at depth 0). `isCollapsed` is
 * consulted per folder path — a collapsed folder is still emitted (so it can be
 * clicked open) but none of its descendants are.
 */
export function flattenFileTree(
  root: FileTreeFolder,
  isCollapsed: (folderPath: string) => boolean,
): FileTreeRow[] {
  const rows: FileTreeRow[] = [];

  const walk = (folder: FileTreeFolder, depth: number) => {
    for (const sub of [...folder.folders].sort((a, b) => a.label.localeCompare(b.label))) {
      const collapsed = isCollapsed(sub.path);
      rows.push({
        kind: "folder",
        path: sub.path,
        label: sub.label,
        depth,
        fileCount: sub.fileCount,
        collapsed,
      });
      if (!collapsed) walk(sub, depth + 1);
    }
    for (const file of [...folder.files].sort((a, b) => a.label.localeCompare(b.label))) {
      rows.push({ kind: "file", change: file.change, label: file.label, depth });
    }
  };

  walk(root, 0);
  return rows;
}

/** How the Files view lays out its list: a flat path list, or a folder tree. */
export type FilesLayout = "flat" | "tree";

/**
 * Which layout the Files view opens in, given whatever `localStorage` held.
 *
 * The tree is the default: a flat list of full paths is only easier to read
 * when there are very few changes, and a workspace with very few changes reads
 * fine either way. An explicit stored `"flat"` is honoured — the toggle is a
 * choice the user made, not a preference to be overridden — but an absent or
 * unrecognisable value means the user has never chosen, so they get the tree.
 */
export function defaultFilesLayout(stored: string | null): FilesLayout {
  return stored === "flat" ? "flat" : "tree";
}

/**
 * Serialise the set of folded-away folder keys for `localStorage`.
 *
 * Sorted so an unchanged set produces an unchanged string, which keeps the
 * write idempotent and makes the stored value diffable by eye.
 */
export function encodeCollapsedFolders(collapsed: Set<string>): string {
  return JSON.stringify([...collapsed].sort());
}

/**
 * Read back what `encodeCollapsedFolders` wrote.
 *
 * A missing, malformed or wrongly-shaped value yields an empty set rather than
 * an error: a corrupt preference must never stop the Changes tab drawing, and
 * "nothing is folded away" is the honest fallback — every folder open shows
 * the user everything, where a wrong guess would hide changes from them.
 */
export function decodeCollapsedFolders(raw: string | null): Set<string> {
  if (raw === null) return new Set();
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((entry): entry is string => typeof entry === "string"));
  } catch {
    return new Set();
  }
}
