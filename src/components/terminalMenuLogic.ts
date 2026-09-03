//! Pure decisions for the titlebar's terminal split button — which rows its
//! menu holds, which of them are disabled and why, the counts they badge, and
//! what a saved command shortcut shows while its service is up — extracted so
//! they are testable without a DOM (vitest runs in the node environment). The
//! component only renders what these functions return.
//!
//! The button replaced four separate titlebar buttons (Launch, Apps, Running,
//! + Terminal). Folding them together is only safe if the menu keeps saying
//! what each of them said: a disabled row states *why* it is disabled rather
//! than silently doing nothing, and the two counts that used to sit on the
//! titlebar keep being visible as row badges.

import type { Launchable, RunningReport, ShellInfo } from "../ipc/types";
import { displayLabel } from "./launcherLogic";

/** What clicking a row does. The component maps these onto its handlers. */
export type TerminalMenuAction =
  | { kind: "newTerminal" }
  | { kind: "launcher" }
  | { kind: "running" }
  | { kind: "apps" }
  /**
   * Open a terminal in one specific detected shell, this once — it does not
   * change the stored preference (Settings does that). The whole `ShellInfo`
   * travels with the action rather than an id, so the resolved path the row
   * showed is the path that gets spawned: re-resolving a name at spawn time
   * would re-run the PATHEXT walk and could launch a different file.
   */
  | { kind: "newTerminalIn"; shell: ShellInfo }
  /** Start a saved shortcut. */
  | { kind: "runShortcut"; entry: Launchable }
  /**
   * Stop a running persistent shortcut. Every live key is carried, not just the
   * first: a service started twice is two processes, and stopping one of them
   * while the row goes back to "Run" would leave the other running invisibly.
   */
  | { kind: "stopShortcut"; entry: Launchable; keys: string[] };

/** One row of the menu. */
export interface TerminalMenuRow {
  /** Stable key for React, and what the tests address a row by. */
  id: string;
  label: string;
  /** A count shown after the label, or `null` for no badge. */
  badge: number | null;
  disabled: boolean;
  /** The tooltip — on a disabled row, the reason it cannot be used. */
  title: string;
  /** `null` exactly when the row is disabled: a dead row has nothing to do. */
  action: TerminalMenuAction | null;
  /** Draw a separator above this row. */
  separator: boolean;
  /**
   * A section heading to draw above this row, under the separator. Carried on
   * the row rather than derived by the component from the row's id, so the menu
   * stays a list the component walks without deciding anything.
   */
  section?: string;
  /**
   * Whether this shortcut has a process running right now. `undefined` for a
   * row where the question does not arise — deliberately not `false`, which
   * would read as "not running" and put a dead dot beside New Terminal.
   */
  live?: boolean;
}

/** Everything the menu is derived from. */
export interface TerminalMenuState {
  /** A codebase is in the foreground. New Terminal needs one. */
  workspaceOpen: boolean;
  /** How many processes are running across every open codebase. */
  runningCount: number;
  /** Open app-output tabs, and how many of those still have a live process. */
  appTabCount: number;
  liveAppCount: number;
  /** Saved shortcuts in display order; see {@link shortcutEntries}. */
  shortcuts: Launchable[];
  /** Live supervisor keys per launcher entry id; see {@link liveKeysByEntry}. */
  liveKeys: ReadonlyMap<string, string[]>;
  /**
   * The shortcut list has not been read yet. Distinct from "there are none":
   * showing an empty section while the read is in flight claims the user has
   * saved no shortcuts, which is a guess.
   */
  shortcutsLoading: boolean;
  /** The shells detected on this machine, in the backend's preference order. */
  shells: ShellInfo[];
  /**
   * Detection has not answered yet. The same *not read yet* vs *none* split as
   * {@link TerminalMenuState.shortcutsLoading}, for the same reason: an empty
   * shells section would report "no shell on this machine", which is a much
   * stronger claim than "we have not looked".
   */
  shellsLoading: boolean;
}

/** The heading above the shortcut rows, when there are any. */
export const SHORTCUTS_SECTION = "Commands";

/**
 * The heading above the one-off shell rows.
 *
 * The menu is a **flat** list — there is no submenu mechanism — so the shells
 * are their own section rather than a fly-out under New Terminal.
 */
export const SHELLS_SECTION = "New terminal in";

/**
 * The shortcuts to offer, this codebase's first.
 *
 * Reads `shortcut`, never `pinned`: the two are different facts (`pinned` only
 * sorts the picker), and the Rust side pins that a shortcut does not sort
 * ahead of a more recent entry. Each group's own order is left alone — it is
 * the backend's display order and re-sorting it here would disagree with the
 * picker for no reason.
 */
export function shortcutEntries(groups: {
  thisCodebase: Launchable[];
  global: Launchable[];
}): Launchable[] {
  return [
    ...groups.thisCodebase.filter((entry) => entry.shortcut),
    ...groups.global.filter((entry) => entry.shortcut),
  ];
}

/**
 * The live supervisor keys of each launcher entry, from the running report and
 * the app's own record of what it launched.
 *
 * The join has to be made here because a `RunningRecord` carries no launcher
 * id — only the key the app minted. Records of launches that have since exited
 * are harmless: nothing matches them in `report.live`, so they simply drop out.
 */
export function liveKeysByEntry(
  report: RunningReport | null,
  launches: ReadonlyMap<string, string>,
): Map<string, string[]> {
  const byEntry = new Map<string, string[]>();
  if (!report) return byEntry;
  for (const record of report.live) {
    if (record.kind !== "external") continue;
    const entryId = launches.get(record.key);
    if (entryId === undefined) continue;
    const keys = byEntry.get(entryId);
    if (keys) keys.push(record.key);
    else byEntry.set(entryId, [record.key]);
  }
  return byEntry;
}

/**
 * The body of the split button: New Terminal.
 *
 * Shares its rule with the menu row of the same name, so the button and the row
 * can never disagree about whether a terminal can be opened.
 */
export function newTerminalButton(state: {
  workspaceOpen: boolean;
}): { disabled: boolean; title: string } {
  return state.workspaceOpen
    ? { disabled: false, title: "Open a floating terminal in the active codebase" }
    : { disabled: true, title: "Open a codebase to run a terminal in it" };
}

/**
 * Whether a shortcut row offers Stop rather than Run.
 *
 * Only a **persistent** entry does. A persistent entry is declared to be one
 * long-running service, so a second copy is almost always a mistake; an
 * ordinary command is legitimately run twice (two workers, two ports), and
 * replacing its Run action while the first copy is alive would take away the
 * thing the row is for.
 */
export function offersStop(entry: Launchable, live: boolean): boolean {
  return live && entry.persistent;
}

/**
 * The shell rows, including the two rows that are *not* shells.
 *
 * Three distinct answers, never collapsed into two:
 *
 * - detection in flight ⇒ one disabled "Detecting shells…" row and **no** shell
 *   rows even if a previous list is still in state, because acting on a stale
 *   list is worse than waiting a moment for the fresh one;
 * - detection answered with nothing ⇒ one **disabled row carrying the reason**,
 *   not an omitted section. This is the `Project.unreadable` posture and
 *   deliberately unlike the switched-off-plugin rule: a plugin the user turned
 *   off needs no arguing with, whereas "we found no shell on this machine" is a
 *   machine fact they may need to act on, and a silently missing section is
 *   indistinguishable from a bug;
 * - a list ⇒ one row each, in the order the backend gave them (its preference
 *   order), with the resolved path as the tooltip — the only thing that tells
 *   two installations of the same shell apart.
 *
 * Every real shell row takes its `disabled` **and** its disabled reason from
 * {@link newTerminalButton}, the same rule the split button's body and the New
 * Terminal row use, so a shell row can never claim it can open a terminal when
 * New Terminal says it cannot. Enabled, the tooltip is the path instead: there
 * is no reason to state, and the path is the useful thing to show.
 *
 * No check-mark marks the preferred shell. These rows are one-off launches, and
 * marking one would suggest clicking it changes the default.
 */
function shellRows(state: TerminalMenuState): TerminalMenuRow[] {
  const open = newTerminalButton(state);
  if (state.shellsLoading) {
    return [
      {
        id: "shells.loading",
        label: "Detecting shells…",
        badge: null,
        disabled: true,
        title: "Looking for the shells installed on this machine",
        action: null,
        separator: true,
        section: SHELLS_SECTION,
      },
    ];
  }
  if (state.shells.length === 0) {
    return [
      {
        id: "shells.none",
        label: "No shells detected",
        badge: null,
        disabled: true,
        title: "No shells detected — new terminals use the system default",
        action: null,
        separator: true,
        section: SHELLS_SECTION,
      },
    ];
  }
  return state.shells.map((shell, index) => ({
    id: `shell:${shell.id}`,
    label: shell.label,
    badge: null,
    disabled: open.disabled,
    title: open.disabled ? open.title : shell.program,
    action: open.disabled ? null : { kind: "newTerminalIn", shell },
    separator: index === 0,
    ...(index === 0 ? { section: SHELLS_SECTION } : {}),
  }));
}

/** The whole menu, in display order. */
export function terminalMenuRows(state: TerminalMenuState): TerminalMenuRow[] {
  const rows: TerminalMenuRow[] = [];

  const terminal = newTerminalButton(state);
  rows.push({
    id: "terminal.new",
    label: "New Terminal",
    badge: null,
    disabled: terminal.disabled,
    title: terminal.title,
    action: terminal.disabled ? null : { kind: "newTerminal" },
    separator: false,
  });

  rows.push({
    id: "panel.launch",
    label: "Launch…",
    badge: null,
    disabled: false,
    title: "Run another app or command, and see what you have run before",
    action: { kind: "launcher" },
    separator: false,
  });

  rows.push({
    id: "panel.running",
    label: "Running",
    // Zero is shown as no badge, not as a "0": an empty badge would draw the
    // eye to the one state that needs no attention.
    badge: state.runningCount > 0 ? state.runningCount : null,
    disabled: false,
    title: "Show everything the app is running (and possible orphans)",
    action: { kind: "running" },
    separator: false,
  });

  const noApps = state.appTabCount === 0;
  rows.push({
    id: "panel.apps",
    label: "App output",
    badge: state.liveAppCount > 0 ? state.liveAppCount : null,
    disabled: noApps,
    title: noApps
      ? "Nothing launched yet — there is no output to show"
      : "Show the output of the apps you launched",
    action: noApps ? null : { kind: "apps" },
    separator: false,
  });

  // The shells sit between the base rows and the commands: they group with New
  // Terminal without pushing Launch/Running/App output under a heading.
  rows.push(...shellRows(state));

  if (state.shortcutsLoading) {
    rows.push({
      id: "shortcuts.loading",
      label: "Reading commands…",
      badge: null,
      disabled: true,
      title: "Reading the saved commands",
      action: null,
      separator: true,
      section: SHORTCUTS_SECTION,
    });
    return rows;
  }

  // No shortcuts means no separator either: a trailing rule under the last row
  // promises a section that never arrives.
  state.shortcuts.forEach((entry, index) => {
    const keys = state.liveKeys.get(entry.id) ?? [];
    const live = keys.length > 0;
    const stop = offersStop(entry, live);
    rows.push({
      id: `shortcut:${entry.id}`,
      label: displayLabel(entry),
      badge: null,
      disabled: false,
      title: stop ? `Stop ${entry.command}` : `Run ${entry.command}`,
      action: stop
        ? { kind: "stopShortcut", entry, keys }
        : { kind: "runShortcut", entry },
      separator: index === 0,
      ...(index === 0 ? { section: SHORTCUTS_SECTION } : {}),
      live,
    });
  });

  return rows;
}

/** What a shortcut row's action button reads. */
export function shortcutActionLabel(row: TerminalMenuRow): string {
  return row.action?.kind === "stopShortcut" ? "Stop" : "Run";
}
