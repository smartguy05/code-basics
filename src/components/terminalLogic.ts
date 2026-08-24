//! Pure decisions for the floating terminals — naming, cascade staggering,
//! persistence key, and the minimized-attention rule — extracted so they are
//! testable without a DOM (vitest runs in the node environment) and without a
//! live PTY. The React/xterm plumbing that drives them lives in
//! `TerminalPanel.tsx` and `TerminalView.tsx` and decides nothing.

/** A terminal the app is hosting: a stable React key and a display title. */
export interface TerminalDescriptor {
  /** Stable across re-renders; the React key and layout scoping id. */
  key: string;
  /** Shown in the header and the minimized pill, e.g. "Terminal 3". */
  title: string;
}

/**
 * Build the descriptor for a newly opened terminal from a monotonic sequence
 * number the host keeps. Monotonic rather than "lowest unused" on purpose:
 * closing Terminal 2 and opening another gives Terminal 4, not a recycled 2, so
 * a title never refers to two different sessions across a session's life.
 */
export function makeTerminal(seq: number): TerminalDescriptor {
  return { key: `term-${seq}`, title: `Terminal ${seq}` };
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

/** The localStorage key the terminals persist their shared layout under. */
export const TERMINAL_LAYOUT_KEY = "cb.terminal.layout";

/**
 * Whether a chunk of terminal output should raise the minimized panel's
 * attention flash. Only while minimized (a visible terminal already shows its
 * output), and only for output that actually contains something — an empty
 * string, which the stream can carry, is not a reason to flash.
 *
 * The bell character (`\x07`) always counts: a program ringing the bell is
 * asking for attention by definition, even if that is all it sent.
 */
export function outputNeedsAttention(minimized: boolean, text: string): boolean {
  if (!minimized) return false;
  if (text.includes(String.fromCharCode(7))) return true; // the bell
  return text.length > 0;
}
