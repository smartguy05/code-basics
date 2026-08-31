//! Pure decisions for the app launcher's picker — whether a command line needs
//! a shell, what a remembered entry is called, filtering, and keyboard
//! navigation — extracted so they are testable without a DOM (vitest runs in the
//! node environment). `LauncherPicker.tsx` only renders.

import type { Launchable, LauncherGroups } from "../ipc/types";

/** Characters that only mean something to a shell (`launcher::SHELL_SPECIALS`). */
const SHELL_SPECIALS = new Set(["|", ">", "<", "&", ";"]);

/**
 * Whether a command line needs a shell to mean what it looks like.
 *
 * This is the **default for the checkbox**, not the enforcement — the backend
 * refuses an unquoted metacharacter outright (`launcher::split_command`), because
 * a bare argv spawn would hand `|` to the program as an ordinary argument and
 * quietly do something else. The two must agree about what counts, so the
 * quoting rules here mirror the Rust tokeniser exactly: only `"` groups, only
 * `\"` escapes (a Windows path is full of backslashes that are not escapes), and
 * a metacharacter inside quotes is just text.
 */
export function needsShell(command: string): boolean {
  let inQuotes = false;
  const chars = Array.from(command);
  for (let i = 0; i < chars.length; i += 1) {
    const char = chars[i];
    if (char === undefined) continue;
    if (char === "\\" && chars[i + 1] === '"') {
      i += 1;
      continue;
    }
    if (char === '"') {
      inQuotes = !inQuotes;
      continue;
    }
    if (!inQuotes && SHELL_SPECIALS.has(char)) return true;
  }
  return false;
}

/**
 * What to show for an entry: the user's rename, else the command line. Never
 * blank — a row with no text would be unreadable and unclickable.
 */
export function displayLabel(entry: Launchable): string {
  const renamed = entry.label?.trim();
  return renamed && renamed.length > 0 ? renamed : entry.command.trim();
}

/** Whether the command box holds something runnable. */
export function canRun(command: string): boolean {
  return command.trim().length > 0;
}

/** Separators normalised and any trailing one dropped, for comparing paths. */
function normalise(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

/**
 * The `cwd` hint shown beside an entry: nothing when it runs at the codebase
 * root (the common case, where saying so is noise), the relative path when it
 * runs inside it, and the whole path otherwise.
 *
 * Compared case-insensitively: this is a display hint on a Windows-first app,
 * where the root the backend scanned and a path the user typed differ in case
 * routinely. The trailing `/` on the root is what keeps `/repo2` out of `/repo`.
 */
export function shortCwd(cwd: string, root: string | null): string {
  if (!root) return cwd;
  const normalisedRoot = normalise(root);
  if (normalisedRoot === "") return cwd;
  const normalisedCwd = normalise(cwd);
  const lowerRoot = normalisedRoot.toLowerCase();
  const lowerCwd = normalisedCwd.toLowerCase();
  if (lowerCwd === lowerRoot) return "";
  if (lowerCwd.startsWith(`${lowerRoot}/`)) {
    return normalisedCwd.slice(normalisedRoot.length + 1);
  }
  return cwd;
}

/** Filter both groups by a query matched against the command and the rename. */
export function filterGroups(groups: LauncherGroups, query: string): LauncherGroups {
  const needle = query.trim().toLowerCase();
  if (needle === "") return groups;
  const matches = (entry: Launchable) =>
    entry.command.toLowerCase().includes(needle) ||
    (entry.label ?? "").toLowerCase().includes(needle);
  return {
    thisCodebase: groups.thisCodebase.filter(matches),
    global: groups.global.filter(matches),
  };
}

/** Which group a flattened row came from, so the picker can label the section. */
export type PickerGroup = "thisCodebase" | "global";

/** One navigable row of the picker. */
export interface PickerRow {
  entry: Launchable;
  group: PickerGroup;
}

/** Both groups flattened in display order: this codebase first. */
export function pickerRows(groups: LauncherGroups): PickerRow[] {
  return [
    ...groups.thisCodebase.map((entry) => ({ entry, group: "thisCodebase" as const })),
    ...groups.global.map((entry) => ({ entry, group: "global" as const })),
  ];
}

/**
 * Move the selection by `delta`, clamped to the list. An index the filter has
 * invalidated snaps to the first row rather than addressing a row that is no
 * longer there; an empty list has no selection (`-1`).
 */
export function moveSelection(rows: PickerRow[], current: number, delta: number): number {
  if (rows.length === 0) return -1;
  if (current < 0 || current >= rows.length) return 0;
  return Math.min(rows.length - 1, Math.max(0, current + delta));
}

/** What the picker does with a key press, or `null` to let the key through. */
export type PickerAction = "run" | "close" | "next" | "prev";

/** The whole key table as one expression, in the `searchLogic` style. */
export function pickerKeyAction(key: string): PickerAction | null {
  switch (key) {
    case "Enter":
      return "run";
    case "Escape":
      return "close";
    case "ArrowDown":
      return "next";
    case "ArrowUp":
      return "prev";
    default:
      return null;
  }
}
