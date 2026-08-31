import { describe, it, expect } from "vitest";
import { featureEnabled, visibleTabs, tabAfterDisable } from "./featuresLogic";
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

// The real tab strip, as `WorkspaceTab.tsx` declares it. Duplicated here rather
// than imported because that module is a `.tsx` and vitest runs in the node
// environment with no DOM — so this is a drift alarm, not a re-export: if a tab
// is added there and not here, this suite still passes and the next reader is
// the one who notices. What it does pin is the shape of the first real entry in
// `FEATURE_BY_TAB`, which was empty until the SQL console landed.
const REAL_TABS = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
  { id: "changes", label: "Changes" },
  { id: "history", label: "History" },
  { id: "architecture", label: "Architecture" },
  { id: "inspect", label: "Objects" },
  { id: "sql", label: "SQL" },
] as const;

const REAL_FEATURE_BY_TAB = { sql: "sqlConsole" } as const;

describe("the SQL tab's gate", () => {
  it("hides the SQL tab, and only it, when the SQL console is off", () => {
    const visible = visibleTabs(REAL_TABS, [feature("sqlConsole", false)], REAL_FEATURE_BY_TAB);
    expect(visible.map((t) => t.id)).toEqual([
      "run",
      "tests",
      "changes",
      "history",
      "architecture",
      "inspect",
    ]);
  });

  it("shows it when the feature is on, at the end of the strip", () => {
    const visible = visibleTabs(REAL_TABS, [feature("sqlConsole", true)], REAL_FEATURE_BY_TAB);
    expect(visible.map((t) => t.id)).toHaveLength(7);
    expect(visible[6]?.id).toBe("sql");
  });

  it("moves a user looking at SQL back to Run when they switch it off", () => {
    // The payoff for the picker gating a tab for the first time: turning the
    // feature off while the SQL tab is selected must not leave a tab strip with
    // nothing beneath it.
    const visible = visibleTabs(REAL_TABS, [feature("sqlConsole", false)], REAL_FEATURE_BY_TAB);
    expect(tabAfterDisable("sql", visible)).toBe("run");
  });

  it("leaves every other tab's selection alone when SQL is switched off", () => {
    const visible = visibleTabs(REAL_TABS, [feature("sqlConsole", false)], REAL_FEATURE_BY_TAB);
    expect(tabAfterDisable("history", visible)).toBe("history");
  });

  it("switching an unrelated feature off does not touch the SQL tab", () => {
    const visible = visibleTabs(REAL_TABS, [feature("askCodebase", false)], REAL_FEATURE_BY_TAB);
    expect(visible.map((t) => t.id)).toContain("sql");
  });
});

// The body gate added alongside these: `WorkspaceTab` mounts `SqlView` only
// while the SQL tab survived the filter (`shownTabs.some(t => t.id === "sql")`),
// so the strip and the body read one list and cannot disagree. The mount itself
// is `.tsx` and vitest runs in the node environment with no DOM, so it cannot be
// tested here — what is pinned below is the predicate the gate asks.
const bodyGate = (features: FeatureInfo[] | null) =>
  visibleTabs(REAL_TABS, features, REAL_FEATURE_BY_TAB).some((t) => t.id === "sql");

describe("the SQL body gate", () => {
  it("is closed exactly when the feature is off", () => {
    expect(bodyGate([feature("sqlConsole", false)])).toBe(false);
    expect(bodyGate([feature("sqlConsole", true)])).toBe(true);
  });

  it("agrees with the tab strip in every state, including not-yet-loaded", () => {
    // The bug this replaces: the strip hid the tab while the body kept SqlView
    // mounted, so a switched-off console still held a live connection and a
    // streaming query with no UI to reach them.
    for (const features of [
      null,
      [],
      [feature("sqlConsole", true)],
      [feature("sqlConsole", false)],
      [feature("askCodebase", false)],
    ] as (FeatureInfo[] | null)[]) {
      expect(bodyGate(features)).toBe(featureEnabled(features, "sqlConsole"));
    }
  });

  it("leaves a user switched off the tab on a tab the body still renders", () => {
    const visible = visibleTabs(REAL_TABS, [feature("sqlConsole", false)], REAL_FEATURE_BY_TAB);
    const next = tabAfterDisable("sql", visible);
    expect(next).toBe("run");
    expect(visible.some((t) => t.id === next)).toBe(true);
  });
});
