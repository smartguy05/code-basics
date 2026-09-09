import { allChangedIndices, focusedBaseline, normaliseEndings, onlyHunks } from "./diffLogic";
import { hunkRisk, type RiskIndex } from "./riskLogic";
import type { DiffLayout } from "./DiffView";
import type { ErosionFlag, FileContents, FileDiff, IntentGroup } from "../ipc/types";

/**
 * The decisions behind the diff pane, kept out of `DiffPane.tsx` so they can be
 * tested in the node environment (there is no DOM here — see CLAUDE.md).
 *
 * Everything in this module is a rule about *what the pane may offer*, which is
 * the cheapest thing to get subtly wrong when the pane is moved: an action
 * offered for a diff it cannot act on stages the wrong thing silently.
 */

export const DIFF_LAYOUT_KEY = "code-basics.diffLayout";
export const COLLAPSE_KEY = "code-basics.diffCollapseUnchanged";
export const WHITESPACE_KEY = "code-basics.diffIgnoreWhitespace";

/** Only the method the loaders need, so a fake is trivial in a test. */
type ReadableStorage = Pick<Storage, "getItem">;

export function loadDiffLayout(storage: ReadableStorage): DiffLayout {
  return storage.getItem(DIFF_LAYOUT_KEY) === "inline" ? "inline" : "sideBySide";
}

/** Off unless it was explicitly turned on — folding hides code by default. */
export function loadCollapse(storage: ReadableStorage): boolean {
  return storage.getItem(COLLAPSE_KEY) === "true";
}

/** Off by default: the honest comparison is the one git actually made. */
export function loadIgnoreWhitespace(storage: ReadableStorage): boolean {
  return storage.getItem(WHITESPACE_KEY) === "true";
}

/**
 * What the pane shows: the whole diff, or — when a card scopes the file — only
 * that card's hunks in it. The whole-file buttons act on this, so "Revert
 * shown" reverts what is on screen, never hidden changes.
 *
 * `scopedHunks` alone decides it. The view used to also require the intent
 * grouping, but leaving that grouping already clears the scope, so the two were
 * never independent and the pane has no use for the grouping otherwise.
 */
export function shownDiff(diff: FileDiff | null, scopedHunks: number[] | null): FileDiff | null {
  return scopedHunks != null && diff != null ? onlyHunks(diff, scopedHunks) : diff;
}

/**
 * The left-hand side of the comparison.
 *
 * With a card scoping the file, the baseline is rebuilt as the working copy
 * with only that card's hunks reverted, so every other region is identical on
 * both sides and only this intent's change reads as a difference. Anything
 * missing — no scope, no diff, no working copy, no baseline — falls back to the
 * file's real baseline rather than inventing one.
 */
export function viewBaseline(
  contents: FileContents | null,
  diff: FileDiff | null,
  scopedHunks: number[] | null,
): string | null {
  if (
    scopedHunks != null &&
    diff != null &&
    contents?.working != null &&
    contents?.baseline != null
  ) {
    return focusedBaseline(normaliseEndings(contents.working), onlyHunks(diff, scopedHunks).hunks);
  }
  return contents?.baseline ?? null;
}

/**
 * Per-line risk for the open file, computed against the **full** diff so the
 * hunk indices line up with the intent groups' `files[].hunks` (which index the
 * full `FileDiff.hunks`). The entries are keyed by `DiffLine.index`, which
 * survives `onlyHunks` scoping unchanged, so they still land correctly when the
 * pane shows only a card's hunks — passing `shownDiff` here instead would
 * renumber the hunks and paint the wash on the wrong lines.
 */
export function riskIndices(
  path: string | null,
  diff: FileDiff | null,
  erosionFlags: ErosionFlag[],
  groups: IntentGroup[],
): RiskIndex[] {
  if (!diff || !path) return [];
  const out: RiskIndex[] = [];
  diff.hunks.forEach((hunk, hunkIndex) => {
    const level = hunkRisk(path, hunkIndex, diff, erosionFlags, groups);
    if (!level) return;
    for (const line of hunk.lines) {
      if (line.origin === "context") continue;
      out.push({ index: line.index, level });
    }
  });
  return out;
}

/** Whether the whole-file revert acts on the card's hunks or the whole file. */
export function revertAllLabel(scopedHunks: number[] | null): "shown" | "file" {
  return scopedHunks != null ? "shown" : "file";
}

export interface DiffToolbarState {
  /** What the marker strip marks and what F7 steps through. */
  differences: number;
  hasSelection: boolean;
  /** There is something to revert, ignoring whether an action is in flight. */
  hasRevertableChanges: boolean;
  canRevertSelected: boolean;
  canRevertAll: boolean;
  canStage: boolean;
  canStep: boolean;
  revertAllLabel: "shown" | "file";
}

/**
 * The toolbar's whole abstain surface in one place: every field is a "do not
 * offer an action we cannot perform" rule.
 *
 * `hasRevertableChanges` is deliberately separate from `canRevertAll`: the
 * deleted-file "Restore it" button offers the same revert without the busy
 * gate, so collapsing the two would either disable Restore for the wrong reason
 * or enable Revert during an action.
 */
export function diffToolbarState(input: {
  shownDiff: FileDiff | null;
  scopedHunks: number[] | null;
  selectedLines: number[];
  path: string | null;
  busy: boolean;
}): DiffToolbarState {
  const differences = input.shownDiff?.hunks.length ?? 0;
  const hasSelection = input.selectedLines.length > 0;
  const hasRevertableChanges =
    input.shownDiff != null && allChangedIndices(input.shownDiff).length > 0;
  return {
    differences,
    hasSelection,
    hasRevertableChanges,
    canRevertSelected: !input.busy && hasSelection,
    canRevertAll: !input.busy && hasRevertableChanges,
    canStage: !input.busy && input.path != null,
    // Stepping only reads the diff, so it stays available while an action runs.
    canStep: differences > 0,
    revertAllLabel: revertAllLabel(input.scopedHunks),
  };
}
