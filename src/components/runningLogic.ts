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
  }
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
