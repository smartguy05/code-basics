import type { Changelist, FileChange, IntentGroup } from "../ipc/types";

/**
 * The letter and colour for a file *within a given section*.
 *
 * A partially staged file appears under both Staged and its unstaged section —
 * the way `git status` lists it twice — so the kind shown has to come from the
 * section rather than from the file, or the same row would read identically in
 * both places.
 */
export function statusLetter(
  change: FileChange,
  side: "staged" | "unstaged",
): { letter: string; className: string } {
  if (change.staged === "conflicted" || change.unstaged === "conflicted") {
    return { letter: "!", className: "conflicted" };
  }
  const kind = side === "staged" ? change.staged : change.unstaged;
  const letter =
    kind === "added" || kind === "untracked"
      ? "A"
      : kind === "deleted"
        ? "D"
        : kind === "renamed"
          ? "R"
          : "M";

  return { letter, className: side };
}

/** A section of the file list: the index, a named group, or the leftovers. */
export interface FileSection {
  key: string;
  label: string;
  /** The group these files belong to; `null` for Staged and Unstaged. */
  group: string | null;
  side: "staged" | "unstaged";
  files: FileChange[];
  /** Custom groups stay visible while empty so they can be dropped into. */
  keepWhenEmpty: boolean;
}

/**
 * Split the working tree into the sections the sidebar draws.
 *
 * Staged and unstaged are taken from the two halves of `git status` rather
 * than being made exclusive, so a partially staged file honestly appears in
 * both. Custom groups only ever hold unstaged work: once something is staged,
 * the index is the grouping that matters.
 */
export function buildSections(
  files: FileChange[],
  groups: Changelist[],
): FileSection[] {
  const staged = files.filter((f) => f.staged != null);
  const unstaged = files.filter((f) => f.unstaged != null);

  const groupOf = (path: string) =>
    groups.find((g) => g.paths.includes(path))?.name ?? null;

  return [
    {
      key: "staged",
      label: "Staged",
      group: null,
      side: "staged",
      files: staged,
      keepWhenEmpty: false,
    },
    ...groups.map((group) => ({
      key: `group:${group.name}`,
      label: group.name,
      group: group.name,
      side: "unstaged" as const,
      files: unstaged.filter((f) => groupOf(f.path) === group.name),
      keepWhenEmpty: true,
    })),
    {
      key: "unstaged",
      label: "Unstaged",
      group: null,
      side: "unstaged",
      files: unstaged.filter((f) => groupOf(f.path) === null),
      keepWhenEmpty: false,
    },
  ];
}

/**
 * Re-order the files *within* each section so the riskier ones surface first,
 * without disturbing the Staged / group / Unstaged partition.
 *
 * The partition is a git fact and must not move — only the order inside a
 * section is a display choice, so this maps each section to a re-ordered copy
 * and leaves everything else (labels, sides, keep-when-empty) untouched.
 *
 * The sort is by risk score descending and is **stable**: files that score
 * equally — every file with no risk signal at all scores 0 — keep the order
 * `git status` reported them in. Sorting on a decorated copy carrying the
 * original index guarantees that regardless of the engine's own sort
 * stability. `riskOf` abstaining (`null`) is a score of 0, so an unweighted
 * list comes back in exactly its original order.
 */
export function sortFilesByRisk(
  sections: FileSection[],
  riskOf: (path: string) => { level: "high" | "elevated"; score: number } | null,
): FileSection[] {
  return sections.map((section) => ({
    ...section,
    files: section.files
      .map((file, index) => ({ file, index, score: riskOf(file.path)?.score ?? 0 }))
      .sort((a, b) => b.score - a.score || a.index - b.index)
      .map((entry) => entry.file),
  }));
}

/** What an open card's highlight should become after the groups reload. */
export type SelectionReconcile =
  | { kind: "skip" }
  | { kind: "update"; highlight: number[]; groupHunks: number[] | null }
  | { kind: "clear" };

/**
 * Re-derive an open card's highlight from freshly loaded intent groups.
 *
 * The cards are recomputed out from under a selection whenever the poll — or a
 * visibility refresh — picks up records an out-of-process hook wrote: the diff
 * renumbers and a card's `lineIndices` move. A highlight captured when the card
 * was clicked would then paint against stale line numbers (this is how a card
 * came to highlight another card's region until the file was reopened).
 *
 * - No card is selected: `skip` — a plain open file owns its own view.
 * - The selected card is gone but groups did load: `clear` the selection.
 * - Groups are momentarily empty (a reload in flight): `skip`, so a transient
 *   empty result never discards a live selection.
 * - Otherwise: `update` to the card's current lines for the selected file (or
 *   its first file, matching how a card opens).
 */
export function reconcileSelection(
  groups: IntentGroup[],
  selectedGroupId: string | null,
  selectedPath: string | null,
): SelectionReconcile {
  if (selectedGroupId === null) return { kind: "skip" };

  const group = groups.find((g) => g.id === selectedGroupId);
  if (!group) return groups.length === 0 ? { kind: "skip" } : { kind: "clear" };

  const file =
    (selectedPath !== null && group.files.find((f) => f.path === selectedPath)) ||
    group.files[0];
  if (!file) return { kind: "clear" };

  return { kind: "update", highlight: file.lineIndices, groupHunks: file.hunks };
}
