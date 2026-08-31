// Decision logic for optional features: what is switched on, and what that hides.
// Pure so it can be unit-tested in the node environment (no DOM); the picker and
// the tab strip are rendering shells.
//
// The store is `cb_core::features`, and every default lives there — this module
// never invents one, it only reads what the backend reported.

import type { FeatureInfo } from "../ipc/types";

/** The feature ids this build knows about. Mirrors `cb_core::features::FeatureId`. */
export type FeatureKey = "sqlConsole" | "askCodebase";

/**
 * Whether a feature is on.
 *
 * `features === null` means the list has not arrived yet, and is answered with
 * **true** rather than false. Every feature defaults to on in `cb-core`, so the
 * two agree for everyone except a user who has turned something off — and for
 * them a single frame of an extra tab is a far smaller wrong than a missing tab
 * for everyone else. In practice the window does not exist: `App` loads the list
 * once at startup, before a workspace can be opened.
 *
 * An id the backend did not report is also **true**, for the same reason: a
 * feature this build renders but an older store never heard of is new, and new
 * features arrive enabled.
 */
export function featureEnabled(
  features: FeatureInfo[] | null,
  id: FeatureKey,
): boolean {
  if (features === null) return true;
  const found = features.find((f) => f.id === id);
  return found ? found.enabled : true;
}

/**
 * Filter a tab list by the feature each tab belongs to.
 *
 * A tab absent from `featureByTab` is core and always shown; only a tab that
 * names a feature can be hidden. Generic over the tab id so the caller keeps its
 * own union type rather than widening it to `string` here.
 */
export function visibleTabs<T extends { id: string }>(
  tabs: readonly T[],
  features: FeatureInfo[] | null,
  featureByTab: Partial<Record<string, FeatureKey>>,
): T[] {
  return tabs.filter((tab) => {
    const feature = featureByTab[tab.id];
    return feature === undefined || featureEnabled(features, feature);
  });
}

/**
 * Which tab to select once `visible` is the new set.
 *
 * The case with no obvious answer: the user is *looking at* the SQL tab and
 * turns the SQL console off. Leaving `active` selected would render a tab strip
 * with nothing beneath it, so fall back — to the first visible tab, which is Run.
 * An active tab that is still visible is never disturbed, and an empty visible
 * set returns `null` rather than inventing an id nothing can render.
 */
export function tabAfterDisable<T extends { id: string }>(
  active: string,
  visible: readonly T[],
): string | null {
  if (visible.some((tab) => tab.id === active)) return active;
  return visible[0]?.id ?? null;
}
