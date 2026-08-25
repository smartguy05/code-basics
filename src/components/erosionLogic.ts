import type { ErosionCategory, ErosionFlag, ErosionReport } from "../ipc/types";

/**
 * Deciding how the erosion flags are labelled and ordered for the panel.
 *
 * Rendering lives in `ErosionPanel.tsx`; every decision the panel makes — the
 * human label for a category, the order categories appear in, the count on the
 * toggle badge — is here so it can be tested in the node environment with no
 * DOM. The detector itself ranks nothing (a flag carries no severity), so the
 * order below is a display choice, not a claim about which weakening is worse.
 */

/** The order categories are shown in — most-scrutinised first. */
export const CATEGORY_ORDER: ErosionCategory[] = [
  "secret",
  "deletedAssertion",
  "ignoredTest",
  "removedNullCheck",
  "removedSafeguard",
  "schemaRisk",
  "widenedCatch",
  "unsafeCast",
  "leftoverStub",
  "droppedLog",
  "logDowngrade",
];

const CATEGORY_LABEL: Record<ErosionCategory, string> = {
  secret: "Hardcoded secrets",
  deletedAssertion: "Deleted assertions",
  ignoredTest: "Skipped tests",
  removedNullCheck: "Removed null checks",
  removedSafeguard: "Removed safeguards",
  schemaRisk: "Backward-incompatible schema changes",
  widenedCatch: "Widened / swallowed catches",
  unsafeCast: "Unsafe casts & panics",
  leftoverStub: "Stubs & TODOs left behind",
  droppedLog: "Dropped log lines",
  logDowngrade: "Log level lowered",
};

/** The human heading for a category. */
export function categoryLabel(category: ErosionCategory): string {
  return CATEGORY_LABEL[category];
}

/** One category's flags, for a section in the panel. */
export interface ErosionSection {
  category: ErosionCategory;
  label: string;
  flags: ErosionFlag[];
}

/**
 * Group flags by category, in {@link CATEGORY_ORDER}, dropping empty
 * categories. Flags within a section keep the scan's order (file, then line).
 */
export function groupByCategory(flags: ErosionFlag[]): ErosionSection[] {
  return CATEGORY_ORDER.map((category) => ({
    category,
    label: CATEGORY_LABEL[category],
    flags: flags.filter((f) => f.category === category),
  })).filter((section) => section.flags.length > 0);
}

/** The number on the Erosion toggle — the count of flags, or 0 for none. */
export function badgeCount(report: ErosionReport | null): number {
  return report ? report.flags.length : 0;
}
