//! Pure decisions for the SQL console — the streaming reducer, the cell
//! rendering discipline, the cap notice, the Run button's reason, the
//! connection-test phrasing, connection grouping and column sizing.
//!
//! Extracted because `SqlConsoleView` cannot be tested (vitest runs in the node
//! environment, so there is no DOM) and because the decisions are the whole
//! point. `crates/core/src/sql/` went to real trouble to keep answers apart —
//! NULL from an empty string from a truncated one, a refusal from a failure, a
//! row cap from a byte cap, `0` rows affected from *no count reported* — and a
//! surface that renders any two of those the same has told the reader something
//! untrue about their data. Every function here exists to stop that.
//!
//! House precedents: `components/reviewStreamLogic.ts` for reducing a stream,
//! `components/lspStatusLogic.ts` for one sentence per variant,
//! `components/ObjectTree.tsx` for the never-render-blank cell rule, and
//! `components/launcherLogic.ts` / `components/runningLogic.ts` for
//! this-codebase-first grouping.

import type {
  SqlColumn,
  SqlCompletion,
  SqlConnectionView,
  SqlEvent,
  SqlRowCap,
  SqlTestOutcome,
  SqlValue,
} from "../ipc/types";

// ---------------------------------------------------------------------------
// The streamed run
// ---------------------------------------------------------------------------

/**
 * Where a run has got to.
 *
 * The three ways a run can *stop* are three variants and never one, because they
 * call for three different things from the reader: `finished` means the answer
 * on screen is the whole answer, `stopped` means it is a partial answer the user
 * asked to cut short, and `failed` means it is not an answer at all. `refused`
 * is a fourth: the guard declined and **nothing reached the database**, which is
 * not a database error and must never be shown as one.
 */
export type SqlRunPhase =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "finished" }
  /** The user cancelled: `finished` with `cancelled: true`. */
  | { kind: "stopped" }
  /** `statementIndex` is null when nothing had run — a connection that would not open. */
  | { kind: "failed"; message: string; statementIndex: number | null }
  /** The read-only guard declined. `reason` is the guard's own wording. */
  | { kind: "refused"; statementIndex: number; reason: string };

/** One statement's accumulated result. */
export interface SqlStatement {
  statementIndex: number;
  /**
   * `null` means *no `columns` event has arrived yet*; `[]` means the statement
   * reported no columns. Different facts — a grid drawn from `[]` is correct and
   * one drawn from "not yet" is premature.
   */
  columns: SqlColumn[] | null;
  /** Every row from every `rows` event, in arrival order. */
  rows: SqlValue[][];
  /** The authoritative counts, cap and elapsed time; `null` until it ends. */
  completion: SqlCompletion | null;
  /** Things the backend said about how this ran — an allowed write, say. */
  notices: string[];
  /** The guard's refusal, when it refused this statement. */
  refusal: string | null;
  /** The driver's error, when this statement failed. */
  error: string | null;
}

export interface SqlState {
  phase: SqlRunPhase;
  /** In arrival order. Indexed by `statementIndex` via {@link statementAt}. */
  statements: SqlStatement[];
  /**
   * A failure that named no statement: the connection would not open, so there
   * is no statement to hang it on and no result to show beside it.
   */
  connectionError: string | null;
}

export function initialSqlState(): SqlState {
  return { phase: { kind: "idle" }, statements: [], connectionError: null };
}

/** The statement with this index, or `undefined` when none has been seen. */
export function statementAt(state: SqlState, index: number): SqlStatement | undefined {
  return state.statements.find((s) => s.statementIndex === index);
}

function blankStatement(statementIndex: number): SqlStatement {
  return {
    statementIndex,
    columns: null,
    rows: [],
    completion: null,
    notices: [],
    refusal: null,
    error: null,
  };
}

/**
 * Apply `change` to the statement with this index, creating it if the stream
 * never announced it.
 *
 * Creating it is deliberate rather than dropping the event: an event for an
 * unannounced statement means our model of the protocol is wrong, and losing the
 * user's rows is a far worse way to find that out than showing them.
 */
function withStatement(
  state: SqlState,
  index: number,
  change: (statement: SqlStatement) => SqlStatement,
): SqlState {
  const existing = statementAt(state, index);
  const statements = existing
    ? state.statements.map((s) => (s.statementIndex === index ? change(s) : s))
    : [...state.statements, change(blankStatement(index))];
  return { ...state, statements };
}

/**
 * Whether the phase has already reached a verdict that a later event must not
 * quietly overwrite.
 *
 * The bug this prevents: the backend sends `finished { cancelled: false }` after
 * a `failed` or a `refused` too — it reports the *channel* closing, not that all
 * was well — so a reducer that maps `finished` straight to `finished` turns
 * every failure and every refusal into a clean success at the last moment.
 */
function isVerdict(phase: SqlRunPhase): boolean {
  return phase.kind === "failed" || phase.kind === "refused";
}

/**
 * Fold one streamed event into the view state. Pure; never mutates its input.
 */
export function applyEvent(state: SqlState, event: SqlEvent): SqlState {
  // Anything that is not a terminal event means the run is under way. An event
  // arriving after a verdict does not un-decide it.
  const running = (next: SqlState): SqlState =>
    isVerdict(next.phase) ? next : { ...next, phase: { kind: "running" } };

  switch (event.kind) {
    case "started":
      return running(withStatement(state, event.statementIndex, (s) => s));

    case "columns":
      return running(
        withStatement(state, event.statementIndex, (s) => ({ ...s, columns: event.columns })),
      );

    case "rows":
      return running(
        withStatement(state, event.statementIndex, (s) => ({
          ...s,
          rows: [...s.rows, ...event.rows],
        })),
      );

    case "notice":
      return running(
        withStatement(state, event.statementIndex, (s) => ({
          ...s,
          notices: [...s.notices, event.message],
        })),
      );

    case "completed":
      return running(
        withStatement(state, event.completion.statementIndex, (s) => ({
          ...s,
          completion: event.completion,
        })),
      );

    case "refused": {
      const next = withStatement(state, event.statementIndex, (s) => ({
        ...s,
        refusal: event.reason,
      }));
      // First verdict stands: a second one would rewrite what the user was told
      // about the first, and the first is the one that stopped the run.
      return isVerdict(next.phase)
        ? next
        : {
            ...next,
            phase: {
              kind: "refused",
              statementIndex: event.statementIndex,
              reason: event.reason,
            },
          };
    }

    case "failed": {
      const withError =
        event.statementIndex === null
          ? { ...state, connectionError: state.connectionError ?? event.message }
          : withStatement(state, event.statementIndex, (s) => ({
              ...s,
              error: s.error ?? event.message,
            }));
      return isVerdict(withError.phase)
        ? withError
        : {
            ...withError,
            phase: {
              kind: "failed",
              message: event.message,
              statementIndex: event.statementIndex,
            },
          };
    }

    case "finished":
      // `cancelled` is the user-stopped flag and nothing else: `SqlEvent::
      // Finished`'s own doc guarantees that a run ending in `Failed` or
      // `Refused` still finishes with `cancelled: false`, and `sql_execute`
      // hard-codes `false` on both of those paths. So a cancel can never be
      // masking a verdict, and taking it as the answer cannot hide one.
      // Otherwise a verdict already reached survives — the backend sends this
      // event after a failure and after a refusal too, so mapping it straight to
      // `finished` would turn every one of those into a clean success at the
      // last moment — and only an undecided run finishes clean.
      if (event.cancelled) return { ...state, phase: { kind: "stopped" } };
      return isVerdict(state.phase) ? state : { ...state, phase: { kind: "finished" } };
  }
}

// ---------------------------------------------------------------------------
// Cells
// ---------------------------------------------------------------------------

/** One cell, ready to render: nothing here needs deciding again. */
export interface SqlCell {
  /** Space-separated classes; distinct per kind, so CSS can style each apart. */
  className: string;
  /** What to draw. **Never empty** — see {@link cellRender}. */
  text: string;
  /** The `title` tooltip, or `null` when there is nothing more to say. */
  title: string | null;
}

const EMPTY_MARKER = "(empty)";
const EMPTY_CUT_MARKER = "(empty, cut short)";

/**
 * What one cell says.
 *
 * `ObjectTree.tsx`'s discipline applied to a grid. The rule that drives all of
 * it: **nothing renders blank except a value that is genuinely an empty string**
 * — and even that renders as a marker rather than as nothing, because a blank
 * cell is indistinguishable from a NULL, from a cell that failed to decode, and
 * from a column the driver could not represent at all. Those are four different
 * statements about the user's data and they get four different renderings.
 *
 * Truncation is called out rather than implied: a value silently cut short is
 * the one failure mode a console cannot afford, since the reader's next act is
 * usually to copy it somewhere.
 */
export function cellRender(value: SqlValue): SqlCell {
  switch (value.kind) {
    case "null":
      return {
        className: "sql-cell sql-null",
        text: "NULL",
        title: "No value (SQL NULL) — not an empty string",
      };

    case "text": {
      if (value.text === "") {
        // An empty string is a value. Distinguished from NULL by both the marker
        // and the class, and distinguished again when it was also cut short —
        // "empty" and "we showed you none of it" are not the same claim.
        return value.truncated
          ? {
              className: "sql-cell sql-text sql-empty sql-truncated",
              text: EMPTY_CUT_MARKER,
              title: "The value was truncated for display; none of it is shown here",
            }
          : {
              className: "sql-cell sql-text sql-empty",
              text: EMPTY_MARKER,
              title: "An empty string — not NULL",
            };
      }
      return value.truncated
        ? {
            className: "sql-cell sql-text sql-truncated",
            text: `${value.text}…`,
            title: "Truncated for display — this is not the whole value",
          }
        : { className: "sql-cell sql-text", text: value.text, title: null };
    }

    case "number":
      // A string on purpose, all the way from the driver: NUMERIC(38,10) and
      // bigint do not survive a JSON number, and a rounded value in a console is
      // a wrong answer rendered confidently. So it is never re-parsed here.
      return value.text === ""
        ? {
            className: "sql-cell sql-number sql-empty",
            text: "(no digits reported)",
            title: "The driver reported this numeric cell as an empty string",
          }
        : { className: "sql-cell sql-number", text: value.text, title: null };

    case "bool":
      return {
        className: "sql-cell sql-bool",
        text: value.value ? "true" : "false",
        title: null,
      };

    case "bytes": {
      const size = `${value.byteLength} bytes`;
      if (value.hex === "" && !value.truncated) {
        // A blob that really is empty. Not the same as one whose preview was cut
        // to nothing, which carries `truncated` and a non-zero byteLength.
        return {
          className: "sql-cell sql-bytes sql-empty",
          text: `(${size})`,
          title: "A zero-length binary value",
        };
      }
      return value.truncated
        ? {
            className: "sql-cell sql-bytes sql-truncated",
            // The original size, not the preview's: a truncated blob must say
            // how big it actually was or the preview reads as the whole value.
            text: `0x${value.hex}… (${size})`,
            title: `Binary value of ${size}; only the first bytes are shown`,
          }
        : {
            className: "sql-cell sql-bytes",
            text: `0x${value.hex} (${size})`,
            title: `Binary value of ${size}`,
          };
    }

    case "unsupported":
      // Names the type rather than showing a placeholder that reads as data.
      // Nothing was decoded, so nothing that looks like a value may appear.
      return {
        className: "sql-cell sql-unsupported",
        text:
          value.typeName === ""
            ? "(type not supported)"
            : `(unsupported: ${value.typeName})`,
        title:
          value.typeName === ""
            ? "This column's type has no representation here, and it was not reported"
            : `No representation for ${value.typeName}; the value was not decoded`,
      };

    case "unavailable":
      // The rest of the row read fine and this one cell did not. Distinct from
      // `unsupported`: that type can never be shown, this one failed once.
      return {
        className: "sql-cell sql-unavailable",
        text: "(could not read)",
        title: value.reason,
      };
  }
}

// ---------------------------------------------------------------------------
// The row cap
// ---------------------------------------------------------------------------

/**
 * What to say about a truncated result set, or `null` when every row is present.
 *
 * A cap is **reported, never silently applied** — the whole reason `rowCap` is
 * `null`-or-present rather than a count. And the two reasons get two sentences:
 * with `rowLimit` raising the limit returns more rows, with `byteLimit` it does
 * not, so collapsing them would send the reader to a setting that cannot help.
 *
 * Takes the cap-carrying shape rather than a whole result, so it serves both
 * `SqlResultSet` and the streamed `SqlCompletion`.
 */
export function capNotice(result: { rowCap: SqlRowCap | null }): string | null {
  const cap = result.rowCap;
  if (!cap) return null;
  const shown = `Showing the first ${cap.limit} rows`;
  switch (cap.reason) {
    case "rowLimit":
      return `${shown} — the row limit stopped this result. More rows match; raise the limit or narrow the query to see them.`;
    case "byteLimit":
      return `${shown} — the result reached its size budget. More rows match, and raising the row limit would not return them; select fewer or smaller columns.`;
  }
}

// ---------------------------------------------------------------------------
// The Run button
// ---------------------------------------------------------------------------

/** Everything the Run button's state depends on. */
export interface SqlRunContext {
  connection: SqlConnectionView | null;
  sql: string;
  phase: SqlRunPhase;
}

/**
 * Why Run is disabled, or `null` when it is not.
 *
 * A reason rather than a boolean, for the reason `LspServerLine.hint` exists: a
 * greyed control that cannot say why is a dead end, and each of these has a
 * different next action — pick a connection, stop the run, type something, or
 * tell the connection which engine it is.
 *
 * The order is the order the user must act in. A run in flight is reported ahead
 * of an empty box because Stop is the only thing that helps either way.
 */
export function runDisabledReason(context: SqlRunContext): string | null {
  if (!context.connection) return "Choose a connection first.";
  if (context.phase.kind === "running") {
    return "A query is already running — stop it before running another.";
  }
  if (context.sql.trim() === "") return "Type a query to run.";
  if (context.connection.engine === null) {
    return "This connection's engine was never determined — set it on the connection before running.";
  }
  return null;
}

// ---------------------------------------------------------------------------
// The connection test
// ---------------------------------------------------------------------------

/** How loudly an outcome is worth saying. Only a real success is `ok`. */
export type SqlStatusTone = "ok" | "warn" | "error";

export interface SqlStatusLine {
  tone: SqlStatusTone;
  /** One sentence, complete on its own without the outcome beside it. */
  text: string;
  /** The driver's own words, when there were any. Never invented. */
  detail: string | null;
}

/**
 * What a connection test found, in the user's terms.
 *
 * Every variant gets its own sentence and two of them get two, for the reason
 * `lspStatusLogic.headlineFor` does: the backend distinguishes a wrong password,
 * an unreachable host, a database file that would not open, a file that opened
 * and is not a database, a failed handshake, a timeout, an undetermined engine,
 * an unsupported one, an unreadable secret, and *an error it has no rule for* —
 * and every pair of those calls for a different fix. Collapsing any two is the
 * bug this function exists to prevent.
 *
 * `failed` is the abstention and says so. It is never filed under whichever
 * category looks closest, because a confidently wrong diagnosis costs more than
 * an honest "not recognised" plus the driver's own message.
 *
 * Nothing here says "safe" or "sandboxed" about any connection — the guard is a
 * heuristic and this surface must not imply otherwise.
 */
export function statusLine(outcome: SqlTestOutcome): SqlStatusLine {
  switch (outcome.kind) {
    case "ok":
      // Two sentences for one variant: "connected, and the server would not say
      // what it is" is a real answer, and a blank version reads as a bug.
      return outcome.serverVersion === null
        ? {
            tone: "ok",
            text: "Connected. The server reported no version.",
            detail: null,
          }
        : {
            tone: "ok",
            text: `Connected — ${outcome.serverVersion}`,
            detail: outcome.serverVersion,
          };

    case "authFailed":
      return {
        tone: "error",
        text: "The server rejected these credentials.",
        detail: outcome.message,
      };

    case "unreachable":
      return {
        tone: "error",
        text: "The server could not be reached over the network.",
        detail: outcome.message,
      };

    case "cannotOpenFile":
      // A wrong path, not a network. Reporting this as `unreachable` would send
      // the user to look at firewalls for a typo in a filename.
      return {
        tone: "error",
        text: "The database file could not be opened.",
        detail: outcome.message,
      };

    case "notADatabase":
      // The handle opened; the first page read says it is something else.
      return {
        tone: "error",
        text: "That file opened, but it is not a database this build can read.",
        detail: outcome.message,
      };

    case "tlsFailed":
      return {
        tone: "error",
        text: "The TLS handshake failed, so no connection was made.",
        detail: outcome.message,
      };

    case "timeout":
      // The duration is reported only when it was measured; `null` means the
      // driver timed out on its own schedule and never said after how long.
      return outcome.afterMs === null
        ? {
            tone: "warn",
            text: "The connection attempt timed out; the driver did not report how long it waited.",
            detail: null,
          }
        : {
            tone: "warn",
            text: `The connection attempt timed out after ${outcome.afterMs} ms.`,
            detail: null,
          };

    case "engineUnknown":
      return {
        tone: "warn",
        text: "The engine could not be determined, so nothing was tried.",
        detail: null,
      };

    case "engineUnsupported":
      return {
        tone: "warn",
        text: `This build cannot speak to ${outcome.engine}, so nothing was tried.`,
        detail: null,
      };

    case "secretUnresolved":
      return {
        tone: "warn",
        text: "The connection string could not be resolved, so nothing was tried.",
        detail: outcome.reason,
      };

    case "failed":
      return {
        tone: "error",
        text: "The connection failed, and the driver's message was not recognised.",
        detail: outcome.message,
      };
  }
}

// ---------------------------------------------------------------------------
// Grouping connections
// ---------------------------------------------------------------------------

/**
 * Saved connections, this codebase first.
 *
 * Three groups and not two. A connection saved against *another* codebase is not
 * a global one: a global connection (`workspaceRoot: null`) was deliberately
 * saved to be available everywhere, and one belonging to a different repository
 * is being shown for findability, the way `runningLogic.stopMenuGroups` shows
 * processes from elsewhere rather than hiding them. Presenting the second as the
 * first would make another project's database look like it belongs here.
 */
export interface SqlConnectionGroups {
  thisCodebase: SqlConnectionView[];
  /** Saved with no codebase — available wherever the user is. */
  global: SqlConnectionView[];
  /** Saved against a different codebase. Shown, but never as local. */
  otherCodebases: SqlConnectionView[];
}

/** Separators normalised and any trailing one dropped, for comparing roots. */
function normaliseRoot(path: string): string {
  return path
    .replace(/\\/g, "/")
    .replace(/\/+$/, "")
    .toLowerCase();
}

/**
 * Group and sort. Roots are compared case-insensitively with separators
 * normalised — this is a Windows-first app, where the root the backend scanned
 * and the one a user typed differ in case and slash routinely — and compared
 * whole, so `C:/repo2` is not this codebase when `C:/repo` is.
 */
export function groupConnections(
  connections: SqlConnectionView[],
  root: string | null,
): SqlConnectionGroups {
  const here = root === null ? null : normaliseRoot(root);
  const groups: SqlConnectionGroups = { thisCodebase: [], global: [], otherCodebases: [] };

  for (const connection of connections) {
    if (connection.workspaceRoot === null) {
      groups.global.push(connection);
    } else if (here !== null && normaliseRoot(connection.workspaceRoot) === here) {
      groups.thisCodebase.push(connection);
    } else {
      // Includes every rooted connection when no codebase is open: nothing can
      // be "this codebase" then, and claiming otherwise would be a guess.
      groups.otherCodebases.push(connection);
    }
  }

  // Stable between reads: the backend's order is storage order, which is not
  // something a list the user scans should inherit.
  const order = (rows: SqlConnectionView[]) =>
    rows.sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));

  return {
    thisCodebase: order(groups.thisCodebase),
    global: order(groups.global),
    otherCodebases: order(groups.otherCodebases),
  };
}

// ---------------------------------------------------------------------------
// Column sizing
// ---------------------------------------------------------------------------

/** Narrow enough to be wasteful below, wide enough for `NULL` and `(empty)`. */
export const MIN_COL_CHARS = 6;

/** Past this a column crowds out every other one; the cell scrolls instead. */
export const MAX_COL_CHARS = 60;

/**
 * How many rows to measure. A cap rather than the whole set: the grid streams,
 * so "all rows" is not a fixed quantity, and re-measuring 100k rows on every
 * `rows` event would cost more than the layout is worth.
 */
export const WIDTH_SAMPLE_ROWS = 50;

/**
 * A width in characters per column: the widest of the header and the first
 * {@link WIDTH_SAMPLE_ROWS} rendered cells, clamped.
 *
 * Measures {@link cellRender}'s output rather than the raw value, so the markers
 * are sized for too — a column of NULLs is a column with `NULL` in it, and one
 * whose type could not be decoded needs room for the type's name.
 */
export function columnWidths(columns: SqlColumn[], rows: SqlValue[][]): number[] {
  const sample = rows.slice(0, WIDTH_SAMPLE_ROWS);
  return columns.map((column, index) => {
    let widest = column.name.length;
    for (const row of sample) {
      const value = row[index];
      // A ragged row is not an error worth stopping on: the row is simply short.
      if (value === undefined) continue;
      widest = Math.max(widest, cellRender(value).text.length);
    }
    return Math.min(MAX_COL_CHARS, Math.max(MIN_COL_CHARS, widest));
  });
}
