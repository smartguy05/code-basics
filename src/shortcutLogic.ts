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
  /**
   * The optional feature that owns this command, if one does.
   *
   * A plugin's keys are remappable like any other, but they are listed apart in
   * Settings: a command you cannot reach because its feature is switched off is
   * a different thing from one that is merely unbound, and a single flat list
   * makes the second look like the first. The value is a
   * `featuresLogic.FeatureKey`, typed loosely here only to keep this module
   * import-free.
   */
  plugin?: string;
}

/**
 * What each plugin is called in Settings.
 *
 * Keyed by the same id `featuresLogic.FeatureKey` uses, so the shortcut list and
 * the feature picker cannot call one thing two different names.
 */
export const PLUGIN_LABELS: Record<string, string> = {
  sqlConsole: "SQL Console",
  askCodebase: "Ask the codebase",
  mcpSqlServer: "SQL MCP server",
  webBrowser: "Web browser",
  tasks: "Tasks",
  // Not a `FeatureKey`: the Roslyn/LSP server is always-on, so it is keyed by
  // its own string. It still lives here so the Plugins menu and Settings name it
  // from one place, like every other plugin.
  mcpRoslyn: "Roslyn MCP server",
  // Also not a `FeatureKey`: the Redis panel is always-on, like the Roslyn
  // server, so it is keyed by its own string.
  redisConsole: "Redis",
};

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
  // `run` and `changes` are no longer tabs — they were merged into `project` —
  // but both keep a handler that selects Project with the matching pane, so they
  // stay advertised rather than disappearing from a user's muscle memory.
  ...["project", "run", "tests", "changes", "history", "architecture", "inspect", "sql"].map((name) => ({
    id: `view.${name}`, label: `Show ${name === "inspect" ? "Objects" : name.replace(/^./, (c) => c.toUpperCase())}`, category: "Navigation", context: "workspace" as const, defaultBinding: null,
  })),
  { id: "panel.notes", label: "Open Notes", category: "Panels", context: "global", defaultBinding: null },
  { id: "panel.launch", label: "Open Launcher", category: "Panels", context: "workspace", defaultBinding: null },
  { id: "panel.apps", label: "Show app output", category: "Panels", context: "global", defaultBinding: null },
  { id: "panel.running", label: "Show running processes", category: "Panels", context: "global", defaultBinding: null },
  { id: "terminal.new", label: "New terminal", category: "Terminal", context: "workspace", defaultBinding: null },
  { id: "agent.ask", label: "Ask the codebase", category: "Agent", context: "workspace", defaultBinding: chord("/", { ctrl: true }), plugin: "askCodebase" },
  // The installer panel, not the server: the server is a subcommand of this
  // executable that an agent spawns. Tagged `plugin` so `commandSections` files
  // it under its own heading in Settings and `pluginMenuRows` can name it from
  // `PLUGIN_LABELS` rather than showing the raw feature id.
  { id: "plugin.mcp", label: "SQL MCP server", category: "Agent", context: "workspace", defaultBinding: null, plugin: "mcpSqlServer" },
  { id: "agent.review", label: "Review changes", category: "Agent", context: "workspace", defaultBinding: null },
  // The embedded browser panel. `context: "global"` and no default chord: it is
  // the first plugin that acts on no codebase at all ("verify my deployment" is
  // not repo-specific), and the titlebar Plugins menu is how it is reached.
  //
  // Tagged `plugin` so `commandSections` files the row under its own heading in
  // Settings and `pluginMenuRows` names it from `PLUGIN_LABELS`. Without the
  // tag the row would be filed nowhere.
  { id: "plugin.browser", label: "Web browser", category: "Agent", context: "global", defaultBinding: null, plugin: "webBrowser" },
  // The per-codebase Tasks panel. Tagged `plugin` so `commandSections` files it
  // under its own heading in Settings and `pluginMenuRows` names it from
  // `PLUGIN_LABELS`. Registered as a handler by `WorkspaceTab` only while the
  // feature is on, exactly as `plugin.mcp` is, so the advertised command always
  // has a handler and a switched-off feature never acts.
  { id: "plugin.tasks", label: "Tasks", category: "Panels", context: "workspace", defaultBinding: null, plugin: "tasks" },
  // `allowInText` on the search overlays: the caret is almost always inside a
  // `.cm-editor` when the user reaches for Search All, so without it
  // `eventIsTyping` filters the command out of `dispatchShortcut` and Ctrl+N
  // falls through to the WebView instead of opening the palette.
  { id: "search.all", label: "Search All", category: "Search", context: "workspace", defaultBinding: chord("n", { ctrl: true }), allowInText: true },
  { id: "search.symbols", label: "Search Symbols", category: "Search", context: "workspace", defaultBinding: null, allowInText: true },
  { id: "search.files", label: "Search Files", category: "Search", context: "workspace", defaultBinding: chord("n", { ctrl: true, shift: true }), allowInText: true },
  { id: "search.actions", label: "Search Actions", category: "Search", context: "workspace", defaultBinding: chord("a", { ctrl: true, shift: true }), allowInText: true },
  { id: "tree.reveal", label: "Select opened file", category: "Navigation", context: "view", defaultBinding: chord("F1", { alt: true }) },
  { id: "tree.collapse", label: "Collapse file tree", category: "Navigation", context: "view", defaultBinding: null },
  { id: "console.find", label: "Find in console", category: "Console", context: "view", defaultBinding: chord("f", { ctrl: true }), allowInText: true },
  // F2, the rename key every IDE this replaces uses.
  //
  // `allowInText` because the caret is *by definition* inside `.cm-editor` when
  // this is pressed — the same reason `run.run`, `console.find` and
  // `change.next` carry it. Without it `eventIsTyping` would filter the command
  // out of `dispatchShortcut` and F2 would reach the WebView instead.
  //
  // And note what `dispatchShortcut` returns: whether a binding **matched**, not
  // whether the command acted. The caller uses that to `preventDefault`, and the
  // two differ exactly where it hurts — F5 bound to Run fell through a disabled
  // Run button and reloaded the whole application. A rename that refuses (no
  // server, a file with no editor on screen) must still consume the key.
  { id: "refactor.rename", label: "Rename symbol", category: "Refactor", context: "view", defaultBinding: chord("F2"), allowInText: true },
  { id: "change.next", label: "Next change", category: "Changes", context: "view", defaultBinding: chord("F7"), allowInText: true },
  { id: "change.previous", label: "Previous change", category: "Changes", context: "view", defaultBinding: chord("F7", { shift: true }), allowInText: true },
  { id: "font.code.increase", label: "Increase code size", category: "Appearance", context: "global", defaultBinding: chord("=", { ctrl: true }), allowInText: true },
  { id: "font.code.decrease", label: "Decrease code size", category: "Appearance", context: "global", defaultBinding: chord("-", { ctrl: true }), allowInText: true },
  { id: "font.code.reset", label: "Reset code size", category: "Appearance", context: "global", defaultBinding: chord("0", { ctrl: true }), allowInText: true },
  { id: "font.ui.increase", label: "Increase UI size", category: "Appearance", context: "global", defaultBinding: null },
  { id: "font.ui.decrease", label: "Decrease UI size", category: "Appearance", context: "global", defaultBinding: null },
  { id: "font.ui.reset", label: "Reset UI size", category: "Appearance", context: "global", defaultBinding: null },
  ...["run", "stop", "restart", "build", "rebuild", "clean"].map((name) => ({
    id: `run.${name}`, label: name.replace(/^./, (c) => c.toUpperCase()), category: "Run", context: "view" as const,
    // F5 for Run, the convention every IDE this replaces already uses.
    // The rest stay unbound: a default is a claim on a key the user cannot
    // easily see, and only this one earns it.
    defaultBinding: name === "run" ? chord("F5") : null,
    // F5 is a function key, not a character: it must still fire while the caret
    // sits in the file editor or the SQL console, which is where a person
    // actually is when they reach for Run.
    allowInText: name === "run",
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
    id: `sql.${name}`, label: name.split("-").map((p) => p.replace(/^./, (c) => c.toUpperCase())).join(" "), category: "SQL", context: "view" as const, defaultBinding: null, plugin: "sqlConsole",
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

/**
 * One candidate element for a command's DOM fallback.
 *
 * Deliberately not an `HTMLElement`: vitest runs in the node environment, so a
 * rule expressed over elements could not be tested at all. `shortcuts.ts` reads
 * these three facts off the DOM and this module decides what they mean.
 */
export interface CommandTarget {
  /**
   * Whether the element is actually being drawn.
   *
   * `getClientRects().length > 0` is the right test and `offsetParent !== null`
   * is not: `offsetParent` is `null` for a `position: fixed` element, and the
   * floating panels (terminals, notes, app output) are fixed — so that test
   * would call half the app's buttons invisible.
   */
  rendered: boolean;
  disabled: boolean;
}

/**
 * Which `[data-command]` element a shortcut should act on, and why not, when
 * there is no answer.
 *
 * **This exists because more than one element can carry the same command id.**
 * Every open codebase renders its own `WorkspaceTab`, and a background one is
 * only `hidden` — which is `display: none`, still in the DOM. A plain
 * `querySelector` takes the *first match in document order*, so with two
 * codebases open a Run or Changes shortcut fired the button belonging to
 * whichever workspace happened to be first in the list rather than the one on
 * screen. For `changes.commit` that means committing in the wrong repository.
 *
 * Four answers, kept distinct rather than collapsed into "no": nothing carries
 * this id, the only things that do are off screen, the visible one is refusing,
 * and here it is. The middle two are the ones worth separating — a hidden
 * match is a wiring bug, a disabled one is the app correctly saying no.
 *
 * A hidden candidate is **never** used as a fallback. Acting on a codebase the
 * user cannot see is worse than doing nothing, and doing nothing is what the
 * key already did before anyone bound it.
 */
export type CommandTargetChoice =
  | { kind: "none" }
  | { kind: "hidden" }
  | { kind: "disabled"; index: number }
  | { kind: "target"; index: number };

export function pickCommandTarget(candidates: readonly CommandTarget[]): CommandTargetChoice {
  if (candidates.length === 0) return { kind: "none" };
  const index = candidates.findIndex((candidate) => candidate.rendered);
  if (index === -1) return { kind: "hidden" };
  // The first *rendered* one, disabled or not. If it refuses, that is the
  // answer — looking past it for an enabled one further down would act on some
  // other surface than the one the user is looking at, which is the whole bug.
  return candidates[index]!.disabled ? { kind: "disabled", index } : { kind: "target", index };
}

/**
 * The keys currently bound to a command, for a tooltip — or `null` when it has
 * none.
 *
 * Tooltips used to spell their own shortcut, which is a copy of a fact that
 * lives somewhere else: rebinding Run in Settings left every button still
 * advertising the old key, and adding F5 left them advertising only Ctrl+Enter.
 * Reading the binding means the label cannot drift from what the key does.
 */
export function shortcutHint(
  commandId: string,
  overrides: ShortcutOverrides,
): string | null {
  const command = COMMANDS.find((candidate) => candidate.id === commandId);
  if (!command) return null;
  const binding = effectiveBinding(command, overrides);
  return binding ? formatChord(binding) : null;
}

/**
 * A tooltip with its shortcuts in brackets, skipping any that are unbound.
 *
 * Several hints because a command can be reachable more than one way and the
 * user should be told all of them — the SQL console answers to both F5 and
 * the editor's own Ctrl+Enter, and naming one hides the other.
 */
export function withShortcut(text: string, ...hints: (string | null)[]): string {
  const shown = hints.filter((hint): hint is string => hint !== null && hint !== "");
  return shown.length === 0 ? text : `${text} (${shown.join(" or ")})`;
}

/** One block of commands in the Settings shortcut list. */
export interface CommandSection {
  title: string;
  /** The feature that owns this block, or `null` for the built-in commands. */
  plugin: string | null;
  commands: CommandDefinition[];
}

/**
 * The Settings shortcut list, split into the app's own commands and one block
 * per plugin.
 *
 * Plugins come last, in a stable alphabetical order. Order matters for a
 * duller reason than taste: the list is long, the app's own keys are what
 * most people opened it to change, and a plugin block that floated to the top
 * depending on how the command array happened to be written would move under
 * people between releases.
 *
 * An empty block is dropped rather than shown: an empty heading reads as a
 * feature that has no shortcuts, which is a different and wrong claim.
 */
export function commandSections(commands: readonly CommandDefinition[]): CommandSection[] {
  const own = commands.filter((command) => command.plugin === undefined);
  const plugins = [
    ...new Set(commands.map((c) => c.plugin).filter((p): p is string => p !== undefined)),
  ].sort();

  const sections: CommandSection[] = [];
  if (own.length > 0) sections.push({ title: "Application", plugin: null, commands: own });
  for (const plugin of plugins) {
    const inPlugin = commands.filter((command) => command.plugin === plugin);
    if (inPlugin.length === 0) continue;
    sections.push({ title: PLUGIN_LABELS[plugin] ?? plugin, plugin, commands: inPlugin });
  }
  return sections;
}
