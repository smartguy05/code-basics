import type {
  ErosionCategory,
  ErosionFlag,
  FileDiff,
  IntentGroup,
} from "../ipc/types";

/**
 * The one place risk is decided for the Changes tab.
 *
 * "Risk-weighted diff" emphasises the files and hunks most worth a second look
 * and lets formatting-only noise recede — all from signals the client already
 * holds (the erosion flags, the intent grouping, the paths), so nothing new is
 * fetched and the whole thing inherits the mode-change clearing the erosion and
 * intent state already do.
 *
 * The abstain rule the rest of the Changes code follows is sharp here, because
 * emphasis on everything is emphasis on nothing: a file or hunk that trips no
 * concrete signal gets no weight at all (`null`), never a guessed one. The two
 * building blocks below — {@link SENSITIVE_PATTERNS} and
 * {@link HIGH_RISK_EROSION} — are the same ones the per-card badge in
 * `intentPanelLogic.ts` reads; they live here so there is exactly one
 * definition, and `intentPanelLogic` imports them back.
 */

// Matched at a path boundary (start, or after / \ . _ -), never as a bare
// substring, so "author(s)" is not read as "auth" and an ordinary "config" file
// is not flagged at all (it was dropped — far too broad to be a signal). Each
// marker names a location where a mistake costs more than usual.
export const SENSITIVE_PATTERNS: RegExp[] = [
  /(^|[/\\._-])auth(?!or)/i, // auth, authn, authz, authentication — not author(s)
  /(^|[/\\._-])(security|secure)([/\\._-]|$)/i,
  /(^|[/\\._-])crypto/i,
  /(^|[/\\._-])secret/i,
  /(^|[/\\._-])credential/i,
  /(^|[/\\._-])passw(or)?d/i, // password / passwd
  /(^|[/\\._-])payment/i,
  /(^|[/\\._-])billing/i,
  /(^|[/\\._-])migrations?([/\\._-]|$)/i,
  /(^|[/\\._-])\.?env([/\\._-]|$)/i, // .env, .env.local, env/
];

/** Erosion categories severe enough to push a file or hunk to "high". */
export const HIGH_RISK_EROSION: ErosionCategory[] = [
  "secret",
  "removedSafeguard",
  "deletedAssertion",
];

/** True when a path names a place where a mistake costs more than usual. */
export function isSensitivePath(path: string): boolean {
  return SENSITIVE_PATTERNS.some((re) => re.test(path));
}

/** The weight a whole file carries. `high` emphasises hardest, `null` abstains. */
export type FileRiskLevel = "high" | "elevated";

/** The weight one hunk carries. `formatting` is the one that *recedes*. */
export type HunkRiskLevel = "high" | "elevated" | "formatting";

/** One changed line and the weight its hunk carries, for the diff overlay. */
export interface RiskIndex {
  /** A `DiffLine.index`, only meaningful within one comparison mode. */
  index: number;
  level: HunkRiskLevel;
}

/**
 * How much a whole file is worth scrutinising, for the file-list sort and the
 * row emphasis — derived only from signals the client already has.
 *
 * The `score` orders files within a section (highest first); the `level` drives
 * the emphasis class. Both abstain to `null` when nothing concrete elevates the
 * file, so the ordinary file keeps its git order and its plain row.
 *
 * What contributes, in rising severity:
 *  - a {@link SENSITIVE_PATTERNS} path — a location where a mistake costs more;
 *  - an intent card touching the file that states no reason (`other`) or was
 *    attributed with low confidence — an unexplained or weakly-tied change;
 *  - an erosion flag on the file — and a {@link HIGH_RISK_EROSION} one (a
 *    removed assertion or safeguard, a hardcoded secret) is what makes it
 *    `high` rather than `elevated`.
 */
export function fileRisk(
  path: string,
  erosionFlags: ErosionFlag[],
  groups: IntentGroup[],
): { level: FileRiskLevel; score: number } | null {
  let score = 0;
  let high = false;

  if (isSensitivePath(path)) score += 3;

  for (const flag of erosionFlags) {
    if (flag.path !== path) continue;
    if (HIGH_RISK_EROSION.includes(flag.category)) {
      score += 5;
      high = true;
    } else {
      score += 1;
    }
  }

  for (const group of groups) {
    if (!group.files.some((f) => f.path === path)) continue;
    if (group.kind === "other") score += 2;
    if (group.confidence === "low") score += 1;
  }

  if (score === 0) return null;
  return { level: high ? "high" : "elevated", score };
}

/** The intent cards that own a given hunk of a file (via `files[].hunks`). */
function owningGroups(
  path: string,
  hunkIndex: number,
  groups: IntentGroup[],
): IntentGroup[] {
  return groups.filter((group) =>
    group.files.some((f) => f.path === path && f.hunks.includes(hunkIndex)),
  );
}

/**
 * How much one hunk is worth scrutinising, for the diff overlay and the marker
 * strip — again from signals already on the client.
 *
 * Precedence, and why it is this way:
 *  1. An erosion flag landing on one of the hunk's own lines dominates: a
 *     {@link HIGH_RISK_EROSION} category is `high`, anything else `elevated`.
 *     A flag elsewhere in the file belongs to another hunk and is ignored.
 *  2. Failing that, an `intent`/`other` change on a {@link SENSITIVE_PATTERNS}
 *     path is `elevated` — a real change in a costly place.
 *  3. Failing that, a hunk whose every owning card is `formatting` *recedes*
 *     (`formatting`) — the one level that de-emphasises rather than emphasises.
 *  4. Otherwise `null`: nothing concrete, so no weight.
 *
 * An out-of-range hunk index abstains rather than throwing — the grouping can
 * lag the diff by a write.
 */
export function hunkRisk(
  path: string,
  hunkIndex: number,
  diff: FileDiff,
  erosionFlags: ErosionFlag[],
  groups: IntentGroup[],
): HunkRiskLevel | null {
  const hunk = diff.hunks[hunkIndex];
  if (!hunk) return null;

  const indices = new Set(hunk.lines.map((l) => l.index));

  // 1. Erosion flags on this hunk's own lines. A high-severity one wins
  //    outright; a lower one still elevates but does not stop the scan, so the
  //    result does not depend on flag order.
  let erosion: HunkRiskLevel | null = null;
  for (const flag of erosionFlags) {
    if (flag.path !== path || !indices.has(flag.index)) continue;
    if (HIGH_RISK_EROSION.includes(flag.category)) return "high";
    erosion = "elevated";
  }
  if (erosion) return erosion;

  const owners = owningGroups(path, hunkIndex, groups);

  // 2. A stated or unexplained change on a sensitive path — worth the eye.
  if (isSensitivePath(path) && owners.some((g) => g.kind === "intent" || g.kind === "other")) {
    return "elevated";
  }

  // 3. Formatting-only: at least one card owns it and none is anything but
  //    formatting. This is the recede case.
  if (owners.length > 0 && owners.every((g) => g.kind === "formatting")) {
    return "formatting";
  }

  return null;
}

/** The severe of two hunk levels; `high` > `elevated` > `formatting`. */
export function moreSevereHunkRisk(
  a: HunkRiskLevel | undefined,
  b: HunkRiskLevel,
): HunkRiskLevel {
  const rank: Record<HunkRiskLevel, number> = { high: 3, elevated: 2, formatting: 1 };
  if (a === undefined) return b;
  return rank[a] >= rank[b] ? a : b;
}
