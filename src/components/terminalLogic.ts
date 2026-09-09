//! Pure decisions for the floating terminals — naming, cascade staggering,
//! persistence key, and the minimized-attention rule — extracted so they are
//! testable without a DOM (vitest runs in the node environment) and without a
//! live PTY. The React/xterm plumbing that drives them lives in
//! `TerminalPanel.tsx` and `TerminalView.tsx` and decides nothing.

import { normalizeLabel } from "./workspaceRenameLogic";

/** A terminal the app is hosting: a stable React key, a display title, and the
 * workspace it belongs to. */
export interface TerminalDescriptor {
  /** Stable across re-renders; the React key and layout scoping id. */
  key: string;
  /** Reusable, workspace-local number reserved while this window is open. */
  number: number;
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
 * Build a newly opened terminal from a monotonic identity and a reusable
 * display number. Keeping those separate lets React/session identity remain
 * unique while the title uses the lowest slot not occupied in this workspace.
 *
 * `command` is the shell to run — either the stored preference resolved against
 * what is on the machine now, or a one-off pick from the titlebar menu.
 * Omitting it is the ordinary case and means *send no program*, leaving
 * `spec_program` to fall back to `cb_core::pty::default_shell()`; that is what
 * every terminal did before the shell picker existed, and it is also the
 * deliberate answer whenever the preference cannot be honoured.
 *
 * An **optional fourth parameter** rather than a sibling of `makeAgentTerminal`,
 * because unlike the agent terminal nothing else about the descriptor changes:
 * the **title stays `Terminal N`** even for a one-off shell pick, since the
 * shell announces itself in its own prompt and the header is renamable.
 *
 * `args` is **copied** for the reason `makeAgentTerminal` copies: it came from
 * an IPC result, and a later mutation of that array must not rewrite what a
 * running terminal was spawned with.
 */
export function makeTerminal(
  seq: number,
  number: number,
  cwd: string,
  command?: { program: string; args: readonly string[] },
): TerminalDescriptor {
  return {
    key: `term-${seq}`,
    number,
    title: `Terminal ${number}`,
    cwd,
    ...(command ? { command: { program: command.program, args: [...command.args] } } : {}),
  };
}

/** The lowest positive terminal number not reserved by an open window. */
export function nextTerminalNumber(open: readonly TerminalDescriptor[]): number {
  const used = new Set(open.map((terminal) => terminal.number));
  let number = 1;
  while (used.has(number)) number += 1;
  return number;
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
  number: number,
  cwd: string,
  program: string,
  args: readonly string[],
  title: string,
): TerminalDescriptor {
  const clean = title.trim();
  return {
    key: `term-${seq}`,
    number,
    title: clean === "" ? `Terminal ${number}` : clean,
    cwd,
    command: { program, args: [...args] },
  };
}

/**
 * Rename the terminal with `key` in a list. A title `normalizeLabel` refuses is
 * refused here too — the existing title is kept — so a terminal never renders as
 * an empty header or an unreadable pill, the same rule `renameNote` follows for
 * notes. Other terminals are returned untouched.
 *
 * The cleaning is {@link normalizeLabel}, the codebase-tab rename's own helper,
 * reused rather than reimplemented: a header title sits beside the other
 * terminals' titles exactly as a tab label sits beside other tabs, so it wants
 * the same control/bidi stripping, the same whitespace collapsing, and the same
 * 40-character cap (`MAX_LABEL_LENGTH`) sliced by **code point**. A second
 * copy of that character class is a second place for it to drift, and the one
 * thing worse than an unreadable title is two rules for what makes one.
 */
export function renameTerminal(
  list: TerminalDescriptor[],
  key: string,
  title: string,
): TerminalDescriptor[] {
  const clean = acceptedTerminalTitle(title);
  return list.map((t) => (t.key === key && clean !== null ? { ...t, title: clean } : t));
}

/**
 * The title a terminal may actually be called, or `null` when the rename is
 * refused outright.
 *
 * This exists so that the *one* rule has one name. A rename has two
 * destinations — the descriptor the header and the pill render from, and the
 * backend running-registry record the Running panel renders from — and they are
 * written by two different calls. While one of them cleaned through
 * {@link normalizeLabel} and the other merely trimmed, the two disagreed on
 * exactly the inputs the cleaning exists for: `trim` removes neither U+0000 nor
 * a bidi override, so a title this rule *refused* still reached the registry,
 * and a title it merely tidied (`"a	b"`) left the header and the Running panel
 * showing different names for one terminal. Both callers now ask this.
 */
export function acceptedTerminalTitle(title: string): string | null {
  return normalizeLabel(title);
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
  /**
   * Whether Alt was held.
   *
   * Read for one reason: on Windows **AltGr is reported as Ctrl+Alt**, so a
   * layout where AltGr+V types a character produces exactly the modifier state
   * a Ctrl+V paste chord matches. Without this the terminal ate the keystroke
   * and that character could not be typed at all.
   */
  altKey: boolean;
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
 * `Shift+Insert`). Everything else passes through — including anything held
 * with Alt, because AltGr is indistinguishable from Ctrl+Alt on Windows and a
 * character the user typed must reach the shell.
 */
export function terminalKeyAction(e: TerminalKeyEvent, hasSelection: boolean): TerminalKeyAction {
  if (e.type !== "keydown") return "passthrough";
  // Alt up on every chord below. AltGr arrives as Ctrl+Alt, so a chord that
  // ignored Alt would claim AltGr+V — a character on several layouts — as a
  // paste, and there is then no way to type that character into a terminal.
  if (e.altKey) return "passthrough";
  const key = e.key.toLowerCase();
  if (e.ctrlKey && e.shiftKey && key === "c") return "copy";
  if (e.ctrlKey && key === "v") return "paste"; // Ctrl+V and Ctrl+Shift+V
  if (e.ctrlKey && !e.shiftKey && key === "insert") return hasSelection ? "copy" : "passthrough";
  if (e.shiftKey && !e.ctrlKey && key === "insert") return "paste";
  return "passthrough";
}

/**
 * What the view must do for a key action: whether to stop the webview's own
 * handling of the chord, and which clipboard operation to run.
 */
export interface TerminalKeyEffect {
  /** Call `preventDefault()` on the keyboard event. */
  preventDefault: boolean;
  /** Write the terminal's selection to the clipboard. */
  copy: boolean;
  /** Read the clipboard and paste it into the terminal. */
  paste: boolean;
}

/**
 * Turn a {@link TerminalKeyAction} into the two things the view actually does,
 * so `TerminalView` executes an effect rather than deciding one.
 *
 * This exists because the interesting half of the decision was invisible.
 * Returning `false` from xterm's custom key handler stops only xterm's own key
 * **translation**; it calls no `preventDefault()`, so the webview still ran its
 * native paste onto xterm's hidden textarea, whose paste listener fired another
 * data event — one `Ctrl+V`, two writes to the PTY, and the extra one neither
 * CRLF-normalized nor bracketed, so a multi-line paste was executed line by
 * line by the shell. The copy chords get the same treatment for a smaller
 * reason: `Ctrl+Insert` is a native Copy command, and leaving the default in
 * place races our `writeText` against a native copy of the (usually empty)
 * textarea selection.
 *
 * A passthrough must **never** prevent the default, which is what the test of
 * that name guards: `Ctrl+C` is the shell interrupt and `F5` is an app
 * shortcut, and a blanket `preventDefault()` here would swallow both.
 */
export function terminalKeyEffect(action: TerminalKeyAction): TerminalKeyEffect {
  return {
    preventDefault: action !== "passthrough",
    copy: action === "copy",
    paste: action === "paste",
  };
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
 * drives `cascadeShift` (a fresh terminal's open position), which is positional
 * identity, while this is temporal recency. Reordering the array to raise a panel
 * would shift every un-dragged panel diagonally, so the two facts never share a
 * representation.
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
