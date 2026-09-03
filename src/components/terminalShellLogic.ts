//! The remembered choice of shell for new terminals: how it is stored, and —
//! the point of the module — when it must be **ignored** rather than acted on.
//!
//! Pure, and `storage` is a parameter rather than the `localStorage` global (the
//! `reviewLogic.ts` pattern), so every branch below is testable without a DOM.
//!
//! Two rules live here and nowhere else:
//!
//! - **Abstaining is a legitimate answer.** A `null` result means "send no
//!   program and let the backend's `default_shell()` decide", which is exactly
//!   what every terminal did before this feature existed. So a preference we
//!   cannot honour degrades to today's behaviour instead of failing an open.
//! - **A preference is never erased because its shell is absent.** A shell can
//!   be missing because PATH is temporarily broken, a tool is mid-upgrade, or a
//!   volume is unmounted; deleting the user's choice over a transient absence is
//!   unrecoverable, while abstaining at the point of use costs one terminal
//!   opening on the platform default. So we persist it, abstain when we use it,
//!   and say so through {@link missingShellNotice}.

import type { ShellInfo } from "../ipc/types";

/** Versioned so a future shape can be told from this one rather than guessed at. */
export const TERMINAL_SHELL_STORAGE_KEY = "code-basics.terminalShell.v1";

/**
 * The stored preference. `shellId` is a {@link ShellInfo.id}, never a path: the
 * path is re-detected on every use, so a reinstall that moves the executable
 * keeps the preference working, and a stored path could name a file that has
 * since become something else.
 *
 * `null` means "the system default" — the platform order `default_shell()`
 * already applies — and is a real choice, not an absent one.
 */
export interface TerminalShellPref {
  version: 1;
  shellId: string | null;
}

/** What a machine with no stored preference has: whatever the backend picks. */
export const DEFAULT_TERMINAL_SHELL: TerminalShellPref = { version: 1, shellId: null };

/**
 * Parse a stored payload. Absent, blank, unparseable, not an object, or a
 * version this build does not know all mean the default — a preference we
 * cannot read is no preference, and there is no earlier shape to migrate from,
 * so inventing one would be a guess.
 */
export function readTerminalShell(raw: string | null): TerminalShellPref {
  if (!raw || raw.trim() === "") return DEFAULT_TERMINAL_SHELL;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_TERMINAL_SHELL;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return DEFAULT_TERMINAL_SHELL;
  const { version, shellId } = parsed as Record<string, unknown>;
  if (version !== 1) return DEFAULT_TERMINAL_SHELL;
  // An empty string is not an id; it is what an unselected `<select>` sends, and
  // treating it as "no preference" is what makes the System-default option work.
  if (typeof shellId !== "string" || shellId === "") return DEFAULT_TERMINAL_SHELL;
  return { version: 1, shellId };
}

/** Read the preference. Never throws — storage may be unavailable entirely. */
export function loadTerminalShell(storage: Pick<Storage, "getItem">): TerminalShellPref {
  try {
    return readTerminalShell(storage.getItem(TERMINAL_SHELL_STORAGE_KEY));
  } catch {
    return DEFAULT_TERMINAL_SHELL;
  }
}

/** Persist the preference. Never throws: persistence is a convenience. */
export function saveTerminalShell(
  storage: Pick<Storage, "setItem">,
  pref: TerminalShellPref,
): void {
  try {
    storage.setItem(TERMINAL_SHELL_STORAGE_KEY, JSON.stringify(pref));
  } catch {
    // Ignore: a terminal opening on the default shell beats refusing to open.
  }
}

/**
 * The program a new terminal should run, or `null` for "send no program and let
 * the backend's `default_shell()` decide".
 *
 * `null` in all three abstaining cases, and each is deliberate:
 *
 * - no preference — the backend's platform order is the answer;
 * - `detected === null` — detection has not been read yet, and resolving a
 *   preference against a list we have not got would be guessing off a stale one;
 * - the remembered id names nothing detected — the shell is not here *now*.
 *
 * That last case notably does **not** fall back to `{ program: pref.shellId }`.
 * A bare remembered id whose file is gone would fail the spawn, so the user
 * would get no terminal at all where the default shell would have given them a
 * working one. `args` is copied for the reason `makeAgentTerminal` copies: the
 * array came from an IPC result, and a later mutation of it must not rewrite
 * what a running terminal was spawned with.
 */
export function resolvePreferredShell(
  pref: TerminalShellPref,
  detected: readonly ShellInfo[] | null,
): { program: string; args: string[] } | null {
  if (pref.shellId === null || detected === null) return null;
  const match = detected.find((s) => s.id === pref.shellId);
  if (!match) return null;
  return { program: match.program, args: [...match.args] };
}

/**
 * The line Settings shows when the remembered shell is no longer here — the
 * visible half of "abstain, do not erase": without it, a preference silently
 * stops taking effect and the user has no way to tell that from a bug.
 *
 * `null` while `detected === null`, because warning during an in-flight read is
 * a false alarm — every shell looks missing before the list arrives.
 */
export function missingShellNotice(
  pref: TerminalShellPref,
  detected: readonly ShellInfo[] | null,
): string | null {
  if (pref.shellId === null || detected === null) return null;
  if (detected.some((s) => s.id === pref.shellId)) return null;
  return `“${pref.shellId}” was not found on this machine, so new terminals use the system default. Your choice is kept in case it comes back.`;
}
