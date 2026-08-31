import { describe, expect, it } from "vitest";
import type {
  SqlColumn,
  SqlConnectionView,
  SqlEvent,
  SqlTestOutcome,
  SqlValue,
} from "../ipc/types";
import {
  applyEvent,
  capNotice,
  cellRender,
  columnWidths,
  groupConnections,
  initialSqlState,
  MAX_COL_CHARS,
  MIN_COL_CHARS,
  runDisabledReason,
  statementAt,
  statusLine,
  WIDTH_SAMPLE_ROWS,
  type SqlState,
} from "./sqlLogic";

// ---------------------------------------------------------------------------
// applyEvent
// ---------------------------------------------------------------------------

const feed = (events: SqlEvent[], from: SqlState = initialSqlState()): SqlState =>
  events.reduce(applyEvent, from);

const text = (t: string): SqlValue => ({ kind: "text", text: t, truncated: false });

const completion = (
  statementIndex: number,
  over: Partial<{ rowCount: number; rowsAffected: number | null; elapsedMs: number }> = {},
) => ({
  statementIndex,
  rowCount: over.rowCount ?? 0,
  rowCap: null,
  rowsAffected: over.rowsAffected ?? null,
  elapsedMs: over.elapsedMs ?? 3,
});

describe("applyEvent", () => {
  it("starts idle with nothing in it", () => {
    const state = initialSqlState();
    expect(state.phase).toEqual({ kind: "idle" });
    expect(state.statements).toEqual([]);
    expect(state.connectionError).toBe(null);
  });

  it("moves to running on `started` and records the statement", () => {
    const state = feed([{ kind: "started", statementIndex: 0 }]);
    expect(state.phase).toEqual({ kind: "running" });
    expect(state.statements).toHaveLength(1);
    expect(statementAt(state, 0)?.statementIndex).toBe(0);
  });

  it("keeps columns-not-yet-reported distinct from a statement with no columns", () => {
    const started = feed([{ kind: "started", statementIndex: 0 }]);
    expect(statementAt(started, 0)?.columns).toBe(null);

    const none = feed([{ kind: "columns", statementIndex: 0, columns: [] }], started);
    expect(statementAt(none, 0)?.columns).toEqual([]);
  });

  it("appends rows across several `rows` events, in order", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "rows", statementIndex: 0, rows: [[text("a")]] },
      { kind: "rows", statementIndex: 0, rows: [[text("b")], [text("c")]] },
    ]);
    const rows = statementAt(state, 0)?.rows ?? [];
    expect(rows).toHaveLength(3);
    expect(rows.map((r) => cellRender(r[0] ?? { kind: "null" }).text)).toEqual(["a", "b", "c"]);
  });

  it("creates a statement for an event that arrives without a `started`", () => {
    const state = feed([{ kind: "rows", statementIndex: 2, rows: [[text("x")]] }]);
    expect(statementAt(state, 2)?.rows).toHaveLength(1);
    expect(state.phase).toEqual({ kind: "running" });
  });

  it("keeps several statements apart", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "rows", statementIndex: 0, rows: [[text("first")]] },
      { kind: "started", statementIndex: 1 },
      { kind: "rows", statementIndex: 1, rows: [[text("second")]] },
    ]);
    expect(state.statements).toHaveLength(2);
    expect(statementAt(state, 0)?.rows).toHaveLength(1);
    expect(statementAt(state, 1)?.rows).toHaveLength(1);
  });

  it("stores the completion on its own statement without ending the run", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "completed", completion: completion(0, { rowsAffected: 0 }) },
    ]);
    expect(statementAt(state, 0)?.completion?.rowsAffected).toBe(0);
    // `completed` ends a statement; `finished` ends the run.
    expect(state.phase).toEqual({ kind: "running" });
  });

  it("keeps rowsAffected 0 distinct from rowsAffected null", () => {
    const zero = feed([{ kind: "completed", completion: completion(0, { rowsAffected: 0 }) }]);
    const none = feed([{ kind: "completed", completion: completion(0) }]);
    expect(statementAt(zero, 0)?.completion?.rowsAffected).toBe(0);
    expect(statementAt(none, 0)?.completion?.rowsAffected).toBe(null);
  });

  it("collects notices without turning them into refusals or failures", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "notice", statementIndex: 0, message: "This is a write, and writes are allowed." },
      { kind: "finished", cancelled: false },
    ]);
    expect(statementAt(state, 0)?.notices).toEqual(["This is a write, and writes are allowed."]);
    expect(statementAt(state, 0)?.refusal).toBe(null);
    expect(state.phase).toEqual({ kind: "finished" });
  });

  // --- the terminal states, kept apart ---

  it("finishes clean", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "completed", completion: completion(0) },
      { kind: "finished", cancelled: false },
    ]);
    expect(state.phase).toEqual({ kind: "finished" });
  });

  it("reports a cancelled run as stopped, never as finished", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "finished", cancelled: true },
    ]);
    expect(state.phase).toEqual({ kind: "stopped" });
  });

  it("reports a failure as failed, and the trailing `finished` does not erase it", () => {
    const state = feed([
      { kind: "started", statementIndex: 0 },
      { kind: "failed", statementIndex: 0, message: "syntax error near slect" },
      { kind: "finished", cancelled: false },
    ]);
    expect(state.phase.kind).toBe("failed");
    if (state.phase.kind === "failed") {
      expect(state.phase.message).toContain("slect");
      expect(state.phase.statementIndex).toBe(0);
    }
    expect(statementAt(state, 0)?.error).toContain("slect");
  });

  it("files a failure that named no statement as a connection error", () => {
    const state = feed([
      { kind: "failed", statementIndex: null, message: "connection refused" },
      { kind: "finished", cancelled: false },
    ]);
    expect(state.connectionError).toBe("connection refused");
    expect(state.statements).toEqual([]);
    expect(state.phase.kind).toBe("failed");
    if (state.phase.kind === "failed") expect(state.phase.statementIndex).toBe(null);
  });

  it("keeps a refusal as its own terminal state, carrying the guard's message", () => {
    const reason = "This looks like a write. The check is a heuristic and may be wrong.";
    const state = feed([
      { kind: "refused", statementIndex: 0, reason },
      { kind: "finished", cancelled: false },
    ]);
    expect(state.phase).toEqual({ kind: "refused", statementIndex: 0, reason });
    expect(statementAt(state, 0)?.refusal).toBe(reason);
  });

  it("lets a stop win over a refusal, but never lets a plain finish erase one", () => {
    const refused: SqlEvent = { kind: "refused", statementIndex: 0, reason: "no" };
    expect(feed([refused, { kind: "finished", cancelled: false }]).phase.kind).toBe("refused");
    expect(feed([refused, { kind: "finished", cancelled: true }]).phase.kind).toBe("stopped");
  });

  it("keeps a failure distinct from a refusal when both arrive", () => {
    const state = feed([
      { kind: "refused", statementIndex: 0, reason: "guard" },
      { kind: "failed", statementIndex: 1, message: "boom" },
    ]);
    // The first terminal verdict stands; both are recorded on their statements.
    expect(state.phase.kind).toBe("refused");
    expect(statementAt(state, 0)?.refusal).toBe("guard");
    expect(statementAt(state, 1)?.error).toBe("boom");
  });

  it("keeps the first failure rather than the last", () => {
    const state = feed([
      { kind: "failed", statementIndex: 0, message: "first" },
      { kind: "failed", statementIndex: 1, message: "second" },
    ]);
    if (state.phase.kind === "failed") expect(state.phase.message).toBe("first");
    else throw new Error("expected failed");
  });

  it("does not mutate the state it is given", () => {
    const before = feed([{ kind: "started", statementIndex: 0 }]);
    const snapshot = JSON.stringify(before);
    applyEvent(before, { kind: "rows", statementIndex: 0, rows: [[text("a")]] });
    expect(JSON.stringify(before)).toBe(snapshot);
  });
});

// ---------------------------------------------------------------------------
// cellRender
// ---------------------------------------------------------------------------

describe("cellRender", () => {
  it("renders NULL as a marker, never as blank", () => {
    const cell = cellRender({ kind: "null" });
    expect(cell.text).toBe("NULL");
    expect(cell.className).toContain("sql-null");
    expect(cell.title).not.toBe(null);
  });

  it("renders an empty string differently from NULL", () => {
    const empty = cellRender({ kind: "text", text: "", truncated: false });
    const nul = cellRender({ kind: "null" });
    expect(empty.text.length).toBeGreaterThan(0);
    expect(empty.text).not.toBe(nul.text);
    expect(empty.className).not.toBe(nul.className);
  });

  it("renders ordinary text as itself", () => {
    expect(cellRender({ kind: "text", text: "hello", truncated: false }).text).toBe("hello");
  });

  it("marks truncated text with an ellipsis and says so", () => {
    const cell = cellRender({ kind: "text", text: "abc", truncated: true });
    expect(cell.text).toBe("abc…");
    expect(cell.className).toContain("sql-truncated");
    expect(cell.title ?? "").toMatch(/truncat/i);
  });

  it("keeps truncated-empty distinct from empty", () => {
    const cut = cellRender({ kind: "text", text: "", truncated: true });
    const empty = cellRender({ kind: "text", text: "", truncated: false });
    expect(cut.text).not.toBe(empty.text);
    expect(cut.className).not.toBe(empty.className);
  });

  it("renders a number from its string, unrounded", () => {
    const cell = cellRender({ kind: "number", text: "12345678901234567890.0000000001" });
    expect(cell.text).toBe("12345678901234567890.0000000001");
    expect(cell.className).toContain("sql-number");
  });

  it("never renders a number as blank even if the driver reported nothing", () => {
    expect(cellRender({ kind: "number", text: "" }).text.length).toBeGreaterThan(0);
  });

  it("renders booleans as words", () => {
    expect(cellRender({ kind: "bool", value: true }).text).toBe("true");
    expect(cellRender({ kind: "bool", value: false }).text).toBe("false");
  });

  it("renders bytes as hex with the original size", () => {
    const cell = cellRender({ kind: "bytes", hex: "0a0b", byteLength: 2, truncated: false });
    expect(cell.text).toContain("0x0a0b");
    expect(cell.text).toContain("2 bytes");
  });

  it("says how big a truncated blob really was", () => {
    const cell = cellRender({ kind: "bytes", hex: "0a", byteLength: 4096, truncated: true });
    expect(cell.text).toContain("…");
    expect(cell.text).toContain("4096 bytes");
    expect(cell.className).toContain("sql-truncated");
  });

  it("renders a zero-length blob as a marker, not blank", () => {
    expect(
      cellRender({ kind: "bytes", hex: "", byteLength: 0, truncated: false }).text,
    ).toBe("(0 bytes)");
  });

  it("names the type it could not decode", () => {
    const cell = cellRender({ kind: "unsupported", typeName: "geography" });
    expect(cell.text).toContain("geography");
    expect(cell.className).toContain("sql-unsupported");
  });

  it("still says something for an unsupported type with no name", () => {
    expect(cellRender({ kind: "unsupported", typeName: "" }).text.trim().length).toBeGreaterThan(
      0,
    );
  });

  it("shows the reason a cell was unavailable", () => {
    const cell = cellRender({ kind: "unavailable", reason: "decode failed" });
    expect(cell.title).toBe("decode failed");
    expect(cell.className).toContain("sql-unavailable");
    expect(cell.text).not.toBe("");
  });

  it("gives unsupported and unavailable different text and different classes", () => {
    const unsupported = cellRender({ kind: "unsupported", typeName: "geography" });
    const unavailable = cellRender({ kind: "unavailable", reason: "geography" });
    expect(unsupported.text).not.toBe(unavailable.text);
    expect(unsupported.className).not.toBe(unavailable.className);
  });

  it("renders every kind non-blank and every kind distinguishable", () => {
    const values: SqlValue[] = [
      { kind: "null" },
      { kind: "text", text: "", truncated: false },
      { kind: "text", text: "", truncated: true },
      { kind: "text", text: "v", truncated: false },
      { kind: "number", text: "1" },
      { kind: "bool", value: true },
      { kind: "bytes", hex: "ff", byteLength: 1, truncated: false },
      { kind: "unsupported", typeName: "t" },
      { kind: "unavailable", reason: "r" },
    ];
    const rendered = values.map(cellRender);
    for (const cell of rendered) expect(cell.text.trim().length).toBeGreaterThan(0);
    const keys = new Set(rendered.map((c) => `${c.className}|${c.text}`));
    expect(keys.size).toBe(values.length);
  });
});

// ---------------------------------------------------------------------------
// capNotice
// ---------------------------------------------------------------------------

describe("capNotice", () => {
  it("says nothing when every row is present", () => {
    expect(capNotice({ rowCap: null })).toBe(null);
  });

  it("names the row limit and that more rows exist", () => {
    const notice = capNotice({ rowCap: { limit: 500, reason: "rowLimit" } }) ?? "";
    expect(notice).toContain("500");
    expect(notice).toMatch(/row limit/i);
    expect(notice).toMatch(/more rows/i);
  });

  it("names the byte budget, and says raising the row limit would not help", () => {
    const notice = capNotice({ rowCap: { limit: 137, reason: "byteLimit" } }) ?? "";
    expect(notice).toContain("137");
    expect(notice).toMatch(/size|byte/i);
    expect(notice).toMatch(/more rows/i);
    expect(notice).toMatch(/would not/i);
  });

  it("gives the two reasons different sentences", () => {
    expect(capNotice({ rowCap: { limit: 10, reason: "rowLimit" } })).not.toBe(
      capNotice({ rowCap: { limit: 10, reason: "byteLimit" } }),
    );
  });
});

// ---------------------------------------------------------------------------
// runDisabledReason
// ---------------------------------------------------------------------------

const connection = (over: Partial<SqlConnectionView> = {}): SqlConnectionView => ({
  id: "c1",
  name: "Local",
  engine: "sqlite",
  secret: { kind: "dotEnv", path: ".env", key: "DB" },
  holdsASecret: false,
  workspaceRoot: null,
  allowWrites: false,
  createdAtMs: 0,
  lastUsedMs: null,
  ...over,
});

describe("runDisabledReason", () => {
  it("returns null when a run is possible", () => {
    expect(
      runDisabledReason({ connection: connection(), sql: "select 1", phase: { kind: "idle" } }),
    ).toBe(null);
  });

  it("says so when there is no connection", () => {
    expect(
      runDisabledReason({ connection: null, sql: "select 1", phase: { kind: "idle" } }),
    ).toMatch(/connection/i);
  });

  it("says so when a run is already going", () => {
    expect(
      runDisabledReason({
        connection: connection(),
        sql: "select 1",
        phase: { kind: "running" },
      }),
    ).toMatch(/already running/i);
  });

  it("says so when the query is empty or only whitespace", () => {
    expect(
      runDisabledReason({ connection: connection(), sql: "   \n\t ", phase: { kind: "idle" } }),
    ).toMatch(/quer/i);
  });

  it("says so when the engine was never determined", () => {
    expect(
      runDisabledReason({
        connection: connection({ engine: null }),
        sql: "select 1",
        phase: { kind: "idle" },
      }),
    ).toMatch(/engine/i);
  });

  it("gives every blocker a different sentence", () => {
    const reasons = [
      runDisabledReason({ connection: null, sql: "x", phase: { kind: "idle" } }),
      runDisabledReason({ connection: connection(), sql: "x", phase: { kind: "running" } }),
      runDisabledReason({ connection: connection(), sql: " ", phase: { kind: "idle" } }),
      runDisabledReason({
        connection: connection({ engine: null }),
        sql: "x",
        phase: { kind: "idle" },
      }),
    ];
    expect(new Set(reasons).size).toBe(4);
  });

  it("reports the running run ahead of an empty box, so Stop is the obvious next step", () => {
    expect(
      runDisabledReason({ connection: connection(), sql: "", phase: { kind: "running" } }),
    ).toMatch(/already running/i);
  });

  it("does not block on a finished, stopped, failed or refused run", () => {
    const phases = [
      { kind: "finished" } as const,
      { kind: "stopped" } as const,
      { kind: "failed", message: "m", statementIndex: null } as const,
      { kind: "refused", statementIndex: 0, reason: "r" } as const,
    ];
    for (const phase of phases) {
      expect(runDisabledReason({ connection: connection(), sql: "select 1", phase })).toBe(null);
    }
  });
});

// ---------------------------------------------------------------------------
// statusLine
// ---------------------------------------------------------------------------

const ALL_OUTCOMES: SqlTestOutcome[] = [
  { kind: "ok", serverVersion: "3.45.1" },
  { kind: "ok", serverVersion: null },
  { kind: "authFailed", message: "password authentication failed" },
  { kind: "unreachable", message: "no route to host" },
  { kind: "cannotOpenFile", message: "no such file" },
  { kind: "notADatabase", message: "file is not a database" },
  { kind: "tlsFailed", message: "certificate verify failed" },
  { kind: "timeout", afterMs: 5000 },
  { kind: "timeout", afterMs: null },
  { kind: "engineUnknown" },
  { kind: "engineUnsupported", engine: "postgres" },
  { kind: "secretUnresolved", reason: "DB_URL is not set" },
  { kind: "failed", message: "SQLSTATE HY000" },
];

describe("statusLine", () => {
  it("gives every outcome — and every sub-case — its own sentence", () => {
    const texts = ALL_OUTCOMES.map((o) => statusLine(o).text);
    expect(new Set(texts).size).toBe(ALL_OUTCOMES.length);
  });

  it("reports success, and says when the server named no version", () => {
    expect(statusLine({ kind: "ok", serverVersion: "3.45.1" }).text).toContain("3.45.1");
    expect(statusLine({ kind: "ok", serverVersion: null }).tone).toBe("ok");
    expect(statusLine({ kind: "ok", serverVersion: null }).text).toMatch(/no version/i);
  });

  it("keeps a bad password apart from an unreachable host", () => {
    expect(statusLine({ kind: "authFailed", message: "x" }).text).toMatch(/credential|password/i);
    expect(statusLine({ kind: "unreachable", message: "x" }).text).toMatch(/reach/i);
  });

  it("keeps a missing file apart from a file that is not a database", () => {
    expect(statusLine({ kind: "cannotOpenFile", message: "x" }).text).toMatch(/open/i);
    expect(statusLine({ kind: "notADatabase", message: "x" }).text).toMatch(/not a database/i);
  });

  it("keeps a driver-reported timeout apart from a measured one", () => {
    expect(statusLine({ kind: "timeout", afterMs: 5000 }).text).toContain("5000");
    expect(statusLine({ kind: "timeout", afterMs: null }).text).not.toContain("5000");
  });

  it("names the engine it does not support", () => {
    expect(statusLine({ kind: "engineUnsupported", engine: "postgres" }).text).toContain(
      "postgres",
    );
  });

  it("says nothing was tried for an unknown engine or an unresolved secret", () => {
    expect(statusLine({ kind: "engineUnknown" }).text).toMatch(/nothing was tried/i);
    expect(statusLine({ kind: "secretUnresolved", reason: "r" }).text).toMatch(
      /nothing was tried/i,
    );
  });

  it("says a `failed` is an abstention rather than filing it under a category", () => {
    const line = statusLine({ kind: "failed", message: "SQLSTATE HY000" });
    expect(line.text).toMatch(/not recognised|could not be classified/i);
    expect(line.detail).toBe("SQLSTATE HY000");
  });

  it("carries the driver's own message as detail wherever there is one", () => {
    expect(statusLine({ kind: "authFailed", message: "boom" }).detail).toBe("boom");
    expect(statusLine({ kind: "secretUnresolved", reason: "why" }).detail).toBe("why");
    expect(statusLine({ kind: "engineUnknown" }).detail).toBe(null);
  });

  it("tones only `ok` as ok", () => {
    for (const outcome of ALL_OUTCOMES) {
      expect(statusLine(outcome).tone === "ok").toBe(outcome.kind === "ok");
    }
  });

  it("never claims the connection is safe or sandboxed", () => {
    for (const outcome of ALL_OUTCOMES) {
      const line = statusLine(outcome);
      expect(`${line.text} ${line.detail ?? ""}`).not.toMatch(/\bsafe\b|sandbox/i);
    }
  });
});

// ---------------------------------------------------------------------------
// groupConnections
// ---------------------------------------------------------------------------

describe("groupConnections", () => {
  const here = connection({ id: "a", name: "Here", workspaceRoot: "C:/repo" });
  const anywhere = connection({ id: "b", name: "Anywhere", workspaceRoot: null });
  const elsewhere = connection({ id: "c", name: "Other", workspaceRoot: "C:/other" });

  it("puts this codebase's connections first and keeps the three groups apart", () => {
    const groups = groupConnections([elsewhere, anywhere, here], "C:/repo");
    expect(groups.thisCodebase.map((c) => c.id)).toEqual(["a"]);
    expect(groups.global.map((c) => c.id)).toEqual(["b"]);
    expect(groups.otherCodebases.map((c) => c.id)).toEqual(["c"]);
  });

  it("does not call another codebase's connection global", () => {
    const groups = groupConnections([elsewhere], "C:/repo");
    expect(groups.global).toEqual([]);
    expect(groups.otherCodebases).toHaveLength(1);
  });

  it("matches a root across separator and case differences", () => {
    const groups = groupConnections(
      [connection({ id: "a", workspaceRoot: "c:\\Repo\\" })],
      "C:/repo",
    );
    expect(groups.thisCodebase).toHaveLength(1);
  });

  it("does not match a root that is only a prefix of another", () => {
    const groups = groupConnections(
      [connection({ id: "a", workspaceRoot: "C:/repo2" })],
      "C:/repo",
    );
    expect(groups.thisCodebase).toEqual([]);
    expect(groups.otherCodebases).toHaveLength(1);
  });

  it("claims nothing for this codebase when no codebase is open", () => {
    const groups = groupConnections([here, anywhere, elsewhere], null);
    expect(groups.thisCodebase).toEqual([]);
    expect(groups.global.map((c) => c.id)).toEqual(["b"]);
    expect(groups.otherCodebases.map((c) => c.id)).toEqual(["a", "c"]);
  });

  it("sorts inside a group by name, then id", () => {
    const groups = groupConnections(
      [
        connection({ id: "z", name: "beta", workspaceRoot: null }),
        connection({ id: "a", name: "alpha", workspaceRoot: null }),
      ],
      "C:/repo",
    );
    expect(groups.global.map((c) => c.name)).toEqual(["alpha", "beta"]);
  });

  it("handles an empty list", () => {
    expect(groupConnections([], "C:/repo")).toEqual({
      thisCodebase: [],
      global: [],
      otherCodebases: [],
    });
  });
});

// ---------------------------------------------------------------------------
// columnWidths
// ---------------------------------------------------------------------------

const col = (name: string): SqlColumn => ({ name, typeName: null });

describe("columnWidths", () => {
  it("returns one width per column", () => {
    expect(columnWidths([col("a"), col("bb")], [])).toHaveLength(2);
  });

  it("never goes below the floor, even for a one-character header", () => {
    expect(columnWidths([col("a")], [])).toEqual([MIN_COL_CHARS]);
  });

  it("clamps to the ceiling for a very wide value", () => {
    expect(columnWidths([col("a")], [[text("x".repeat(500))]])[0]).toBe(MAX_COL_CHARS);
  });

  it("widens to fit the header when it is longer than the values", () => {
    const header = "a_rather_long_column_name";
    expect(columnWidths([col(header)], [[text("x")]])[0]).toBe(header.length);
  });

  it("measures the rendered cell, not the raw value", () => {
    // A NULL renders as a marker, not as nothing.
    expect(columnWidths([col("c")], [[{ kind: "null" }]])[0]).toBe(MIN_COL_CHARS);
    const unsupported = columnWidths(
      [col("c")],
      [[{ kind: "unsupported", typeName: "geography_multipolygon" }]],
    );
    expect(unsupported[0]).toBeGreaterThan(MIN_COL_CHARS);
  });

  it("samples only the first N rows, so a late wide row does not count", () => {
    const rows: SqlValue[][] = Array.from({ length: WIDTH_SAMPLE_ROWS + 5 }, (_, i) =>
      i < WIDTH_SAMPLE_ROWS ? [text("ab")] : [text("x".repeat(200))],
    );
    expect(columnWidths([col("c")], rows)[0]).toBe(MIN_COL_CHARS);
  });

  it("tolerates a ragged row shorter than the column list", () => {
    expect(columnWidths([col("one"), col("two_wide_header")], [[text("v")]])).toEqual([
      MIN_COL_CHARS,
      "two_wide_header".length,
    ]);
  });

  it("returns nothing for no columns", () => {
    expect(columnWidths([], [[text("x")]])).toEqual([]);
  });
});
