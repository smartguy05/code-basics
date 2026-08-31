//! Pure decisions for the launched-apps output panel — the tab list, which tab
//! is active after a close, what a process event does to a tab's status, and how
//! that status reads — extracted so they are testable without a DOM (vitest runs
//! in the node environment). `AppOutputPanel.tsx` only renders.
//!
//! One rule shapes the rest: a tab **outlives its process**. The Running panel
//! drops a row the instant the process exits (that is the registry's behaviour,
//! not this panel's), so after an exit this tab is the only place the exit code
//! and the output still exist. Nothing here removes a tab on exit.

import type { LaunchedApp, ProcessEvent } from "../ipc/types";
import type { Severity } from "./consoleLogic";

/** Where a launched app has got to. */
export type AppStatus =
  | { kind: "running" }
  | {
      kind: "exited";
      /** `null` for a signalled or cancelled process — not the same as 0. */
      code: number | null;
      success: boolean;
      cancelled: boolean;
    }
  | { kind: "failed"; message: string };

/** One tab: a launched app, its console, and where it got to. */
export interface AppTab {
  /** The supervisor key — this tab's identity, and what Stop addresses. */
  key: string;
  /** The recents entry it was launched from, for pin/rename from the panel. */
  entryId: string;
  label: string;
  cwd: string;
  /** Workspace that owned the launcher when this command was started. */
  workspaceRoot: string | null;
  /** Known once the `started` event arrives; `null` before that and if absent. */
  pid: number | null;
  status: AppStatus;
  /**
   * The severity threshold this tab's console is filtered to.
   *
   * Per tab, not per panel: two apps running at once are usually being watched
   * for different reasons, and narrowing one to its failures should not blind
   * you to the other. Stored here rather than inside the console component so
   * it survives the panel being hidden — the consoles stay mounted, but the
   * setting is a property of the app being watched, not of a DOM node.
   */
  severity: Severity;
}

/** The localStorage key the panel persists its shared layout under. */
export const APP_OUTPUT_LAYOUT_KEY = "cb.launcher.layout";

/** A fresh tab for a just-launched app. */
export function makeTab(app: LaunchedApp, workspaceRoot: string | null = null): AppTab {
  return {
    key: app.key,
    entryId: app.id,
    label: app.label,
    cwd: app.cwd,
    workspaceRoot,
    pid: null,
    status: { kind: "running" },
    severity: "all",
  };
}

/** Append a tab and focus it. */
export function addTab(tabs: AppTab[], tab: AppTab): { tabs: AppTab[]; activeKey: string } {
  return { tabs: [...tabs, tab], activeKey: tab.key };
}

/**
 * Close one tab. When the closing tab was active the focus moves to its
 * neighbour — the next one, or the previous when it was last — so the panel
 * never lands on nothing while tabs remain.
 */
export function closeTab(
  tabs: AppTab[],
  key: string,
  activeKey: string | null,
): { tabs: AppTab[]; activeKey: string | null } {
  const index = tabs.findIndex((t) => t.key === key);
  const remaining = tabs.filter((t) => t.key !== key);
  if (remaining.length === 0) return { tabs: remaining, activeKey: null };
  if (activeKey !== key) return { tabs: remaining, activeKey };
  const next = remaining[Math.min(Math.max(index, 0), remaining.length - 1)];
  return { tabs: remaining, activeKey: next ? next.key : null };
}

/**
 * Fold a process event into the tab it belongs to. Output events change no
 * status (the console renders those itself), and an event for a tab that is gone
 * is ignored rather than resurrecting it.
 */
export function applyEvent(tabs: AppTab[], key: string, event: ProcessEvent): AppTab[] {
  if (event.type === "output") return tabs;
  // Aliased to a `const` so the narrowing above survives into the closure below:
  // TypeScript resets a *parameter's* narrowing inside a nested function, which
  // would leave `output` unhandled there and the mapped value possibly undefined.
  const settled = event;
  return tabs.map((tab) => {
    if (tab.key !== key) return tab;
    switch (settled.type) {
      case "started":
        return { ...tab, pid: settled.pid };
      case "exited":
        return {
          ...tab,
          status: {
            kind: "exited",
            code: settled.code,
            success: settled.success,
            cancelled: settled.cancelled,
          },
        };
      case "failed":
        return { ...tab, status: { kind: "failed", message: settled.message } };
    }
  });
}

/**
 * How a status reads on the tab strip. A cancelled process says `stopped` (the
 * user asked), a signalled one says `exited` with no number (there is no code —
 * reporting 0 would claim a clean exit), and a spawn failure names the reason.
 */
export function statusText(status: AppStatus): string {
  switch (status.kind) {
    case "running":
      return "running";
    case "exited":
      if (status.cancelled) return "stopped";
      return status.code === null ? "exited" : `exited ${status.code}`;
    case "failed":
      return `failed: ${status.message}`;
  }
}

/** Whether Stop applies — only to a process that is still running. */
export function canStop(tab: AppTab): boolean {
  return tab.status.kind === "running";
}

/** How many tabs are still live, for the panel header. */
export function liveTabCount(tabs: AppTab[]): number {
  return tabs.filter(canStop).length;
}

/**
 * The tab's title, numbered when its label repeats. Running the same command
 * twice is a legitimate thing to do (two workers, two ports) and gives two
 * identically labelled tabs, which would otherwise be indistinguishable.
 */
export function tabTitle(tabs: AppTab[], tab: AppTab): string {
  const sameLabel = tabs.filter((t) => t.label === tab.label);
  if (sameLabel.length < 2) return tab.label;
  const position = sameLabel.findIndex((t) => t.key === tab.key) + 1;
  return `${tab.label} (${position})`;
}

/**
 * Set one tab's severity threshold.
 *
 * Returns the same array when nothing changes, so a re-selection of the level
 * already showing does not re-render every console in the panel.
 */
export function setTabSeverity(tabs: AppTab[], key: string, severity: Severity): AppTab[] {
  if (!tabs.some((t) => t.key === key && t.severity !== severity)) return tabs;
  return tabs.map((t) => (t.key === key ? { ...t, severity } : t));
}
