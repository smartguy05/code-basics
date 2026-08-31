export const SHORTCUT_STORAGE_KEY = "code-basics.shortcuts.v1";

export type CommandContext = "global" | "workspace" | "view" | "panel";
export interface ShortcutChord {
  key: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
}
export interface CommandDefinition {
  id: string;
  label: string;
  category: string;
  context: CommandContext;
  defaultBinding: ShortcutChord | null;
  allowInText?: boolean;
}

const chord = (key: string, over: Partial<Omit<ShortcutChord, "key">> = {}): ShortcutChord => ({
  key, ctrl: false, shift: false, alt: false, meta: false, ...over,
});

export const COMMANDS: CommandDefinition[] = [
  { id: "file.open", label: "Open folder", category: "File", context: "global", defaultBinding: null },
  { id: "file.rescan", label: "Rescan workspace", category: "File", context: "workspace", defaultBinding: null },
  { id: "file.settings", label: "Open Settings", category: "File", context: "global", defaultBinding: null },
  { id: "project.next", label: "Next project", category: "Navigation", context: "global", defaultBinding: null },
  { id: "project.previous", label: "Previous project", category: "Navigation", context: "global", defaultBinding: null },
  { id: "project.close", label: "Close current project", category: "File", context: "workspace", defaultBinding: null },
  ...["run", "tests", "changes", "history", "architecture", "inspect", "sql"].map((name) => ({
    id: `view.${name}`, label: `Show ${name === "inspect" ? "Objects" : name.replace(/^./, (c) => c.toUpperCase())}`, category: "Navigation", context: "workspace" as const, defaultBinding: null,
  })),
  { id: "panel.notes", label: "Open Notes", category: "Panels", context: "global", defaultBinding: null },
  { id: "panel.launch", label: "Open Launcher", category: "Panels", context: "workspace", defaultBinding: null },
  { id: "panel.apps", label: "Show app output", category: "Panels", context: "global", defaultBinding: null },
  { id: "panel.running", label: "Show running processes", category: "Panels", context: "global", defaultBinding: null },
  { id: "terminal.new", label: "New terminal", category: "Terminal", context: "workspace", defaultBinding: null },
  { id: "agent.ask", label: "Ask the codebase", category: "Agent", context: "workspace", defaultBinding: chord("/", { ctrl: true }) },
  { id: "agent.review", label: "Review changes", category: "Agent", context: "workspace", defaultBinding: null },
  { id: "search.all", label: "Search All", category: "Search", context: "workspace", defaultBinding: chord("n", { ctrl: true }) },
  { id: "search.symbols", label: "Search Symbols", category: "Search", context: "workspace", defaultBinding: null },
  { id: "search.files", label: "Search Files", category: "Search", context: "workspace", defaultBinding: chord("n", { ctrl: true, shift: true }) },
  { id: "search.actions", label: "Search Actions", category: "Search", context: "workspace", defaultBinding: chord("a", { ctrl: true, shift: true }) },
  { id: "console.find", label: "Find in console", category: "Console", context: "view", defaultBinding: chord("f", { ctrl: true }), allowInText: true },
  { id: "change.next", label: "Next change", category: "Changes", context: "view", defaultBinding: chord("F7"), allowInText: true },
  { id: "change.previous", label: "Previous change", category: "Changes", context: "view", defaultBinding: chord("F7", { shift: true }), allowInText: true },
  { id: "font.code.increase", label: "Increase code size", category: "Appearance", context: "global", defaultBinding: chord("=", { ctrl: true }), allowInText: true },
  { id: "font.code.decrease", label: "Decrease code size", category: "Appearance", context: "global", defaultBinding: chord("-", { ctrl: true }), allowInText: true },
  { id: "font.code.reset", label: "Reset code size", category: "Appearance", context: "global", defaultBinding: chord("0", { ctrl: true }), allowInText: true },
  { id: "font.ui.increase", label: "Increase UI size", category: "Appearance", context: "global", defaultBinding: null },
  { id: "font.ui.decrease", label: "Decrease UI size", category: "Appearance", context: "global", defaultBinding: null },
  { id: "font.ui.reset", label: "Reset UI size", category: "Appearance", context: "global", defaultBinding: null },
  ...["run", "stop", "restart", "build", "rebuild", "clean"].map((name) => ({
    id: `run.${name}`, label: name.replace(/^./, (c) => c.toUpperCase()), category: "Run", context: "view" as const, defaultBinding: null,
  })),
  ...["all", "failed", "coverage", "stop"].map((name) => ({
    id: `tests.${name}`, label: `${name.replace(/^./, (c) => c.toUpperCase())} tests`, category: "Tests", context: "view" as const, defaultBinding: null,
  })),
  ...["stage", "unstage", "commit"].map((name) => ({
    id: `changes.${name}`, label: name.split("-").map((p) => p.replace(/^./, (c) => c.toUpperCase())).join(" "), category: "Changes", context: "view" as const, defaultBinding: null,
  })),
  ...["refresh", "edit", "zoom-in", "zoom-out", "fit", "actual-size"].map((name) => ({
    id: `architecture.${name}`, label: name.split("-").map((p) => p.replace(/^./, (c) => c.toUpperCase())).join(" "), category: "Architecture", context: "view" as const, defaultBinding: null,
  })),
  ...["refresh"].map((name) => ({
    id: `inspect.${name}`, label: name.split("-").map((p) => p.replace(/^./, (c) => c.toUpperCase())).join(" "), category: "Objects", context: "view" as const, defaultBinding: null,
  })),
  ...["run", "stop", "connections", "toggle-writes"].map((name) => ({
    id: `sql.${name}`, label: name.split("-").map((p) => p.replace(/^./, (c) => c.toUpperCase())).join(" "), category: "SQL", context: "view" as const, defaultBinding: null,
  })),
];

export type ShortcutOverrides = Record<string, ShortcutChord | null>;

export function readShortcutOverrides(raw: string | null): ShortcutOverrides {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const result: ShortcutOverrides = {};
    for (const [id, value] of Object.entries(parsed)) {
      if (value === null) result[id] = null;
      else if (isChord(value)) result[id] = normaliseChord(value);
    }
    return result;
  } catch { return {}; }
}

export function isChord(value: unknown): value is ShortcutChord {
  if (!value || typeof value !== "object") return false;
  const c = value as Partial<ShortcutChord>;
  return typeof c.key === "string" && typeof c.ctrl === "boolean" && typeof c.shift === "boolean"
    && typeof c.alt === "boolean" && typeof c.meta === "boolean";
}

export function normaliseChord(value: ShortcutChord): ShortcutChord {
  const key = value.key.length === 1 ? value.key.toLowerCase() : value.key;
  return { ...value, key };
}

export function chordFromEvent(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">): ShortcutChord {
  return normaliseChord({ key: event.key, ctrl: event.ctrlKey, shift: event.shiftKey, alt: event.altKey, meta: event.metaKey });
}

export function chordEquals(a: ShortcutChord, b: ShortcutChord): boolean {
  const left = normaliseChord(a); const right = normaliseChord(b);
  return left.key === right.key && left.ctrl === right.ctrl && left.shift === right.shift && left.alt === right.alt && left.meta === right.meta;
}

export function effectiveBinding(command: CommandDefinition, overrides: ShortcutOverrides): ShortcutChord | null {
  return Object.prototype.hasOwnProperty.call(overrides, command.id) ? overrides[command.id] ?? null : command.defaultBinding;
}

export function formatChord(binding: ShortcutChord | null): string {
  if (!binding) return "Unbound";
  return [binding.ctrl && "Ctrl", binding.meta && "Cmd", binding.alt && "Alt", binding.shift && "Shift", binding.key.length === 1 ? binding.key.toUpperCase() : binding.key]
    .filter(Boolean).join("+");
}

export function conflictingCommand(command: CommandDefinition, binding: ShortcutChord, overrides: ShortcutOverrides): CommandDefinition | null {
  // All app contexts can coexist in the DOM (views stay mounted behind panels),
  // so a duplicate would depend on registration order rather than user intent.
  return COMMANDS.find((other) => other.id !== command.id
    && effectiveBinding(other, overrides) != null && chordEquals(effectiveBinding(other, overrides)!, binding)) ?? null;
}

export function eventIsTyping(event: KeyboardEvent): boolean {
  const target = event.target;
  return target instanceof HTMLElement && (target.isContentEditable || target.closest("input, textarea, select, .cm-editor, .xterm") != null);
}

export function eventIsInsideSettings(event: KeyboardEvent): boolean {
  return event.target instanceof HTMLElement && event.target.closest(".settings-dialog") !== null;
}
