import type { FileDiff, IntentGroup, SelfConfidence } from "../ipc/types";

/**
 * The one place the confidence heatmap is decided for the Changes tab.
 *
 * The heatmap tints a changed line by how confident the agent said it was in the
 * intent that produced it — the strongest tint on `low`, because that is where
 * the reviewer is being asked to look hardest. It reads only signals the client
 * already holds (the intent grouping and the file's diff), so nothing new is
 * fetched and the overlay inherits the same mode-change clearing the risk and
 * coverage overlays already do.
 *
 * The abstain rule the rest of the Changes code follows is sharp here: a line
 * whose group offered no `selfConfidence` gets **no** tint (it is omitted
 * entirely), never a guessed level. The token is voluntary and therefore usually
 * absent, and dressing an absence up as "high confidence" would be exactly the
 * wrong answer — the whole point is to steer the eye toward what the agent was
 * *unsure* about.
 */

/** Rising order of concern: `low` is the most cautious, so it wins on overlap. */
const RANK: Record<SelfConfidence, number> = { low: 0, medium: 1, high: 2 };

/** The most cautious of two stated levels — `low` beats `medium` beats `high`. */
function moreCautious(a: SelfConfidence, b: SelfConfidence): SelfConfidence {
  return RANK[a] <= RANK[b] ? a : b;
}

/**
 * Map each changed `DiffLine.index` of `path` to the agent's stated confidence
 * for the intent that produced it.
 *
 * For every group carrying a `selfConfidence` whose files include `path`, its
 * recorded `lineIndices` are emitted at that level. A line claimed by more than
 * one such group takes the **lowest** confidence of them — the most cautious
 * reading — so overlapping cards never let a confident claim hide a shaky one.
 *
 * `diff` bounds the output to lines that actually exist in this file's diff: a
 * `lineIndices` entry that no hunk here contains is dropped rather than emitted
 * for a line that is not on screen. Lines whose only group has no
 * `selfConfidence` are omitted entirely (the abstain case).
 */
export function confidenceForFile(
  path: string,
  diff: FileDiff,
  groups: IntentGroup[],
): { index: number; level: SelfConfidence }[] {
  // The changed-line indices this file's diff actually holds. An index a group
  // claims but the diff does not carry paints nothing, so it is dropped here.
  const present = new Set<number>();
  for (const hunk of diff.hunks) {
    for (const l of hunk.lines) present.add(l.index);
  }

  const byIndex = new Map<number, SelfConfidence>();
  for (const group of groups) {
    const level = group.selfConfidence;
    if (!level) continue;
    for (const file of group.files) {
      if (file.path !== path) continue;
      for (const index of file.lineIndices) {
        if (!present.has(index)) continue;
        const existing = byIndex.get(index);
        byIndex.set(index, existing ? moreCautious(existing, level) : level);
      }
    }
  }

  return [...byIndex].map(([index, level]) => ({ index, level }));
}

/** The decoration class for one stated confidence level. */
export function confidenceClass(level: SelfConfidence): string {
  return `cb-line-confidence-${level}`;
}
