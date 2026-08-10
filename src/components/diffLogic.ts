import type { FileDiff } from "../ipc/types";

/** Every changed line index in a diff, for "select all". */
export function allChangedIndices(diff: FileDiff): number[] {
  return diff.hunks
    .flatMap((hunk) => hunk.lines)
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}

/**
 * The diff reduced to the named hunks — an intent group's share of one file.
 *
 * Order follows the diff itself, and indices the diff does not have are
 * ignored: the group was computed from an earlier snapshot, and a stale index
 * must not throw the whole view away.
 */
export function onlyHunks(diff: FileDiff, hunks: number[]): FileDiff {
  const wanted = new Set(hunks);
  return { ...diff, hunks: diff.hunks.filter((_, index) => wanted.has(index)) };
}

/** Changed line indices belonging to one hunk. */
export function hunkIndices(diff: FileDiff, hunk: number): number[] {
  return (diff.hunks[hunk]?.lines ?? [])
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}
