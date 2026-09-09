import { describe, it, expect } from "vitest";
import { featureEnabled, visibleTabs, tabAfterDisable, type FeatureKey } from "./featuresLogic";
import type { FeatureInfo } from "../ipc/types";

function feature(id: string, enabled: boolean): FeatureInfo {
  return { id, label: id, description: "", enabled };
}

const TABS = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
  { id: "sql", label: "SQL" },
] as const;

const BY_TAB = { sql: "sqlConsole" } as const;

describe("featureEnabled", () => {
  it("reads what the backend reported", () => {
    const features = [feature("sqlConsole", false), feature("askCodebase", true)];
    expect(featureEnabled(features, "sqlConsole")).toBe(false);
    expect(featureEnabled(features, "askCodebase")).toBe(true);
  });

  it("treats a not-yet-loaded list as everything on", () => {
    // Defaults are on in cb-core, so this agrees with a loaded list for everyone
    // except a user who turned something off.
    expect(featureEnabled(null, "sqlConsole")).toBe(true);
    expect(featureEnabled(null, "askCodebase")).toBe(true);
  });

  it("treats a feature the store never heard of as on", () => {
    // A store written by an older build has no row for a newly shipped feature,
    // and a new feature arrives enabled.
    expect(featureEnabled([feature("askCodebase", true)], "sqlConsole")).toBe(true);
  });
});

describe("visibleTabs", () => {
  it("hides only the tab whose feature is off", () => {
    const visible = visibleTabs(TABS, [feature("sqlConsole", false)], BY_TAB);
    expect(visible.map((t) => t.id)).toEqual(["run", "tests"]);
  });

  it("keeps a tab that names no feature", () => {
    // Core tabs are not gateable, whatever the store says.
    const visible = visibleTabs(TABS, [feature("sqlConsole", false)], {});
    expect(visible.map((t) => t.id)).toEqual(["run", "tests", "sql"]);
  });

  it("shows everything while the list is loading", () => {
    expect(visibleTabs(TABS, null, BY_TAB)).toHaveLength(3);
  });

  it("preserves the declared order", () => {
    const visible = visibleTabs(TABS, [feature("sqlConsole", true)], BY_TAB);
    expect(visible.map((t) => t.id)).toEqual(["run", "tests", "sql"]);
  });
});

describe("tabAfterDisable", () => {
  it("leaves an active tab alone while it is still visible", () => {
    expect(tabAfterDisable("tests", TABS)).toBe("tests");
  });

  it("falls back to the first visible tab when the active one is switched off", () => {
    // Looking at SQL and turning the SQL console off must not leave a tab strip
    // with nothing beneath it.
    const visible = visibleTabs(TABS, [feature("sqlConsole", false)], BY_TAB);
    expect(tabAfterDisable("sql", visible)).toBe("run");
  });

  it("returns null rather than inventing an id when nothing is visible", () => {
    expect(tabAfterDisable("sql", [])).toBeNull();
  });
});

// The real tab strip, as `WorkspaceTab.tsx` declares it. Duplicated here
// rather than imported because that module is a `.tsx` and vitest runs in the
// node environment with no DOM — so this is a drift alarm, not a re-export: if
// a tab is added there and not here, this suite still passes and the next
// reader is the one who notices.
//
// **`FEATURE_BY_TAB` is empty again.** It held exactly one entry, `sql`, and
// the SQL console is no longer a tab — it is a floating panel, so the feature
// now gates the *opener* rather than a strip entry. The tests that used to
// live here pinned the tab gate and the body gate; both were deleted with the
// tab rather than rewritten to describe something that no longer happens. What
// replaced them is `sqlPanelLogic.sqlPanelMounted` / `sqlPanelAfterFeatureChange`,
// tested beside that module, which carry the same guarantee the body gate did:
// a switched-off console is unmounted, not merely hidden, so it cannot keep a
// live connection and a streaming query with no UI to reach them.
const REAL_TABS = [
  { id: "project", label: "Project" },
  { id: "tests", label: "Tests" },
  { id: "history", label: "History" },
  { id: "architecture", label: "Architecture" },
  { id: "inspect", label: "Objects" },
] as const;

/** Empty, and the type is what says so rather than a comment. */
const REAL_FEATURE_BY_TAB: Partial<Record<string, FeatureKey>> = {};

describe("the real tab strip", () => {
  it("gates no tab on a feature any more", () => {
    // Every tab in the strip is core. If this starts failing, a tab has been
    // put behind a feature and the two gates (strip and body) have to be made
    // to agree again — which is the bug the SQL entry existed to prevent.
    for (const features of [
      null,
      [],
      [feature("sqlConsole", false)],
      [feature("askCodebase", false)],
    ] as (FeatureInfo[] | null)[]) {
      expect(visibleTabs(REAL_TABS, features, REAL_FEATURE_BY_TAB).map((t) => t.id)).toEqual([
        "project",
        "tests",
        "history",
        "architecture",
        "inspect",
      ]);
    }
  });

  it("never moves the selection, because nothing can vanish", () => {
    const visible = visibleTabs(REAL_TABS, [], REAL_FEATURE_BY_TAB);
    for (const id of REAL_TABS.map((t) => t.id)) {
      expect(tabAfterDisable(id, visible)).toBe(id);
    }
  });
});
