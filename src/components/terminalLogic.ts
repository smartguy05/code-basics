//! Pure decisions for the floating terminals — naming, cascade staggering,
//! persistence key, and the minimized-attention rule — extracted so they are
//! testable without a DOM (vitest runs in the node environment) and without a
//! live PTY. The React/xterm plumbing that drives them lives in
//! `TerminalPanel.tsx` and `TerminalView.tsx` and decides nothing.

/** A terminal the app is hosting: a stable React key, a display title, and the
 * workspace it belongs to. */
export interface TerminalDescriptor {
  /** Stable across re-renders; the React key and layout scoping id. */
  key: string;
  /** Shown in the header and the minimized pill, e.g. "Terminal 3". */
  title: string;
  /**
   * The workspace root this terminal was opened for, passed to the backend as
   * the PTY's cwd. Bound at open time and never re-derived, so a terminal stays
   * in its own repository even after its tab is backgrounded and the backend's
   * *active* workspace has moved on.
   */
  cwd: string;
  /**
   * Optional user-chosen background for the minimized pill, so several
   * terminals can be told apart at a glance. `undefined` (never set, or cleared
   * back to "Default") leaves the pill its theme colour. In-memory with the
   * descriptor — terminals do not survive a restart, so neither does the colour.
   */
  color?: string;
}

/**
 * Build the descriptor for a newly opened terminal from a monotonic sequence
 * number the host keeps and the workspace root it belongs to. Monotonic rather
 * than "lowest unused" on purpose: closing Terminal 2 and opening another gives
 * Terminal 4, not a recycled 2, so a title never refers to two different
 * sessions across a session's life.
 */
export function makeTerminal(seq: number, cwd: string): TerminalDescriptor {
  return { key: `term-${seq}`, title: `Terminal ${seq}`, cwd };
}

/**
 * Rename the terminal with `key` in a list. A blank (or whitespace-only) title
 * is refused — the existing title is kept — so a terminal never renders as an
 * empty header or an unreadable pill, the same rule `renameNote` follows for
 * notes. Other terminals are returned untouched.
 */
export function renameTerminal(
  list: TerminalDescriptor[],
  key: string,
  title: string,
): TerminalDescriptor[] {
  const clean = title.trim();
  return list.map((t) => (t.key === key && clean !== "" ? { ...t, title: clean } : t));
}

/**
 * Set (or clear) the minimized-pill colour of the terminal with `key`. An
 * `undefined` colour clears it back to the theme default. Other terminals are
 * returned untouched.
 */
export function recolorTerminal(
  list: TerminalDescriptor[],
  key: string,
  color: string | undefined,
): TerminalDescriptor[] {
  return list.map((t) => (t.key === key ? { ...t, color } : t));
}

/** The step, in px, between one minimized pill and the next, stacked upward. */
const PILL_STEP = 48;

/**
 * The `bottom` offset, in px, of a minimized terminal pill at position `index`
 * among the open terminals. The base slot (`bottom: 16`) is reserved for the
 * global Notes bar, so terminals start one step up and never land on it — the
 * fix for the Notes/terminal pill overlap. Pills then stack upward so several
 * minimized terminals do not share a spot either.
 */
export function pillBottom(index: number, step: number = PILL_STEP): number {
  return 16 + (index + 1) * step;
}

/** The step, in px, each cascaded terminal is offset from the previous. */
const CASCADE_STEP = 28;
/** How many steps before the cascade wraps back to the start. */
const CASCADE_WRAP = 6;

/**
 * The pixel offset a freshly opened terminal (one with no remembered position)
 * is nudged by, so several opened in a row do not land exactly on top of one
 * another. Wraps after `CASCADE_WRAP` so a long-lived session does not march a
 * new terminal off the screen.
 *
 * `index` is the terminal's position among those currently open.
 */
export function cascadeShift(index: number, step: number = CASCADE_STEP): number {
  const clamped = ((index % CASCADE_WRAP) + CASCADE_WRAP) % CASCADE_WRAP;
  return clamped * step;
}

/**
 * The localStorage key the terminals of one workspace persist their shared
 * layout under. Scoped by root so a fresh terminal in one codebase does not
 * adopt the saved geometry of a terminal in another.
 */
export function terminalLayoutKey(root: string): string {
  return `cb.terminal.layout:${root}`;
}

/**
 * Whether a chunk of terminal output should raise the minimized panel's
 * attention flash. Only while minimized (a visible terminal already shows its
 * output), and only for the bell character (`\x07`) — a program ringing the
 * bell is asking for attention by definition.
 *
 * Ordinary output does **not** flash: a running terminal (a build, a TUI,
 * `claude`) streams output constantly, so flashing on any of it would pulse the
 * pill the whole time it runs and mean nothing. The bell is the one in-band
 * signal that the terminal actually wants the user.
 */
export function outputNeedsAttention(minimized: boolean, text: string): boolean {
  if (!minimized) return false;
  return text.includes(String.fromCharCode(7)); // the bell — the only ask
}

/** What a key event should do in the terminal, once copy/paste is accounted for. */
export type TerminalKeyAction = "copy" | "paste" | "passthrough";

/** The parts of a keyboard event this decision reads — kept minimal so it is
 * testable without a real `KeyboardEvent`. */
export interface TerminalKeyEvent {
  type: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  key: string;
}

/**
 * Decide whether a key event copies the selection, pastes the clipboard, or is
 * forwarded to the shell untouched.
 *
 * A raw PTY terminal has no copy/paste of its own. `Ctrl+C` must stay the shell
 * **interrupt**, so copying uses `Ctrl+Shift+C` (or `Ctrl+Insert` with a
 * selection). Pasting has no such conflict to protect, so both the Windows
 * standard `Ctrl+V` and the terminal chord `Ctrl+Shift+V` paste (as does
 * `Shift+Insert`). Everything else passes through.
 */
export function terminalKeyAction(e: TerminalKeyEvent, hasSelection: boolean): TerminalKeyAction {
  if (e.type !== "keydown") return "passthrough";
  const key = e.key.toLowerCase();
  if (e.ctrlKey && e.shiftKey && key === "c") return "copy";
  if (e.ctrlKey && key === "v") return "paste"; // Ctrl+V and Ctrl+Shift+V
  if (e.ctrlKey && !e.shiftKey && key === "insert") return hasSelection ? "copy" : "passthrough";
  if (e.shiftKey && !e.ctrlKey && key === "insert") return "paste";
  return "passthrough";
}
