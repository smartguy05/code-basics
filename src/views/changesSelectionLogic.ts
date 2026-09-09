import type { FileChange } from "../ipc/types";

/**
 * How a click on a file row changes the multi-selection.
 *
 * This is separate from `selectedPath`, which means "the file the diff pane is
 * showing". A row can be the one on screen without being part of a selection,
 * and a selection can span rows without changing what is displayed.
 */
export type ClickModifier = "none" | "toggle" | "range";

/** Which modifier a mouse event asks for, in the order the platforms expect. */
export function clickModifier(event: {
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): ClickModifier {
  if (event.shiftKey) return "range";
  if (event.ctrlKey || event.metaKey) return "toggle";
  return "none";
}

/**
 * The selection after a click, plus the anchor a later range-click extends
 * from.
 *
 * A range with no anchor yet is treated as a plain click rather than guessed
 * at — there is no "obvious" other end to select to.
 */
export function toggleSelection(
  selected: ReadonlySet<string>,
  path: string,
  modifier: ClickModifier,
  ordered: readonly string[],
  anchor: string | null,
): { selected: Set<string>; anchor: string | null } {
  if (modifier === "toggle") {
    const next = new Set(selected);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    return { selected: next, anchor: path };
  }

  if (modifier === "range" && anchor != null) {
    const from = ordered.indexOf(anchor);
    const to = ordered.indexOf(path);
    if (from !== -1 && to !== -1) {
      const [lo, hi] = from <= to ? [from, to] : [to, from];
      // The anchor stays put, so dragging a range back and forth keeps
      // extending from where it started rather than walking away.
      return { selected: new Set(ordered.slice(lo, hi + 1)), anchor };
    }
  }

  return { selected: new Set([path]), anchor: path };
}

/**
 * The selection a right-click acts on.
 *
 * Right-clicking inside an existing selection keeps it; right-clicking outside
 * replaces it with the one row. Anything else either silently acts on rows the
 * user cannot see, or throws away a selection they just made.
 */
export function contextSelection(selected: ReadonlySet<string>, clicked: string): Set<string> {
  return selected.has(clicked) ? new Set(selected) : new Set([clicked]);
}

/**
 * The subset of a selection that can actually be stashed.
 *
 * A conflicted file has no single content to capture, and a path with neither
 * a staged nor an unstaged change has nothing to set aside — both are dropped
 * here rather than sent to the backend to be refused one at a time.
 */
export function stashablePaths(
  selected: ReadonlySet<string>,
  files: readonly FileChange[],
): string[] {
  const eligible = new Set(
    files
      .filter(
        (file) =>
          (file.staged != null || file.unstaged != null) &&
          file.staged !== "conflicted" &&
          file.unstaged !== "conflicted",
      )
      .map((file) => file.path),
  );
  return [...selected].filter((path) => eligible.has(path)).sort();
}

/** The right-click menu's wording for the files it would stash. */
export function stashMenuLabel(count: number): string {
  if (count <= 0) return "";
  return count === 1 ? "Stash file…" : `Stash ${count} files…`;
}

/**
 * What the stash-message prompt starts with.
 *
 * The file's own name is a better starting point than a generic label, because
 * the stash list is read later with no memory of what was going on.
 */
export function defaultStashMessage(paths: readonly string[]): string {
  const first = paths[0];
  if (first == null) return "work in progress";
  const name = first.split("/").pop() || first;
  return paths.length === 1 ? name : `${name} +${paths.length - 1} more`;
}
