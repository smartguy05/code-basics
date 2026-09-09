//! Pure decisions for the Running panel — the per-kind label/icon, the codebase
//! name shown per row, the age string, the live count badge, and the shape of a
//! kill request — extracted so they are testable without a DOM (vitest runs in
//! the node environment). The panel component only renders.

import type { ProcessKind, RunningRecord, RunningReport } from "../ipc/types";

/** A short glyph per kind, for the row marker. */
export function kindIcon(kind: ProcessKind): string {
  switch (kind) {
    case "run":
      return "▶";
    case "build":
      return "⚙";
    case "terminal":
      return "▷";
    case "review":
      return "🔍";
    case "behavioral":
      return "⇄";
    case "external":
      return "⚡";
  }
}

/** A human word per kind, for a tooltip / secondary label. */
export function kindLabel(kind: ProcessKind): string {
  switch (kind) {
    case "run":
      return "Run";
    case "build":
      return "Build";
    case "terminal":
      return "Terminal";
    case "review":
      return "Review";
    case "behavioral":
      return "Behavioral";
    case "external":
      return "App";
  }
}

/**
 * Whether the panel can show this process's output.
 *
 * Only a launched app can: its console lives in the shared output panel, which
 * the Running panel focuses by key. A configuration run's console belongs to the
 * Run tab of its codebase, a terminal *is* its own window, and a review or
 * behavioral run has its own panel — offering the action for those would give
 * the user a button that does nothing.
 */
export function hasOutput(record: RunningRecord): boolean {
  return record.kind === "external";
}

/**
 * The codebase name shown per row: the last segment of the root path, tolerant
 * of both `/` and `\` separators and a trailing slash. Falls back to the whole
 * string when there is no segment (e.g. a bare drive or an empty root).
 */
export function rootBasename(root: string): string {
  const trimmed = root.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/).filter((p) => p !== "");
  const last = parts[parts.length - 1];
  return last !== undefined ? last : root;
}

/**
 * A compact age like `4s`, `3m`, or `2h 5m` from a start time and the current
 * clock (both injected, so the result is testable). Never negative — a clock
 * skew that puts the start in the future reads as `0s`.
 */
export function formatAge(startedAtMs: number, nowMs: number): string {
  const totalSec = Math.max(0, Math.floor((nowMs - startedAtMs) / 1000));
  if (totalSec < 60) return `${totalSec}s`;
  const totalMin = Math.floor(totalSec / 60);
  if (totalMin < 60) return `${totalMin}m`;
  const hours = Math.floor(totalMin / 60);
  const minutes = totalMin % 60;
  return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
}

/** The number for the titlebar badge: how many processes are running now. */
export function liveCount(report: RunningReport | null): number {
  return report?.live.length ?? 0;
}

/** Whether the panel has anything at all to show. */
export function isEmpty(report: RunningReport | null): boolean {
  if (!report) return true;
  return report.live.length === 0 && report.orphans.length === 0;
}

/** The argument shape `api.killRunning` expects, built from a record. */
export interface KillRequest {
  pid: number;
  kind: ProcessKind;
  root: string;
  key: string;
  orphan: boolean;
}

/** Build the kill request for a record; `orphan` says which section it came from. */
export function killRequest(record: RunningRecord, orphan: boolean): KillRequest {
  return {
    pid: record.pid,
    kind: record.kind,
    root: record.root,
    key: record.key,
    orphan,
  };
}

// ---------------------------------------------------------------------------
// The Stop button's menu (Run tab)
// ---------------------------------------------------------------------------

/** The Stop split-button manages only configurations launched by Run. */
const STOP_MENU_KIND: ProcessKind = "run";

/** One stoppable process in the Stop menu. */
export interface StopMenuRow {
  record: RunningRecord;
  /** From `RunningReport.orphans` — killing it needs the extra confirmation. */
  orphan: boolean;
  /** Started from the codebase whose Run tab this menu belongs to. */
  here: boolean;
}

/** A labelled block of rows in the Stop menu. */
export interface StopMenuGroup {
  key: string;
  label: string;
  rows: StopMenuRow[];
}

/**
 * Compare two workspace roots.
 *
 * Tolerant of both separators and a trailing one, and case-insensitive, because
 * the roots being compared come from different places — the open workspace, and
 * whatever the supervisor recorded when the process started — and on Windows
 * those routinely differ in case and in how they spell the separator. Getting
 * this wrong only mis-sorts the menu, so tolerance costs nothing here; it is not
 * doing security work, which is why it can be looser than
 * `symbols::index::relative_to_root`, which refuses to guess.
 */
export function sameRoot(a: string, b: string): boolean {
  const normalise = (root: string) =>
    root
      .replace(/[\\/]+$/, "")
      .replace(/\\/g, "/")
      .toLowerCase();
  return normalise(a) === normalise(b);
}

/**
 * Everything the Stop menu lists: Run-launched configurations, ordered so the
 * open codebase's processes come first.
 *
 * Runs from every codebase are shown: the Stop button exists
 * because a process the user has lost track of is hard to find, and the ones
 * hardest to find are precisely the ones started from a tab that is no longer
 * in front of them. Rows from elsewhere are marked (`here: false`) so the UI can
 * say where they came from rather than presenting them as local.
 *
 * Run orphans — processes the app started and can no longer address normally — are
 * a group of their own at the end rather than being mixed in, because killing
 * one is a different action with a different confirmation.
 */
export function stopMenuGroups(
  report: RunningReport | null,
  activeRoot: string,
): StopMenuGroup[] {
  if (!report) return [];

  const row = (record: RunningRecord, orphan: boolean): StopMenuRow => ({
    record,
    orphan,
    here: sameRoot(record.root, activeRoot),
  });

  // This codebase first, then by label, so the list is stable between polls —
  // the backend's order is enumeration order and is not something to rely on.
  const order = (rows: StopMenuRow[]) =>
    [...rows].sort(
      (a, b) =>
        Number(b.here) - Number(a.here) ||
        a.record.label.localeCompare(b.record.label) ||
        a.record.pid - b.record.pid,
    );

  const groups: StopMenuGroup[] = [];
  const rows = order(
    report.live.filter((r) => r.kind === STOP_MENU_KIND).map((r) => row(r, false)),
  );
  if (rows.length > 0) {
    groups.push({ key: STOP_MENU_KIND, label: kindLabel(STOP_MENU_KIND), rows });
  }

  const orphans = order(
    report.orphans.filter((r) => r.kind === STOP_MENU_KIND).map((r) => row(r, true)),
  );
  if (orphans.length > 0) {
    groups.push({ key: "orphans", label: "Left over", rows: orphans });
  }

  return groups;
}

/** How many rows the menu holds — the count beside the button, and the empty test. */
export function stopMenuCount(groups: StopMenuGroup[]): number {
  return groups.reduce((total, group) => total + group.rows.length, 0);
}

/**
 * What one row says: the process, and where it came from when that is not here.
 *
 * The codebase is named only for a process from somewhere else. Repeating the
 * open codebase's name on every local row would bury the one piece of
 * information the row exists to carry.
 */
export function stopRowLabel(row: StopMenuRow): string {
  return row.here ? row.record.label : `${row.record.label} — ${rootBasename(row.record.root)}`;
}
