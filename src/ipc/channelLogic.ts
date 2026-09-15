import type { DebugEvent, ProcessEvent, SqlEvent, TerminalEvent } from "./types";

/**
 * Whether a streamed event is the last one its channel will deliver.
 *
 * A Tauri `Channel`'s `onmessage` handler is registered in the webview's
 * `__TAURI_INTERNALS__` callback registry for the life of the page and is never
 * released on its own. Since these apps mint a fresh `Channel` (and a fresh
 * `onEvent` closure capturing React state) per run/build/test/terminal/query,
 * an un-released handler is a genuine per-call heap leak. The wrappers in
 * {@link ./api} use these predicates to swap `onmessage` for a no-op once the
 * stream ends, dropping the heavy closure while keeping live streams intact.
 *
 * Each predicate names a single, unambiguous terminal marker — never a
 * mid-stream event that a later one follows — so releasing on it can never cut a
 * stream short. That is why the SQL predicate keys on the whole-query
 * `finished`, not a per-statement `completed`, and the debug predicate keys on
 * the session-ending states rather than a transient `paused`.
 */
export function isProcessTerminal(event: ProcessEvent): boolean {
  return event.type === "exited" || event.type === "failed";
}

/**
 * A PTY session ends when the shell exits or the spawn fails. A terminal the
 * user closes while the shell still runs is killed backend-side, which emits an
 * `exited` before the session ends — so this covers that path too.
 */
export function isTerminalStreamTerminal(event: TerminalEvent): boolean {
  return event.type === "exited" || event.type === "failed";
}

/**
 * A debug session is over once it exits, fails, or reports no adapter was
 * installed. `paused`/`running`/`starting` are mid-session and must not release
 * the channel; a Stop reports `exited` (with a possibly-null code), which this
 * treats as terminal like any other exit.
 */
export function isDebugTerminal(event: DebugEvent): boolean {
  if (event.type !== "state") return false;
  const kind = event.state.kind;
  return kind === "exited" || kind === "failed" || kind === "notInstalled";
}

/**
 * A SQL run ends with one `finished` event after every statement, whatever each
 * statement did. Keying on the per-statement `completed`/`failed` instead would
 * release the channel before a multi-statement query delivered its later
 * statements.
 */
export function isSqlTerminal(event: SqlEvent): boolean {
  return event.kind === "finished";
}
