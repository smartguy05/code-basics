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
  /**
   * What this terminal runs instead of the default shell: an interactive agent
   * seeded with a question ("Ask the codebase"). `undefined` is a plain shell,
   * which is what every terminal opened from the titlebar is.
   *
   * A program **and** its arguments, never a command string. The PTY spawns
   * through `CommandBuilder` with these arguments as they stand, so nothing on
   * this side joins or re-splits them — assembling a string here and
   * re-splitting it in the backend is the bug this shape exists to make
   * impossible.
   *
   * That is not the same as "there is no shell". On Windows a program name can
   * resolve to a `.cmd`/`.bat` shim, and `cmd.exe` then re-parses the command
   * line: `&`, `|`, `<`, `>`, `^`, `"` and `%` change its meaning. The backend
   * (`cb_core::pty::argv`) refuses such an argument for a batch target before
   * spawning, so `terminalOpen` rejects rather than running something else. For
   * a real executable the guard does not apply and a question containing a
   * quote, a newline or a `&` crosses verbatim as one argv entry.
   */
  command?: { program: string; args: string[] };
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
 * Build the descriptor for a terminal running an **agent** rather than a shell —
 * the "Ask the codebase" terminal, opened already asking its question.
 *
 * A sibling of {@link makeTerminal} rather than an extra parameter on it,
 * because the two differ in more than the command: this one is titled after the
 * question (a strip of "Terminal 4"s would tell two asks apart not at all),
 * while a plain terminal is titled after its sequence number. They share the
 * *sequence*, and so the key space, deliberately: one monotonic counter in the
 * host means two terminals can never mint the same `term-N` and have React
 * reuse a live xterm for a different session.
 *
 * `args` is **copied**, not aliased: the caller assembled it from an IPC result
 * and a later mutation of that array must not rewrite what a running terminal
 * was spawned with.
 *
 * A blank `title` falls back to the plain terminal name. `terminalTitle` in
 * `askLogic` never returns blank, so this is defensive — but an unlabelled
 * panel is indistinguishable from a broken one, and the fallback costs nothing.
 */
export function makeAgentTerminal(
  seq: number,
  cwd: string,
  program: string,
  args: readonly string[],
  title: string,
): TerminalDescriptor {
  const clean = title.trim();
  return {
    key: `term-${seq}`,
    title: clean === "" ? `Terminal ${seq}` : clean,
    cwd,
    command: { program, args: [...args] },
  };
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

// --- Which terminal is in front -------------------------------------------

/**
 * How many raise steps the stylesheet reserves for the terminal band.
 *
 * Pinned by a test against `--z-panel-stack-span` in `styles.css`, which is the
 * only other place this number appears: CSS owns the band bases and this owns
 * the ordinal within them, so no z-index integer is ever written in TypeScript.
 * The clamp lives here because it is a decision, and decisions are tested.
 */
export const TERMINAL_STACK_SPAN = 100;

/**
 * Bring one terminal to the front of the stacking order.
 *
 * The order is a list of terminal keys, bottom-most first, kept **separately**
 * from the `terminals` array. That separation is the point: the array index
 * drives `pillBottom` and `cascadeShift`, which are positional identity, while
 * this is temporal recency. Reordering the array to raise a panel would
 * teleport its minimized pill to another slot and shift every un-dragged panel
 * diagonally, so the two facts never share a representation.
 *
 * Returns the **same array** when the key is already top, so the caller's
 * `setState` bails out and clicking the front terminal — much the commonest
 * case — costs no render at all.
 */
export function raiseTerminal(order: string[], key: string): string[] {
  if (order.length > 0 && order[order.length - 1] === key) return order;
  return [...order.filter((k) => k !== key), key];
}

/**
 * Reconcile the stacking order against the terminals that are actually open:
 * drop closed keys, append newly opened ones (so a fresh terminal starts on
 * top), and otherwise **leave the order alone**.
 *
 * Never reordering to match `open` is the contract that keeps stacking
 * independent of the array order. Returns the same array when nothing changed,
 * which is what stops the effect that calls it from looping.
 *
 * Deliberately not persisted across restarts: terminals do not survive one, and
 * keys are `term-${seq}` from a counter that restarts at 1 each session, so a
 * remembered order would either match nothing or silently apply a previous
 * session's stacking to unrelated terminals.
 */
export function syncStackOrder(order: string[], open: string[]): string[] {
  const live = new Set(open);
  const kept = order.filter((k) => live.has(k));
  const known = new Set(kept);
  const added = open.filter((k) => !known.has(k));

  if (added.length === 0 && kept.length === order.length) return order;
  return [...kept, ...added];
}

/**
 * The raise step a terminal renders at: 0 for the bottom of the stack, rising
 * to the top. Clamped into `TERMINAL_STACK_SPAN` so a very long-lived session
 * can never climb a terminal out of its band and over the Notes panel; the
 * clamp collapses the *bottom* of an absurd stack, never the top.
 *
 * A key the order has not seen yet — a terminal rendered in the commit before
 * the reconciling effect runs — sits at the bottom rather than yielding `NaN`.
 */
export function stackOffset(order: string[], key: string): number {
  const index = order.indexOf(key);
  if (index < 0) return 0;
  const excess = Math.max(0, order.length - TERMINAL_STACK_SPAN);
  return Math.max(0, index - excess);
}
