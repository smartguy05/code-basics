import type { ChangeCoverage } from "../ipc/types";

/**
 * Deciding what the coverage-of-change overlay says and paints.
 *
 * `coverage_of_change` in `cb-core` maps the last coverage-enabled test run onto
 * the current diff and reports, per file, which changed lines were never
 * executed. Rendering lives in `ChangesView`/`IntentPanel`/`DiffView`; every
 * decision they make — the summary wording, which diff-line indices to tint,
 * whether a card has uncovered lines of its own — is here so it can be tested in
 * the node environment with no DOM.
 *
 * The abstain rule carries over from the backend: a changed line the coverage
 * tool could not classify is excluded from `changedLines` entirely, and files
 * whose coverage could not be matched land in `warnings` (counted as
 * `abstained`), never silently dropped.
 */

/**
 * Whether a report actually describes coverage of the diff.
 *
 * `coverage_of_change` returns an empty map carrying a warning when no
 * coverage-enabled run has happened yet; that is not something to summarise as
 * "0 changed lines". A type predicate so callers can gate the summary section on
 * it and keep a narrowed `ChangeCoverage` inside.
 */
export function hasCoverage(
  report: ChangeCoverage | null | undefined,
): report is ChangeCoverage {
  return report != null && (report.files.length > 0 || report.changedLines > 0);
}

/**
 * The one-line summary shown above the cards, twin of
 * `behavioralPanelLogic.behavioralScoreLine`.
 *
 * Reads e.g. `42 changed lines · 35 covered · 7 uncovered · 1 abstained`, where
 * abstained is the number of files coverage could not be matched for
 * (`warnings.length`). Reads sensibly at zero.
 */
export function coverageSummaryLine(report: ChangeCoverage): string {
  const changed = report.changedLines;
  return [
    `${changed} changed line${changed === 1 ? "" : "s"}`,
    `${report.coveredLines} covered`,
    `${report.uncoveredLines} uncovered`,
    `${report.warnings.length} abstained`,
  ].join(" · ");
}

/**
 * Every uncovered `DiffLine.index` across the whole report.
 *
 * These are only meaningful within the comparison mode the report was built in,
 * so a caller must clear them on a mode change — exactly as the erosion flags
 * are. Indices are per-file in the diff model, so when painting one file's
 * pane prefer {@link uncoveredIndicesForPath}, which avoids a stray index from
 * another file colliding with a line number here.
 */
export function uncoveredIndices(
  report: ChangeCoverage | null | undefined,
): number[] {
  if (!report) return [];
  return report.files.flatMap((file) => file.uncovered.map((line) => line.index));
}

/**
 * The uncovered `DiffLine.index` values for one file only.
 *
 * The DiffView holds a single file's diff, whose line indices start at 0 for
 * that file; an index from a different file could collide with a real line here.
 * So the pane is tinted from this per-file set, matched by path — mirroring how
 * `openErosionFlag` scopes its highlight to `flag.path`.
 */
export function uncoveredIndicesForPath(
  report: ChangeCoverage | null | undefined,
  path: string | null,
): number[] {
  if (!report || path == null) return [];
  const file = report.files.find((f) => f.path === path);
  return file ? file.uncovered.map((line) => line.index) : [];
}

/**
 * How many of a card's own changed lines the run never executed.
 *
 * `cardLines` is the set of `DiffLine.index` values a card claims (across its
 * files); this counts the uncovered indices that fall inside it — the same
 * Set-has intersection `intentPanelLogic.cardRisk` uses against erosion flags.
 * Zero means either fully covered or no coverage collected yet; the caller only
 * badges a card when this is positive.
 */
export function cardCoverage(
  cardLines: Set<number>,
  report: ChangeCoverage | null | undefined,
): number {
  if (!report) return 0;
  let count = 0;
  for (const index of uncoveredIndices(report)) {
    if (cardLines.has(index)) count += 1;
  }
  return count;
}
