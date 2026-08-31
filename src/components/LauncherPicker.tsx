import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../ipc/api";
import type { Launchable, LauncherGroups } from "../ipc/types";
import {
  canRun,
  displayLabel,
  filterGroups,
  moveSelection,
  needsShell,
  pickerKeyAction,
  pickerRows,
  shortCwd,
} from "./launcherLogic";

/**
 * The app launcher: a command box plus the commands you have run before.
 *
 * An overlay opened from the titlebar, deliberately **not** a tab: what it runs
 * belongs to no project (a local Redis, a Python script, `docker compose up`),
 * which is exactly why it cannot be a run configuration — see
 * `cb_core::launcher`. Typing a command and pressing Enter runs it and remembers
 * it; there is no separate "add an entry" form.
 *
 * Every decision is in the pure, tested `launcherLogic`: whether the shell
 * checkbox starts ticked, what a row is called, the filter, and the key table.
 */
export function LauncherPicker({
  root,
  onLaunch,
  onClose,
}: {
  /** The active codebase root, for the cwd default and the "this codebase" hint. */
  root: string | null;
  /** Run a command; the app owns the process and its output tab. */
  onLaunch: (spec: { command: string; cwd: string; shell: boolean; label?: string }) => void;
  onClose: () => void;
}) {
  const [groups, setGroups] = useState<LauncherGroups>({ thisCodebase: [], global: [] });
  const [command, setCommand] = useState("");
  const [cwd, setCwd] = useState(root ?? "");
  // Ticked automatically for a command line that needs a shell, but still a
  // checkbox: the user may have quoted something we read as a metacharacter, and
  // the backend refuses rather than guessing either way.
  const [shell, setShell] = useState(false);
  const [shellTouched, setShellTouched] = useState(false);
  const [selected, setSelected] = useState(-1);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const refresh = () => {
    api
      .listLaunchables()
      .then(setGroups)
      .catch((e) => setError(String(e)));
  };

  // Read once on open: the store is small and the picker is short-lived.
  useEffect(() => {
    refresh();
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setCwd(root ?? "");
  }, [root]);

  // The checkbox follows the command line until the user overrides it.
  useEffect(() => {
    if (!shellTouched) setShell(needsShell(command));
  }, [command, shellTouched]);

  const filtered = useMemo(() => filterGroups(groups, command), [groups, command]);
  const rows = useMemo(() => pickerRows(filtered), [filtered]);

  const runTyped = () => {
    if (!canRun(command)) return;
    onLaunch({ command: command.trim(), cwd: cwd.trim() || (root ?? ""), shell });
    onClose();
  };

  const runEntry = (entry: Launchable) => {
    onLaunch({
      command: entry.command,
      cwd: entry.cwd,
      shell: entry.shell,
      label: entry.label ?? undefined,
    });
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const action = pickerKeyAction(e.key);
    if (!action) return;
    e.preventDefault();
    switch (action) {
      case "close":
        onClose();
        return;
      case "next":
        setSelected(moveSelection(rows, selected, 1));
        return;
      case "prev":
        setSelected(moveSelection(rows, selected, -1));
        return;
      case "run": {
        const row = rows[selected];
        if (row) runEntry(row.entry);
        else runTyped();
        return;
      }
    }
  };

  const pin = (entry: Launchable) => {
    api
      .saveLaunchable(entry.id, { pinned: !entry.pinned })
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const rename = (entry: Launchable) => {
    const next = window.prompt("Name this command", displayLabel(entry));
    if (next === null) return;
    api
      .saveLaunchable(entry.id, { label: next })
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const forget = (entry: Launchable) => {
    api
      .deleteLaunchable(entry.id)
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const section = (title: string, entries: Launchable[], offset: number) => {
    if (entries.length === 0) return null;
    return (
      <div className="launcher-section">
        <div className="launcher-section-title">{title}</div>
        {entries.map((entry, i) => {
          const index = offset + i;
          const hint = shortCwd(entry.cwd, root);
          return (
            <div
              className={`launcher-row${index === selected ? " selected" : ""}`}
              key={entry.id}
              onMouseEnter={() => setSelected(index)}
              onDoubleClick={() => runEntry(entry)}
            >
              <button
                className="launcher-run"
                title={`Run ${entry.command}`}
                onClick={() => runEntry(entry)}
              >
                ▶
              </button>
              <span className="launcher-label" title={entry.command}>
                {displayLabel(entry)}
              </span>
              {hint !== "" && <span className="launcher-meta">{hint}</span>}
              {entry.shell && (
                <span className="launcher-meta" title="Runs through the shell">
                  shell
                </span>
              )}
              <button
                className={`launcher-icon${entry.pinned ? " on" : ""}`}
                title={entry.pinned ? "Unpin" : "Pin to the top"}
                onClick={() => pin(entry)}
              >
                ★
              </button>
              <button className="launcher-icon" title="Rename" onClick={() => rename(entry)}>
                ✎
              </button>
              <button className="launcher-icon" title="Forget" onClick={() => forget(entry)}>
                ✕
              </button>
            </div>
          );
        })}
      </div>
    );
  };

  const hasRows = rows.length > 0;

  return (
    <div className="launcher-overlay" onMouseDown={onClose}>
      <div className="launcher" onMouseDown={(e) => e.stopPropagation()} onKeyDown={onKeyDown}>
        <div className="launcher-input-row">
          <input
            ref={inputRef}
            className="launcher-input"
            placeholder="Command to run — e.g. docker compose up"
            value={command}
            onChange={(e) => {
              setCommand(e.target.value);
              setSelected(-1);
            }}
          />
          <button className="launcher-go" disabled={!canRun(command)} onClick={runTyped}>
            Run
          </button>
        </div>
        <div className="launcher-options">
          <input
            className="launcher-cwd"
            placeholder="Working directory"
            value={cwd}
            onChange={(e) => setCwd(e.target.value)}
            title="Where the command runs. Defaults to the open codebase."
          />
          <label className="launcher-shell" title="Needed for | > &amp;&amp; and other shell syntax">
            <input
              type="checkbox"
              checked={shell}
              onChange={(e) => {
                setShellTouched(true);
                setShell(e.target.checked);
              }}
            />
            run through shell
          </label>
        </div>

        {error !== null && <div className="launcher-error">{error}</div>}

        <div className="launcher-list">
          {hasRows ? (
            <>
              {section("This codebase", filtered.thisCodebase, 0)}
              {section("All commands", filtered.global, filtered.thisCodebase.length)}
            </>
          ) : (
            <div className="launcher-empty">
              {command.trim() === ""
                ? "Nothing run yet — type a command and press Enter."
                : "No remembered command matches. Press Enter to run what you typed."}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
