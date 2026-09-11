import { useEffect, useMemo, useRef, useState } from "react";
import { applyAppearance, loadAppearance, validThemeColors } from "../appearance";
import {
  activeTheme, allThemes, BUILTIN_THEMES, clampUnfocusedOpacity, clampWindowOpacity, COLOR_KEYS, DEFAULT_APPEARANCE,
  parseThemeFile,
  type AppearanceSettings, type ThemeDefinition,
} from "../appearanceLogic";
import { transparencySupport } from "../windowTransparencyLogic";
import {
  COMMANDS, chordFromEvent, commandSections, conflictingCommand, effectiveBinding, formatChord,
  type CommandDefinition, type ShortcutOverrides,
} from "../shortcutLogic";
import { loadShortcutOverrides, saveShortcutOverrides } from "../shortcuts";
import {
  DEFAULT_TERMINAL_SHELL, loadTerminalShell, missingShellNotice, saveTerminalShell,
  type TerminalShellPref,
} from "./terminalShellLogic";
import * as api from "../ipc/api";
import type { DetectedShells } from "../ipc/types";

import { McpToolsPage } from "./McpToolsPage";

type Page = "appearance" | "terminal" | "mcp" | "keyboard" | "reference";
const cloneAppearance = (value: AppearanceSettings): AppearanceSettings => JSON.parse(JSON.stringify(value)) as AppearanceSettings;

function downloadTheme(theme: ThemeDefinition) {
  const blob = new Blob([JSON.stringify({ version: 1, theme }, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `${theme.name.replace(/[^a-z0-9]+/gi, "-").toLowerCase() || "theme"}.code-basics-theme.json`;
  link.click();
  URL.revokeObjectURL(url);
}

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const originalAppearance = useRef(loadAppearance());
  const [appearance, setAppearance] = useState(() => cloneAppearance(originalAppearance.current));
  const [shortcuts, setShortcuts] = useState<ShortcutOverrides>(() => loadShortcutOverrides());
  const [page, setPage] = useState<Page>("appearance");
  /**
   * Whether this platform can show anything behind the window. `null` until
   * `about_info` answers, which `transparencySupport` reads as unsupported with
   * nothing to say — so the slider is briefly disabled and silent rather than
   * accusing the platform of something before we have looked.
   */
  const [os, setOs] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void api.aboutInfo().then((info) => { if (live) setOs(info.os); }).catch(() => {});
    return () => { live = false; };
  }, []);
  const transparency = transparencySupport(os);
  const [query, setQuery] = useState("");
  const [recording, setRecording] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const importRef = useRef<HTMLInputElement>(null);
  // The remembered shell for new terminals, buffered like the appearance draft.
  const [shell, setShell] = useState<TerminalShellPref>(() => loadTerminalShell(localStorage));
  /**
   * What the machine has. `null` is "not read yet", never "none found" — an
   * empty select would claim we looked and found nothing.
   *
   * **This is the first asynchronously loaded surface in this dialog**, which is
   * otherwise entirely synchronous reads of `localStorage`, so it is the first
   * place here that has a loading state to render at all.
   */
  const [shells, setShells] = useState<DetectedShells | null>(null);
  useEffect(() => {
    let live = true;
    void api.listShells()
      .then((found) => { if (live) setShells(found); })
      // A detection failure leaves the list unread rather than claiming an empty
      // machine: "Detecting shells…" is wrong for only as long as the dialog is
      // open, while "no shells found" would be a fabricated machine fact.
      .catch((e) => console.error("code-basics: shell detection failed", e));
    return () => { live = false; };
  }, []);
  const shellList = shells?.shells ?? null;
  const defaultShellLabel = shells?.shells.find((s) => s.id === shells.defaultId)?.label;
  const shellMissing = missingShellNotice(shell, shellList);

  const selected = activeTheme(appearance);
  const selectedBuiltin = BUILTIN_THEMES.some((theme) => theme.id === selected.id);
  const commands = useMemo(() => COMMANDS.filter((command) => `${command.category} ${command.label}`.toLowerCase().includes(query.toLowerCase())), [query]);
  // Split after filtering, so searching narrows the blocks rather than
  // rearranging them, and a block with no match disappears entirely.
  const sections = useMemo(() => commandSections(commands), [commands]);

  function preview(next: AppearanceSettings) {
    setAppearance(next);
    applyAppearance(next, false);
  }

  function updateTheme(update: (theme: ThemeDefinition) => ThemeDefinition) {
    if (selectedBuiltin) return;
    preview({ ...appearance, customThemes: appearance.customThemes.map((theme) => theme.id === selected.id ? update(theme) : theme) });
  }

  function duplicateTheme() {
    const copy: ThemeDefinition = { ...selected, id: crypto.randomUUID(), name: `${selected.name} Copy`, colors: { ...selected.colors }, fonts: { ...selected.fonts } };
    preview({ ...appearance, activeThemeId: copy.id, customThemes: [...appearance.customThemes, copy] });
  }

  function removeTheme() {
    if (selectedBuiltin) return;
    preview({ ...appearance, activeThemeId: BUILTIN_THEMES[0]!.id, customThemes: appearance.customThemes.filter((theme) => theme.id !== selected.id) });
  }

  function record(command: CommandDefinition, event: React.KeyboardEvent<HTMLButtonElement>) {
    event.preventDefault(); event.stopPropagation();
    if (event.key === "Escape") { setRecording(null); return; }
    if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return;
    const next = chordFromEvent(event.nativeEvent);
    if (next.key.length === 1 && !next.ctrl && !next.alt && !next.meta) {
      setMessage("Printable keys require Ctrl, Alt, or Cmd so typing remains safe."); return;
    }
    const conflict = conflictingCommand(command, next, shortcuts);
    if (conflict) { setMessage(`${formatChord(next)} is already assigned to ${conflict.label} in this context.`); return; }
    setShortcuts({ ...shortcuts, [command.id]: next });
    setRecording(null); setMessage(null);
  }

  async function importTheme(file: File) {
    const imported = parseThemeFile(await file.text());
    if (!imported || !COLOR_KEYS.every((key) => CSS.supports("color", imported.colors[key]))) {
      setMessage("That file is not a valid Code Basics theme."); return;
    }
    const copy = { ...imported, id: crypto.randomUUID(), name: allThemes(appearance).some((theme) => theme.name === imported.name) ? `${imported.name} (Imported)` : imported.name };
    preview({ ...appearance, activeThemeId: copy.id, customThemes: [...appearance.customThemes, copy] });
    setMessage(null);
  }

  function cancel() { applyAppearance(originalAppearance.current, false); onClose(); }
  function apply() {
    if (!validThemeColors(appearance)) { setMessage("Every color must be a valid CSS color."); return; }
    applyAppearance(appearance, true); saveShortcutOverrides(shortcuts); saveTerminalShell(localStorage, shell); onClose();
  }

  return <div className="settings-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) cancel(); }}>
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-label="Settings">
      <header><h2>Settings</h2><button onClick={cancel} title="Cancel">✕</button></header>
      <div className="settings-body">
        <nav>
          <button className={page === "appearance" ? "active" : ""} onClick={() => setPage("appearance")}>Appearance</button>
          <button className={page === "terminal" ? "active" : ""} onClick={() => setPage("terminal")}>Terminal</button>
          <button className={page === "mcp" ? "active" : ""} onClick={() => setPage("mcp")}>MCP tools</button>
          <button className={page === "keyboard" ? "active" : ""} onClick={() => setPage("keyboard")}>Keyboard</button>
          <button className={page === "reference" ? "active" : ""} onClick={() => setPage("reference")}>Native keys</button>
        </nav>
        <main>
          {page === "appearance" && <>
            <div className="settings-row"><label>Theme</label><select value={appearance.activeThemeId} onChange={(event) => preview({ ...appearance, activeThemeId: event.target.value })}>{allThemes(appearance).map((theme) => <option key={theme.id} value={theme.id}>{theme.name}</option>)}</select></div>
            <div className="settings-actions"><button onClick={duplicateTheme}>Duplicate</button><button disabled={selectedBuiltin} onClick={removeTheme}>Delete</button><button onClick={() => importRef.current?.click()}>Import…</button><button onClick={() => downloadTheme(selected)}>Export…</button></div>
            <input ref={importRef} type="file" accept="application/json,.json" hidden onChange={(event) => { const file = event.target.files?.[0]; if (file) void importTheme(file); event.target.value = ""; }} />
            {!selectedBuiltin && <><div className="settings-row"><label>Name</label><input value={selected.name} onChange={(event) => updateTheme((theme) => ({ ...theme, name: event.target.value }))} /></div><div className="settings-row"><label>Control style</label><select value={selected.mode} onChange={(event) => updateTheme((theme) => ({ ...theme, mode: event.target.value as "dark" | "light" }))}><option value="dark">Dark</option><option value="light">Light</option></select></div></>}
            <div className="settings-grid fonts">
              <label>UI font<input disabled={selectedBuiltin} value={selected.fonts.ui} onChange={(event) => updateTheme((theme) => ({ ...theme, fonts: { ...theme.fonts, ui: event.target.value } }))} /></label>
              <label>Code font<input disabled={selectedBuiltin} value={selected.fonts.code} onChange={(event) => updateTheme((theme) => ({ ...theme, fonts: { ...theme.fonts, code: event.target.value } }))} /></label>
              <label>UI size<input type="number" min={10} max={24} step={1} value={appearance.uiFontSize} onChange={(event) => preview({ ...appearance, uiFontSize: Math.min(24, Math.max(10, Number(event.target.value))) })} /></label>
              <label>Code size<input type="number" min={8} max={32} step={0.5} value={appearance.codeFontSize} onChange={(event) => preview({ ...appearance, codeFontSize: Math.min(32, Math.max(8, Number(event.target.value))) })} /></label>
            </div>
            <h3>Window</h3>
            {/* The one appearance control that a *runtime* condition also gates:
                the window is only translucent while the active codebase's editor
                area is empty. `windowTransparencyLogic` owns that rule; this
                just stores the number. Disabled-with-a-reason rather than hidden
                on a platform that cannot do it — "not available here, and why"
                beats a control that silently does nothing. */}
            <div className="settings-row">
              <label>Opacity when nothing is open</label>
              <input
                type="range"
                min={30}
                max={100}
                step={5}
                disabled={!transparency.supported}
                value={appearance.windowOpacity}
                onChange={(event) => preview({ ...appearance, windowOpacity: clampWindowOpacity(Number(event.target.value)) })}
              />
              <span className="muted">{appearance.windowOpacity}%</span>
            </div>
            {transparency.reason !== null && <div className="warning">{transparency.reason}</div>}
            <p>Applies only while no file or diff is open. Terminals, Notes and the other floating panels stay opaque.</p>
            {/* A separate control from the window opacity above: this fades an
                individual editor or terminal while it is not focused, so the one
                being typed in stands out. Not runtime-gated, so it previews and
                applies through `applyAppearance` like the fonts. */}
            <div className="settings-row">
              <label>Unfocused editor/terminal opacity</label>
              <input
                type="range"
                min={30}
                max={100}
                step={5}
                value={appearance.unfocusedOpacity}
                onChange={(event) => preview({ ...appearance, unfocusedOpacity: clampUnfocusedOpacity(Number(event.target.value)) })}
              />
              <span className="muted">{appearance.unfocusedOpacity}%</span>
            </div>
            <p>Fades a file editor or terminal while it does not have focus. The focused one is always fully opaque; 100% turns the effect off.</p>
            <h3>Colors</h3>
            <div className="settings-grid colors">{COLOR_KEYS.map((key) => <label key={key}>{key}<span><input type="color" disabled={selectedBuiltin} value={selected.colors[key].startsWith("#") && selected.colors[key].length === 7 ? selected.colors[key] : "#000000"} onChange={(event) => updateTheme((theme) => ({ ...theme, colors: { ...theme.colors, [key]: event.target.value } }))} /><input disabled={selectedBuiltin} value={selected.colors[key]} onChange={(event) => updateTheme((theme) => ({ ...theme, colors: { ...theme.colors, [key]: event.target.value } }))} /></span></label>)}</div>
          </>}
          {/* No `preview()` on this page, unlike every other control in this
              dialog: there is nothing to apply live. A terminal reads its
              `command` once at mount and a live shell cannot be swapped under a
              running session, so existing terminals are never re-pointed and the
              value only matters to the *next* one. The buffered value is written
              in `apply()`; `cancel()` therefore needs no undo, because nothing
              was applied. */}
          {page === "terminal" && <>
            <h3>Shell for new terminals</h3>
            {shellList === null
              ? <div className="settings-row"><label>Shell</label><select disabled><option>Detecting shells…</option></select></div>
              : shellList.length === 0
                ? <p>No shells were detected on this machine. New terminals use the system default.</p>
                : <div className="settings-row"><label>Shell</label>
                    <select value={shell.shellId ?? ""} onChange={(event) => setShell({ version: 1, shellId: event.target.value === "" ? null : event.target.value })}>
                      {/* The default's name is shown only when the backend told
                          us which id it resolves to — inferring it from the list
                          order would be a guess. */}
                      <option value="">{defaultShellLabel ? `System default (${defaultShellLabel})` : "System default"}</option>
                      {shellList.map((option) => <option key={option.id} value={option.id} title={option.program}>{option.label} — {option.program}</option>)}
                    </select>
                  </div>}
            {shellMissing && <div className="warning">{shellMissing}</div>}
            <p>Applies to terminals opened from now on. A terminal already open keeps the shell it started with.</p>
          </>}
          {page === "mcp" && <McpToolsPage />}
          {page === "keyboard" && <>
            <input className="settings-search" placeholder="Search commands" value={query} onChange={(event) => setQuery(event.target.value)} />
            <div className="shortcut-list">{sections.map((section) => <div key={section.title}>
              {/* Plugin keys are remappable like any other; they are listed apart
                  because a command whose feature is switched off is a different
                  thing from one that is merely unbound. */}
              <h3 className="shortcut-section">{section.title}</h3>
              {section.commands.map((command) => {
              const binding = effectiveBinding(command, shortcuts);
              return <div className="shortcut-row" key={command.id}><span><strong>{command.label}</strong><small>{command.category} · {command.context}</small></span><button className={recording === command.id ? "recording" : ""} onClick={() => setRecording(command.id)} onKeyDown={(event) => recording === command.id && record(command, event)}>{recording === command.id ? "Press keys…" : formatChord(binding)}</button><button title="Clear" onClick={() => setShortcuts({ ...shortcuts, [command.id]: null })}>×</button><button title="Reset default" onClick={() => { const next = { ...shortcuts }; delete next[command.id]; setShortcuts(next); }}>↺</button></div>;
            })}
            </div>)}</div>
          </>}
          {page === "reference" && <div className="native-reference"><h3>Editor</h3><p>Ctrl/Cmd+/ toggle comment · Ctrl+G go to line · Ctrl/Cmd+F find · Ctrl/Cmd+S save where supported.</p><h3>Terminal</h3><p>Ctrl+Shift+C copy · Ctrl+V or Ctrl+Shift+V paste · Ctrl+Insert copy selection · Shift+Insert paste.</p><p>Native editor and terminal bindings are shown for reference and are not remapped by app shortcuts.</p></div>}
          {message && <div className="settings-message">{message}</div>}
        </main>
      </div>
      <footer><button onClick={() => { preview(DEFAULT_APPEARANCE); setShortcuts({}); setShell(DEFAULT_TERMINAL_SHELL); }}>Reset all</button><span /><button onClick={cancel}>Cancel</button><button className="primary" onClick={apply}>Apply</button></footer>
    </section>
  </div>;
}
