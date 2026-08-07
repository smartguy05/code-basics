import type { FileDiff } from "../ipc/types";

/** Every changed line index in a diff, for "select all". */
export function allChangedIndices(diff: FileDiff): number[] {
  return diff.hunks
    .flatMap((hunk) => hunk.lines)
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}

/** Changed line indices belonging to one hunk. */
export function hunkIndices(diff: FileDiff, hunk: number): number[] {
  return (diff.hunks[hunk]?.lines ?? [])
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}
