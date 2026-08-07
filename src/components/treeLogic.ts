import type {
  Branch,
  InspectGraph,
  InspectNode,
  ObjectValue,
  TestNode,
  TestOutcome,
} from "../ipc/types";

/**
 * Pure tree helpers shared by the three tree components (`BranchMenu`,
 * `ObjectTree`, `TestTree`), lifted out of them verbatim so they can be tested
 * headlessly. Sections below mirror the file each helper came from.
 *
 * `ObjectTree` and `TestTree` each defined a `matches`; they are exported here
 * as `objectMatches` and `testMatches`.
 */

// ---------------------------------------------------------------------------
// BranchMenu.tsx
// ---------------------------------------------------------------------------

/**
 * Slash-named branches (`users/anthony/thing`) rendered as a directory tree:
 * each segment before the last is a collapsible folder.
 */
export interface BranchFolder {
  /** Full segment path, e.g. `users/anthony`. Unique within its section. */
  path: string;
  label: string;
  folders: BranchFolder[];
  leaves: { branch: Branch; label: string }[];
}

export function buildTree(branches: Branch[]): BranchFolder {
  const root: BranchFolder = { path: "", label: "", folders: [], leaves: [] };

  for (const branch of branches) {
    const parts = branch.name.split("/");
    let node = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const path = parts.slice(0, i + 1).join("/");
      let child = node.folders.find((folder) => folder.path === path);
      if (!child) {
        child = { path, label: parts[i] ?? "", folders: [], leaves: [] };
        node.folders.push(child);
      }
      node = child;
    }
    node.leaves.push({ branch, label: parts[parts.length - 1] ?? branch.name });
  }
  return root;
}

/** Folder paths leading to a branch, so the current one can start expanded. */
export function ancestorPaths(name: string): string[] {
  const parts = name.split("/");
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

// ---------------------------------------------------------------------------
// ObjectTree.tsx
// ---------------------------------------------------------------------------

/** The text a filter should be able to find a node by. */
export function searchableValue(value: ObjectValue): string {
  switch (value.kind) {
    case "primitive":
      return value.text;
    case "text":
      return value.text;
    case "reference":
      return `${value.typeName} ${value.address}`;
    case "cycle":
      return value.path;
    case "unavailable":
      return value.reason;
    case "null":
    case "elided":
      return "";
  }
  return "";
}

export function objectMatches(node: InspectNode, text: string): boolean {
  if (text === "") return true;

  const own =
    node.label.toLowerCase().includes(text) ||
    (node.typeName?.toLowerCase().includes(text) ?? false) ||
    searchableValue(node.value).toLowerCase().includes(text);
  if (own) return true;

  // A branch survives if any descendant does, so filtering never hides a
  // matching field behind a non-matching parent.
  return node.children.some((child) => objectMatches(child, text));
}

/** "showing 3 of 5,412" — only ever claims a total the inspector counted. */
export function countLabel(node: InspectNode): string | null {
  const shown = node.children.length;
  if (node.childCountTotal != null && node.childCountTotal > shown) {
    return `showing ${shown.toLocaleString()} of ${node.childCountTotal.toLocaleString()}`;
  }
  if (node.hasMore) {
    // The inspector knew there was more but not how much; saying so beats
    // inventing a total.
    return `showing ${shown.toLocaleString()}, more not read`;
  }
  return null;
}

export function targetLabel(graph: InspectGraph): string {
  const target = graph.target.target;
  if (target.kind === "dump") return target.path;
  const name = graph.target.processName;
  return name != null ? `${name} (pid ${target.pid})` : `pid ${target.pid}`;
}

// ---------------------------------------------------------------------------
// TestTree.tsx
// ---------------------------------------------------------------------------

export function formatDuration(ms: number | null): string {
  if (ms == null) return "";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

/** Does this node, or anything below it, match the filters? */
export function testMatches(
  node: TestNode,
  text: string,
  outcomes: Set<TestOutcome>,
): boolean {
  const outcomeOk = outcomes.size === 0 || outcomes.has(node.outcome);
  const textOk =
    text === "" ||
    node.label.toLowerCase().includes(text) ||
    (node.case?.fullName.toLowerCase().includes(text) ?? false);

  if (node.children.length === 0) {
    return outcomeOk && textOk;
  }
  // A branch survives if any descendant does, so filtering never hides a
  // matching test behind a non-matching parent.
  return node.children.some((child) => testMatches(child, text, outcomes));
}
