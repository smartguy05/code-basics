import type { Branch } from "../ipc/types";
import { ancestorPaths } from "./treeLogic";

/**
 * Filtering the branch menu's list, and the expansion that has to follow it.
 *
 * The menu renders branches as a folder tree (`treeLogic.buildTree`), and a
 * folder only shows its children while its key sits in the expansion set. So a
 * filter on its own is not enough: `users/anthony/spike` matching a query is
 * invisible while `users` and `users/anthony` are still folded, and the user
 * reads that as "nothing matched". `expansionForQuery` exists precisely to stop
 * that — it opens every folder on the path to a match, for as long as a query is
 * active, and hands the stored set straight back the moment it is not.
 *
 * Both helpers are identities on an empty query, and deliberately return the
 * *same reference* they were given rather than a fresh copy: the caller passes
 * the result to React, and a new array or Set on every keystroke of an empty box
 * would re-render (and, for the expansion set, fight the effect that persists
 * it) for no change at all.
 */

/** The key spelling `BranchMenu` renders with — a near-miss expands nothing. */
type Section = "local" | "remote";

function sectionOf(branch: Branch): Section {
  return branch.isRemote ? "remote" : "local";
}

/** `true` once the box holds something worth filtering on. */
export function hasQuery(query: string): boolean {
  return query.trim() !== "";
}

/**
 * Case-insensitive substring match over the **full** branch name.
 *
 * The tree shows only the last segment of a name, but the user is thinking of
 * the whole thing — "feat" has to find `origin/feature/x`, and "origin" has to
 * find every remote branch — so the match never looks at the rendered label.
 */
export function branchMatches(branch: Branch, query: string): boolean {
  return branch.name.toLowerCase().includes(query.trim().toLowerCase());
}

/**
 * The branches a query leaves visible, in their original order.
 *
 * An empty (or whitespace-only) query is not a filter at all, so the input array
 * comes back untouched. Everything else may legitimately produce an empty
 * array — that is the answer, and the caller is expected to say so rather than
 * render an empty list that reads as "this repository has no branches".
 */
export function filterBranches<T extends Branch>(branches: T[], query: string): T[] {
  if (!hasQuery(query)) return branches;
  return branches.filter((branch) => branchMatches(branch, query));
}

/**
 * The expansion set to render with while `query` is active.
 *
 * Built from `stored` rather than from scratch, so a folder the user opened by
 * hand stays open; a folder is added for every ancestor of every match, plus the
 * section header the match lives under (a match in Remote is worthless while the
 * Remote section itself is folded).
 *
 * With no query this is the stored set itself — the same reference — which is
 * what lets the caller keep one persistent set and simply *substitute* this one
 * while filtering, leaving the user's own expansions intact for when the box is
 * cleared again.
 */
export function expansionForQuery(
  branches: Branch[],
  query: string,
  stored: Set<string>,
): Set<string> {
  if (!hasQuery(query)) return stored;

  const next = new Set(stored);
  for (const branch of branches) {
    if (!branchMatches(branch, query)) continue;
    const section = sectionOf(branch);
    next.add(`section:${section}`);
    for (const path of ancestorPaths(branch.name)) {
      next.add(`${section}:${path}`);
    }
  }
  return next;
}
