import { describe, it, expect } from "vitest";
import type { SqlCandidate, SqlConnectionView, SqlEngine } from "../ipc/types";
import type { SqlRunPhase } from "./sqlLogic";
import {
  enforcementBadge,
  keepSelection,
  mintQueryId,
  phaseLine,
  profileFromCandidate,
  profileFromManual,
  runTarget,
  selectedConnection,
  statementTitle,
  stopLine,
  stoppedNote,
  writesConfirm,
} from "./sqlViewLogic";

function connection(overrides: Partial<SqlConnectionView> = {}): SqlConnectionView {
  return {
    id: "c1",
    name: "Local",
    engine: "sqlite",
    secret: { kind: "dotEnv", path: ".env", key: "DB" },
    holdsASecret: false,
    workspaceRoot: null,
    allowWrites: false,
    createdAtMs: 0,
    lastUsedMs: null,
    ...overrides,
  };
}

describe("enforcementBadge", () => {
  it("says nothing at all when no connection is chosen", () => {
    // An empty badge beside a bar with no connection would read as "read-only",
    // which is a claim about a database nobody has picked yet.
    expect(enforcementBadge(null)).toBeNull();
  });

  it("mirrors the driver's own wording for a SQLite read-only connection", () => {
    // The string is `ReadOnlyEnforcement::Driver.label()` verbatim. If the Rust
    // label changes, this fails — which is the whole point of pinning it.
    const badge = enforcementBadge(connection({ engine: "sqlite" }));
    expect(badge?.tone).toBe("driver");
    expect(badge?.label).toBe("Opened read-only by the driver");
    expect(badge?.detail).toContain("SQLITE_OPEN_READONLY");
  });

  it("keeps a guard-only engine apart from a driver-enforced one", () => {
    // Postgres has no read-only open mode here, so only the text check stands.
    // Rendering this the same as SQLite would promise a guarantee it has not.
    const badge = enforcementBadge(connection({ engine: "postgres" }));
    expect(badge?.tone).toBe("guard");
    expect(badge?.label).toBe("Read-only by text check only — this connection can write");
    expect(badge?.label).not.toBe(enforcementBadge(connection({ engine: "sqlite" }))?.label);
  });

  it("abstains when the engine was never determined", () => {
    // There is no driver yet, so which of the two read-only promises applies is
    // unknown — and guessing the stronger one is the dangerous guess.
    const badge = enforcementBadge(connection({ engine: null }));
    expect(badge?.tone).toBe("undetermined");
    expect(badge?.label).toContain("not determined");
  });

  it("says writes are allowed, whatever the engine could have enforced", () => {
    const badge = enforcementBadge(connection({ engine: "sqlite", allowWrites: true }));
    expect(badge?.tone).toBe("writes");
    expect(badge?.label).toBe("Writes are allowed on this connection");
  });

  it("never claims a connection is safe or sandboxed", () => {
    const engines: (SqlEngine | null)[] = ["sqlite", "postgres", "sqlServer", null];
    for (const engine of engines) {
      for (const allowWrites of [false, true]) {
        const badge = enforcementBadge(connection({ engine, allowWrites }));
        const text = `${badge?.label} ${badge?.detail}`.toLowerCase();
        expect(text.includes("sandbox") && !text.includes("not a database-enforced sandbox")).toBe(
          false,
        );
        expect(text).not.toContain("is safe");
        expect(text).not.toContain("guaranteed read-only");
      }
    }
  });
});

describe("writesConfirm", () => {
  it("owes no confirmation for turning writes off", () => {
    // Restoring a protection is the safe direction; a modal there is a modal
    // people learn to click through, and the other direction cannot afford that.
    expect(writesConfirm(connection({ allowWrites: true }), false)).toBeNull();
  });

  it("always states that the guard is a heuristic over the text, not a sandbox", () => {
    const confirm = writesConfirm(connection({ engine: "postgres" }), true);
    expect(confirm?.guard).toContain("heuristic");
    expect(confirm?.guard).toContain("not a database-enforced sandbox");
  });

  it("says separately what a SQLite connection gives up", () => {
    // The driver-enforced promise is a different, stronger thing from the guard,
    // so it is its own paragraph and not folded into the guard sentence.
    const confirm = writesConfirm(connection({ engine: "sqlite" }), true);
    expect(confirm?.driverGiveUp).toContain("SQLITE_OPEN_READONLY");
    expect(confirm?.guard).not.toContain("SQLITE_OPEN_READONLY");
  });

  it("invents no driver protection for an engine that has none", () => {
    expect(writesConfirm(connection({ engine: "postgres" }), true)?.driverGiveUp).toBeNull();
    expect(writesConfirm(connection({ engine: null }), true)?.driverGiveUp).toBeNull();
  });

  it("names the connection it is about", () => {
    expect(writesConfirm(connection({ name: "prod" }), true)?.title).toContain("prod");
  });

  it("pins the lead sentence for the driver paragraph rather than leaving it in the view", () => {
    // It was a literal in SqlView. It is the strongest safety sentence in the
    // console, and it rendered for *any* engine with a driver give-up — so a
    // future engine with a weaker guarantee would have inherited the word
    // "protected" with nothing pinning it.
    const confirm = writesConfirm(connection({ engine: "sqlite" }), true);
    expect(confirm?.driverGiveUpLead).toBe("This connection is protected by more than the guard.");
  });

  it("gives no lead sentence to an engine with no driver-level guarantee", () => {
    for (const engine of ["postgres", "sqlServer", null] as (SqlEngine | null)[]) {
      const confirm = writesConfirm(connection({ engine }), true);
      expect(confirm?.driverGiveUpLead).toBeNull();
      expect(confirm?.driverGiveUp).toBeNull();
    }
  });

  it("keeps the lead and the give-up together — neither may appear without the other", () => {
    for (const engine of ["sqlite", "postgres", "sqlServer", null] as (SqlEngine | null)[]) {
      const confirm = writesConfirm(connection({ engine }), true);
      expect(confirm?.driverGiveUpLead === null).toBe(confirm?.driverGiveUp === null);
    }
  });
});

describe("runTarget", () => {
  it("refuses rather than falling back to the whole editor when nothing is selected", () => {
    // The bug this prevents: pressing *run selection* and having the console run
    // a script the user meant to skip past.
    const target = runTarget("selection", "select 1;\nselect 2;", "   \n ");
    expect(target.kind).toBe("refused");
    if (target.kind === "refused") expect(target.reason).toContain("Nothing is selected");
  });

  it("sends the selection exactly as it stands", () => {
    // Not trimmed: the guard parses what is sent, so the text must not be
    // rewritten on the way to it.
    expect(runTarget("selection", "all of it", "  select 1  ")).toEqual({
      kind: "run",
      sql: "  select 1  ",
    });
  });

  it("refuses an empty editor with its own reason", () => {
    const target = runTarget("all", "  \n\t", "");
    expect(target).toEqual({ kind: "refused", reason: "Type a query to run." });
  });

  it("runs the whole text when there is some", () => {
    expect(runTarget("all", "select 1", "select 1")).toEqual({ kind: "run", sql: "select 1" });
  });
});

describe("mintQueryId", () => {
  it("is unique across both a repeated clock and a restarted counter", () => {
    expect(mintQueryId(1, 1000)).toBe("sql-1000-1");
    expect(mintQueryId(2, 1000)).not.toBe(mintQueryId(1, 1000));
    expect(mintQueryId(1, 1001)).not.toBe(mintQueryId(1, 1000));
  });
});

describe("phaseLine", () => {
  const phases: SqlRunPhase[] = [
    { kind: "idle" },
    { kind: "running" },
    { kind: "finished" },
    { kind: "stopped" },
    { kind: "failed", message: "boom", statementIndex: 0 },
    { kind: "refused", statementIndex: 0, reason: "not recognised as a read" },
  ];

  it("gives every phase its own sentence", () => {
    const texts = phases.map((p) => phaseLine(p).text);
    expect(new Set(texts).size).toBe(phases.length);
  });

  it("does not word a refusal as a database failure", () => {
    // Nothing reached the database, so the reader must not be sent to the server.
    const line = phaseLine({ kind: "refused", statementIndex: 1, reason: "unparseable" });
    expect(line.tone).toBe("warn");
    expect(line.text).toContain("was not sent");
    expect(line.text).toContain("Statement 2");
    expect(line.text).not.toContain("failed");
  });

  it("keeps a connection failure apart from a statement failure", () => {
    const noStatement = phaseLine({ kind: "failed", message: "no file", statementIndex: null });
    expect(noStatement.text).toContain("nothing ran");
    const oneStatement = phaseLine({ kind: "failed", message: "no table", statementIndex: 2 });
    expect(oneStatement.text).toContain("Statement 3 failed");
  });

  it("says a stopped run is partial rather than finished", () => {
    expect(phaseLine({ kind: "stopped" }).text).toContain("partial");
    expect(phaseLine({ kind: "stopped" }).tone).not.toBe("ok");
  });
});

describe("statementTitle", () => {
  it("counts from one", () => {
    expect(statementTitle(0)).toBe("Statement 1");
  });
});

describe("stopLine", () => {
  it("keeps the three outcomes apart", () => {
    const texts = (["signalled", "alreadyStopping", "notFound"] as const).map(
      (o) => stopLine(o).text,
    );
    expect(new Set(texts).size).toBe(3);
  });

  it("does not claim the server stopped", () => {
    expect(stopLine("signalled").text).toContain("the server may still be running");
  });

  it("treats the lost race as ordinary, not as an error", () => {
    // Clicking Stop just as a statement finishes is normal, and an error tone
    // would make a working feature look broken.
    expect(stopLine("notFound").tone).toBe("idle");
  });
});

describe("keepSelection", () => {
  it("drops a selection the store has forgotten", () => {
    expect(keepSelection([connection({ id: "b" })], "a")).toBeNull();
  });

  it("keeps one that survived", () => {
    expect(keepSelection([connection({ id: "a" })], "a")).toBe("a");
  });

  it("does not invent a selection out of an empty one", () => {
    expect(keepSelection([connection({ id: "a" })], null)).toBeNull();
  });
});

describe("selectedConnection", () => {
  it("never falls back to the first connection", () => {
    expect(selectedConnection([connection({ id: "a" })], null)).toBeNull();
    expect(selectedConnection([connection({ id: "a" })], "gone")).toBeNull();
  });
});

describe("profileFromCandidate", () => {
  const candidate: SqlCandidate = {
    id: "env:DB",
    name: "DB",
    origin: ".env",
    project: null,
    engine: "sqlite",
    source: { kind: "dotEnv", path: ".env", key: "DB" },
    display: {
      engine: "sqlite",
      server: null,
      database: "app.db",
      authMode: "noneStated",
      confidence: "described",
    },
    state: { kind: "ready" },
  };

  it("saves with writes disallowed, whatever else is true", () => {
    // Consent moves only through `sql_set_allow_writes`; a save must never be a
    // route to it.
    expect(profileFromCandidate(candidate, "C:/repo", [], 5).allowWrites).toBe(false);
  });

  it("carries the reference rather than any value, and ties it to the codebase", () => {
    const profile = profileFromCandidate(candidate, "C:/repo", [], 5);
    expect(profile.secret).toEqual({ kind: "dotEnv", path: ".env", key: "DB" });
    expect(profile.workspaceRoot).toBe("C:/repo");
    expect(profile.createdAtMs).toBe(5);
    expect(profile.lastUsedMs).toBeNull();
  });

  it("does not overwrite an existing connection that shares the id", () => {
    // `sql_save_connection` is an upsert, so a collision would silently replace
    // a connection the user set up rather than adding the one they clicked.
    expect(profileFromCandidate(candidate, null, ["env:DB"], 5).id).toBe("env:DB-2");
    expect(profileFromCandidate(candidate, null, ["env:DB", "env:DB-2"], 5).id).toBe("env:DB-3");
  });

  it("uses an explicit engine for a candidate whose DSN was ambiguous", () => {
    const unknown = {
      ...candidate,
      engine: null,
      state: { kind: "engineUnknown", reason: "" } as const,
    };
    expect(profileFromCandidate(unknown, "C:/repo", [], 5, "postgres").engine).toBe("postgres");
  });
});

describe("profileFromManual", () => {
  const draft = {
    name: "  Orders  ",
    engine: "postgres" as const,
    connectionString: " Host=localhost;Database=orders ",
    global: false,
  };

  it("stores the exact literal under the current codebase and starts read-only", () => {
    const profile = profileFromManual(draft, "C:/repo", [], 50);
    expect(profile).toMatchObject({
      id: "manual:50",
      name: "Orders",
      engine: "postgres",
      workspaceRoot: "C:/repo",
      allowWrites: false,
      secret: { kind: "literal", connectionString: draft.connectionString },
    });
  });

  it("can be global and avoids existing ids", () => {
    const profile = profileFromManual(
      { ...draft, global: true },
      "C:/repo",
      ["manual:50", "manual:50-2"],
      50,
    );
    expect(profile.id).toBe("manual:50-3");
    expect(profile.workspaceRoot).toBe(null);
  });
});

describe("stoppedNote", () => {
  const stopped: SqlRunPhase = { kind: "stopped" };

  it("marks the result a stop cut short, so a partial answer cannot read as complete", () => {
    // The backend sends `Completed` on the stopped path too, with no row cap —
    // so the grid otherwise holds a real completion and says "300 rows" with no
    // sign at all that reading ended early.
    const note = stoppedNote(stopped, 0, 0);
    expect(note).not.toBeNull();
    expect(note?.header.toLowerCase()).toContain("stopped");
    expect(note?.row.toLowerCase()).toContain("stopped");
  });

  it("never claims the whole answer is on screen, and never claims rows are certainly missing", () => {
    // A stop can land after the last row as easily as before it. \"Rows are
    // missing\" would be a guess; \"may be\" is what is known.
    const note = stoppedNote(stopped, 0, 0);
    expect(note?.row).toContain("may be");
    expect(note?.row.toLowerCase()).not.toContain("complete result");
  });

  it("says nothing for a run that finished, failed or was refused", () => {
    const others: SqlRunPhase[] = [
      { kind: "idle" },
      { kind: "running" },
      { kind: "finished" },
      { kind: "failed", message: "boom", statementIndex: 0 },
      { kind: "refused", statementIndex: 0, reason: "no" },
    ];
    for (const phase of others) {
      expect(stoppedNote(phase, 0, 0)).toBeNull();
    }
  });

  it("leaves the statements that had already completed alone", () => {
    // Statements run in order, so the ones before the last had reported their
    // own completion before the stop landed. Labelling those partial is exactly
    // as wrong as labelling the stopped one complete.
    expect(stoppedNote(stopped, 0, 2)).toBeNull();
    expect(stoppedNote(stopped, 1, 2)).toBeNull();
    expect(stoppedNote(stopped, 2, 2)).not.toBeNull();
  });
});
