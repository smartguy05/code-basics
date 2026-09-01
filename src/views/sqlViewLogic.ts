//! The decisions `SqlView` would otherwise make inline.
//!
//! Separate from `views/sqlLogic.ts` on purpose: that module is the *result*
//! surface — the streamed reducer, the cells, the cap notice, the connection
//! grouping. What is here is the console *shell*'s vocabulary: what the
//! read-only badge may claim, what the writes confirmation must say before
//! consent is taken, which text a Run press actually submits, and what a stop
//! request did.
//!
//! Two rules from the backend are re-stated here rather than re-decided, and
//! both are the reason this file exists at all:
//!
//!  - **The guard is a heuristic over the SQL text.** Nothing in this module may
//!    render a word like "safe" or "sandboxed", because `sql::guard` itself
//!    refuses to make that claim (`RefusalReason` exists precisely to keep
//!    "this is a write" apart from "I could not tell").
//!  - **`SQLITE_OPEN_READONLY` is a stronger promise than the guard**, and the
//!    two must be tellable apart. `driver::ReadOnlyEnforcement` has three
//!    variants for this and its labels live in Rust so an engine cannot inherit
//!    a sentence it has not earned — but that enum crosses no IPC boundary
//!    today, so {@link enforcementBadge} mirrors those exact strings and a test
//!    pins them. If the Rust labels change, change them here too.

import type {
  SqlCandidate,
  SqlConnectionProfile,
  SqlConnectionView,
  SqlStopOutcome,
} from "../ipc/types";
import {
  candidateConnectionLabel,
  type ManualConnectionDraft,
} from "../components/sqlPickerLogic";
import type { SqlRunPhase } from "./sqlLogic";

// ---------------------------------------------------------------------------
// What is standing between the user and a write
// ---------------------------------------------------------------------------

/**
 * How strong the read-only promise on this connection is.
 *
 * Four tones for what Rust models as three variants, because the frontend can
 * be in a state the driver never is: a saved profile whose engine was never
 * determined has no driver behind it yet, so *which* of the three applies is
 * not yet knowable. Answering that with either of the real ones would be a
 * guess, and the stronger guess would be a dangerous one.
 */
export type EnforcementTone = "driver" | "guard" | "writes" | "undetermined";

export interface EnforcementBadge {
  tone: EnforcementTone;
  /** The short label on the badge. */
  label: string;
  /** The sentence behind it — the tooltip, and the line under the bar. */
  detail: string;
}

/**
 * What the connection bar's badge says, or `null` when no connection is chosen
 * (there is then nothing to make a claim about, and an empty badge would read
 * as "read-only").
 *
 * The three enforced labels are `driver::ReadOnlyEnforcement::label`'s own
 * words, copied deliberately: the wording lives in Rust so that adding an
 * engine cannot quietly promote a weaker promise, and duplicating it here with
 * a test is the closest this side can get to that guarantee while the enum
 * crosses no IPC boundary.
 */
export function enforcementBadge(connection: SqlConnectionView | null): EnforcementBadge | null {
  if (connection === null) return null;

  if (connection.allowWrites) {
    return {
      tone: "writes",
      label: "Writes are allowed on this connection",
      detail:
        "Nothing is enforcing read-only here: the guard lets a recognised write through, and a driver that could have opened read-only was not asked to.",
    };
  }

  if (connection.engine === null) {
    // Not "read-only by text check only": that names a specific mechanism, and
    // with no engine there is no driver to say whether a stronger one applies.
    return {
      tone: "undetermined",
      label: "Read-only — enforcement not determined",
      detail:
        "Writes are not allowed, but this connection's engine was never determined, so it cannot be said whether the database itself would also refuse one.",
    };
  }

  if (connection.engine === "sqlite") {
    return {
      tone: "driver",
      label: "Opened read-only by the driver",
      detail:
        "SQLite opens the file with SQLITE_OPEN_READONLY, so the database itself refuses a write. That is a stronger guarantee than the text check, and it does not depend on the guard classifying your SQL correctly.",
    };
  }

  return {
    tone: "guard",
    label: "Read-only by text check only — this connection can write",
    detail:
      "This driver has no read-only open mode, so a heuristic over the SQL text is the only thing between you and a write. It is not a database-enforced sandbox, and it abstains rather than guessing.",
  };
}

// ---------------------------------------------------------------------------
// Consent
// ---------------------------------------------------------------------------

/** What the writes confirmation must put in front of the user before consent. */
export interface WritesConfirm {
  title: string;
  /** What the guard *is*. Always stated — it is the claim most easily overread. */
  guard: string;
  /**
   * The extra, stronger protection this particular connection gives up, or
   * `null` when it has none to give up. Never merged into {@link guard}: the
   * whole point is that the user can tell a driver-enforced connection from one
   * where only the text check ever stood.
   */
  driverGiveUp: string | null;
  /**
   * The sentence that introduces {@link driverGiveUp}, or `null` when there is
   * nothing to introduce.
   *
   * It lives here rather than in the view because it is the strongest safety
   * claim the console makes — "protected" — and as a literal in `SqlView` it
   * rendered for *any* connection with a driver give-up, so an engine added
   * later would have inherited the word untested. Always `null` exactly when
   * {@link driverGiveUp} is, which a test pins: a lead with nothing behind it
   * would promise a protection that was never named.
   */
  driverGiveUpLead: string | null;
  confirmLabel: string;
}

/**
 * The confirmation for a writes toggle, or `null` when none is owed.
 *
 * `null` for *disallowing* writes, and that asymmetry is deliberate: turning
 * writes off only ever restores a protection, and a modal in front of the safe
 * direction is a modal people learn to dismiss — which is exactly the habit the
 * dangerous direction cannot afford.
 */
export function writesConfirm(connection: SqlConnectionView, next: boolean): WritesConfirm | null {
  if (!next) return null;

  const guard =
    "The read-only guard is a heuristic over the text of your SQL: it parses the statement and refuses anything it cannot positively recognise as a read. It is not a database-enforced sandbox, and it cannot see what a stored procedure does.";

  const driverGiveUp =
    connection.engine === "sqlite"
      ? "This connection is currently opened with SQLITE_OPEN_READONLY, so the database itself refuses a write today. Allowing writes reopens it without that flag and gives that up — a stronger protection than the guard, and the only one here that does not depend on your SQL being classified correctly."
      : null;

  return {
    title: `Allow writes on ${connection.name}?`,
    guard,
    driverGiveUp,
    driverGiveUpLead:
      driverGiveUp === null ? null : "This connection is protected by more than the guard.",
    confirmLabel: "Allow writes",
  };
}

// ---------------------------------------------------------------------------
// What a Run press submits
// ---------------------------------------------------------------------------

/**
 * The text a Run press would send, or why nothing will be sent.
 *
 * A refusal rather than a silent fallback. Running the whole editor because
 * nothing was selected is the failure mode this exists to prevent: the user
 * pressed *run selection*, and answering a narrower request with a broader
 * action is how a console runs a script somebody meant to skip.
 */
export type RunTarget = { kind: "run"; sql: string } | { kind: "refused"; reason: string };

/**
 * `mode` is which chord was pressed. `selection` is the editor's current
 * selection, exactly as it stands — the submitted text is never trimmed or
 * rewritten here, because the guard parses what is sent and a console that
 * edits your SQL on the way to the server is a console you cannot trust.
 */
export function runTarget(mode: "all" | "selection", full: string, selection: string): RunTarget {
  if (mode === "selection") {
    if (selection.trim() === "") {
      return {
        kind: "refused",
        reason:
          "Nothing is selected. Select the statement to run, or press Ctrl+Enter to run everything in the editor.",
      };
    }
    return { kind: "run", sql: selection };
  }
  if (full.trim() === "") {
    return { kind: "refused", reason: "Type a query to run." };
  }
  return { kind: "run", sql: full };
}

// ---------------------------------------------------------------------------
// Identity for a run
// ---------------------------------------------------------------------------

/**
 * The handle `sql_execute` runs under and `sql_cancel` stops.
 *
 * **Minted here, not by the backend**, for the reason `AppOutputPanel` mints its
 * console key: the first streamed events land before `invoke` resolves, so an id
 * that only came back with the promise would arrive after the rows it is
 * supposed to identify — and Stop would have nothing to aim at during exactly
 * the window a user is most likely to press it.
 *
 * The clock is an argument so this is testable, and both halves are present
 * because neither alone is enough: a counter restarts with the window and a
 * millisecond can serve two runs.
 */
export function mintQueryId(seq: number, nowMs: number): string {
  return `sql-${nowMs}-${seq}`;
}

// ---------------------------------------------------------------------------
// Phases and stops
// ---------------------------------------------------------------------------

export type SqlPhaseTone = "idle" | "busy" | "ok" | "warn" | "error";

export interface SqlPhaseLine {
  tone: SqlPhaseTone;
  text: string;
}

/**
 * One sentence for where the run got to.
 *
 * Six phases, six sentences. `refused` is the one that must not be worded as a
 * failure: nothing reached the database, so the reader's next act is to rewrite
 * the statement or allow writes — not to go looking at the server. And
 * `stopped` says the answer on screen is partial, which "finished" would deny.
 */
export function phaseLine(phase: SqlRunPhase): SqlPhaseLine {
  switch (phase.kind) {
    case "idle":
      return { tone: "idle", text: "Nothing has run yet." };
    case "running":
      return { tone: "busy", text: "Running…" };
    case "finished":
      return { tone: "ok", text: "Finished." };
    case "stopped":
      return {
        tone: "warn",
        text: "Stopped. What is shown is a partial answer — the statement was cut short.",
      };
    case "failed":
      return {
        tone: "error",
        text:
          phase.statementIndex === null
            ? `The connection failed, so nothing ran: ${phase.message}`
            : `${statementTitle(phase.statementIndex)} failed: ${phase.message}`,
      };
    case "refused":
      return {
        tone: "warn",
        text: `${statementTitle(phase.statementIndex)} was not sent — the read-only guard declined it. ${phase.reason}`,
      };
  }
}

/** How a result set is named. 1-based, because the user counts from one. */
export function statementTitle(statementIndex: number): string {
  return `Statement ${statementIndex + 1}`;
}

/**
 * What a stop request actually did.
 *
 * Three outcomes and three sentences: `notFound` is the ordinary race between a
 * Stop click and a statement that has just finished, and reporting it as a
 * success would make that race look like a working cancel. And none of them may
 * claim the *server* stopped — `sql_cancel` drops this side's connection, which
 * is what it says.
 */
export function stopLine(outcome: SqlStopOutcome): SqlPhaseLine {
  switch (outcome) {
    case "signalled":
      return {
        tone: "warn",
        text: "Asked to stop. This side stops reading and drops the connection; the server may still be running the statement.",
      };
    case "alreadyStopping":
      return { tone: "warn", text: "Already stopping — the earlier stop still stands." };
    case "notFound":
      return {
        tone: "idle",
        text: "Nothing was running under that id; it had already finished.",
      };
  }
}

// ---------------------------------------------------------------------------
// Keeping a selection honest
// ---------------------------------------------------------------------------

/**
 * The selected connection id after a fresh list arrives, or `null`.
 *
 * A deleted connection must not stay selected: the console would then point at
 * nothing while the bar still named a database, and the next Run would fail
 * against an id the store has forgotten.
 */
export function keepSelection(
  connections: SqlConnectionView[],
  selectedId: string | null,
): string | null {
  if (selectedId === null) return null;
  return connections.some((c) => c.id === selectedId) ? selectedId : null;
}

/** The chosen connection, or `null` — never a "first one" fallback. */
export function selectedConnection(
  connections: SqlConnectionView[],
  selectedId: string | null,
): SqlConnectionView | null {
  if (selectedId === null) return null;
  return connections.find((c) => c.id === selectedId) ?? null;
}

/**
 * Turn a discovered candidate into a profile to save.
 *
 * `allowWrites` is `false` and stays false whatever else is true — the backend
 * ignores this field on save anyway (consent moves only through
 * `sql_set_allow_writes`), and sending `true` would be a request the UI has no
 * business making.
 *
 * The id is made unique against what is already stored rather than reusing the
 * candidate's as-is: two files can name the same key, and `sql_save_connection`
 * is an upsert — a collision would silently overwrite a connection the user set
 * up rather than adding the one they clicked.
 */
export function profileFromCandidate(
  candidate: SqlCandidate,
  workspaceRoot: string | null,
  existingIds: readonly string[],
  nowMs: number,
  engineOverride?: NonNullable<SqlConnectionProfile["engine"]>,
): SqlConnectionProfile {
  const taken = new Set(existingIds);
  let id = candidate.id;
  let n = 2;
  while (taken.has(id)) {
    id = `${candidate.id}-${n}`;
    n += 1;
  }
  return {
    id,
    name: candidateConnectionLabel(candidate),
    engine: engineOverride ?? candidate.engine,
    secret: candidate.source,
    workspaceRoot,
    allowWrites: false,
    createdAtMs: nowMs,
    lastUsedMs: null,
  };
}

/** Build a collision-free, read-only profile after a manual draft tests OK. */
export function profileFromManual(
  draft: ManualConnectionDraft & { engine: NonNullable<ManualConnectionDraft["engine"]> },
  workspaceRoot: string,
  existingIds: readonly string[],
  nowMs: number,
): SqlConnectionProfile {
  const taken = new Set(existingIds);
  const stem = `manual:${nowMs}`;
  let id = stem;
  let suffix = 2;
  while (taken.has(id)) {
    id = `${stem}-${suffix}`;
    suffix += 1;
  }
  return {
    id,
    name: draft.name.trim(),
    engine: draft.engine,
    secret: { kind: "literal", connectionString: draft.connectionString },
    workspaceRoot: draft.global ? null : workspaceRoot,
    allowWrites: false,
    createdAtMs: nowMs,
    lastUsedMs: null,
  };
}

// ---------------------------------------------------------------------------
// A result the user cut short
// ---------------------------------------------------------------------------

/**
 * What a stopped run must put on the grid it cut short.
 *
 * Two halves for the same reason the row cap has two: a header note is visible
 * before scrolling, and a permanent final row is what the reader who scrolls to
 * the bottom of the data sees there. The phase line in the results bar is
 * neither — it sits inside a scrolling pane and leaves the screen while the
 * grid is read, which is the toast failure the cap row exists to prevent.
 */
export interface StoppedNote {
  /** The short note in the grid header, beside the row count. */
  header: string;
  /** The permanent last row of the grid. */
  row: string;
}

/**
 * The stop note for one statement's grid, or `null` when it is owed none.
 *
 * The backend reports a stopped statement with an ordinary `Completed` carrying
 * no row cap, so nothing in the completion distinguishes "we read every row"
 * from "we stopped reading" — this is the only thing that does.
 *
 * Two abstentions, and both matter:
 *
 *  - Only a `stopped` phase produces a note. A run that finished, failed or was
 *    refused stopped for a reason that is already stated in its own words.
 *  - Only the **last** statement gets one. Statements run in order, so every
 *    earlier statement had reported its own completion before the stop landed;
 *    calling those partial is exactly as wrong as calling the stopped one
 *    complete. And the note itself says rows *may* be missing rather than that
 *    they are — a stop can land after the final row as easily as before it.
 */
export function stoppedNote(
  phase: SqlRunPhase,
  statementIndex: number,
  lastStatementIndex: number,
): StoppedNote | null {
  if (phase.kind !== "stopped") return null;
  if (statementIndex !== lastStatementIndex) return null;
  return {
    header: "stopped — may be incomplete",
    row: "The run was stopped, so reading ended here. Rows may be missing and no cap was reported — do not read this as the whole answer.",
  };
}
