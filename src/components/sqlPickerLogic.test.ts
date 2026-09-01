import { describe, expect, it } from "vitest";
import type {
  SqlCandidateState,
  SqlConnectionDisplay,
  SqlSecretView,
} from "../ipc/types";
import {
  candidateBlocker,
  candidateConnectionLabel,
  candidateSourceDetail,
  describeDisplay,
  manualConnectionError,
  secretOrigin,
  savedConnectionLabel,
} from "./sqlPickerLogic";

describe("connection labels", () => {
  const candidate = {
    id: "appsettings:api:db",
    name: "DatabaseConnection",
    origin: "ONEflight.Server.Api/appsettings.Development.json",
    project: "ONEflight.Server.Api",
    engine: "postgres" as const,
    source: {
      kind: "appSettings" as const,
      path: "C:/repo/ONEflight.Server.Api/appsettings.Development.json",
      key: "AppConfiguration:ConnectionStrings:DatabaseConnection",
    },
    display: {
      engine: "postgres" as const,
      server: "db.example:5432",
      database: "orders",
      authMode: "password" as const,
      confidence: "described" as const,
    },
    state: { kind: "ready" as const },
  };

  it("puts project and environment ahead of a generic candidate key", () => {
    expect(candidateConnectionLabel(candidate)).toBe(
      "ONEflight.Server.Api · Development · DatabaseConnection",
    );
  });

  it("shows the exact origin and configuration key separately", () => {
    expect(candidateSourceDetail(candidate)).toContain("appsettings.Development.json");
    expect(candidateSourceDetail(candidate)).toContain(
      "AppConfiguration:ConnectionStrings:DatabaseConnection",
    );
  });

  it("upgrades the display label of an older generically named saved reference", () => {
    expect(
      savedConnectionLabel({
        id: "saved",
        name: "DatabaseConnection",
        engine: "postgres",
        secret: {
          kind: "appSettings",
          path: "C:/repo/ONEflight.Server.Api/appsettings.Staging.json",
          key: "AppConfiguration:ConnectionStrings:DatabaseConnection",
        },
        holdsASecret: false,
        workspaceRoot: "C:/repo",
        allowWrites: false,
        createdAtMs: 0,
        lastUsedMs: null,
      }),
    ).toBe("ONEflight.Server.Api · Staging · DatabaseConnection");
  });
});

describe("manualConnectionError", () => {
  const valid = {
    name: "Orders",
    engine: "postgres" as const,
    connectionString: "Host=localhost;Database=orders",
    global: false,
  };

  it("requires a name, engine, and connection string", () => {
    expect(manualConnectionError({ ...valid, name: "  " })).toMatch(/name/i);
    expect(manualConnectionError({ ...valid, engine: null })).toMatch(/engine/i);
    expect(manualConnectionError({ ...valid, connectionString: "  " })).toMatch(/string/i);
  });

  it("accepts either codebase or global scope without rewriting the string", () => {
    expect(manualConnectionError(valid)).toBe(null);
    expect(manualConnectionError({ ...valid, global: true })).toBe(null);
  });
});

// ---------------------------------------------------------------------------
// candidateBlocker
// ---------------------------------------------------------------------------

describe("candidateBlocker", () => {
  it("blocks nothing when the candidate is ready", () => {
    expect(candidateBlocker({ kind: "ready" })).toBe(null);
  });

  it("keeps an undetermined engine apart from an unresolved value", () => {
    const engine = candidateBlocker({ kind: "engineUnknown", reason: "no scheme" }) ?? "";
    const unresolved = candidateBlocker({ kind: "unresolved", reason: "no scheme" }) ?? "";
    expect(engine).not.toBe(unresolved);
    expect(engine).toMatch(/engine/i);
    expect(unresolved).toMatch(/resolv/i);
  });

  it("carries the backend's own reason through", () => {
    expect(candidateBlocker({ kind: "unresolved", reason: "${DB_URL} is not set" })).toContain(
      "${DB_URL} is not set",
    );
  });

  it("still says why when the backend gave no reason", () => {
    const blocked = candidateBlocker({ kind: "engineUnknown", reason: "   " }) ?? "";
    expect(blocked.trim().length).toBeGreaterThan(0);
    expect(blocked).toMatch(/engine/i);
  });

  it("gives every non-ready state a sentence of its own", () => {
    const states: SqlCandidateState[] = [
      { kind: "engineUnknown", reason: "" },
      { kind: "unresolved", reason: "" },
    ];
    const texts = states.map((s) => candidateBlocker(s));
    expect(new Set(texts).size).toBe(states.length);
  });
});

// ---------------------------------------------------------------------------
// describeDisplay
// ---------------------------------------------------------------------------

const display = (over: Partial<SqlConnectionDisplay> = {}): SqlConnectionDisplay => ({
  engine: "postgres",
  server: "db.example:5432",
  database: "orders",
  authMode: "password",
  confidence: "described",
  ...over,
});

describe("describeDisplay", () => {
  it("says nothing about a refused string, not even the fields it carries", () => {
    const described = describeDisplay(
      display({ confidence: "refused", server: "db.example", database: "orders" }),
    );
    expect(described.refused).toBe(true);
    expect(described.text).not.toContain("db.example");
    expect(described.text).not.toContain("orders");
    expect(described.text).toMatch(/could not be read/i);
  });

  it("describes a server and database when both were read", () => {
    const described = describeDisplay(display());
    expect(described.refused).toBe(false);
    expect(described.text).toContain("db.example:5432");
    expect(described.text).toContain("orders");
  });

  it("keeps a read-but-empty description apart from a refusal", () => {
    const empty = describeDisplay(
      display({ server: null, database: null, authMode: "unknown" }),
    );
    const refused = describeDisplay(display({ confidence: "refused" }));
    expect(empty.text).not.toBe(refused.text);
    expect(empty.refused).toBe(false);
    expect(empty.text.trim().length).toBeGreaterThan(0);
  });

  // A blank field and an absent one are the same absence here (see
  // `describeDisplay`): both must raise the stated-nothing clause, and neither
  // may be shown as a value. The four combinations are pinned because the two
  // checks in that function drifted apart once already.
  it("says nothing was stated when both fields are null", () => {
    const described = describeDisplay(display({ server: null, database: null }));
    expect(described.text).toMatch(/no server or database stated/i);
  });

  it("says nothing was stated when both fields are blank", () => {
    const described = describeDisplay(display({ server: "", database: "   " }));
    expect(described.text).toMatch(/no server or database stated/i);
    expect(described.text).toBe(describeDisplay(display({ server: null, database: null })).text);
  });

  it("says nothing was stated when one field is blank and the other null", () => {
    const blankServer = describeDisplay(display({ server: "", database: null }));
    const blankDatabase = describeDisplay(display({ server: null, database: "  " }));
    expect(blankServer.text).toMatch(/no server or database stated/i);
    expect(blankDatabase.text).toMatch(/no server or database stated/i);
  });

  it("does not claim nothing was stated when a blank field sits beside a real one", () => {
    const described = describeDisplay(display({ server: "", database: "orders" }));
    expect(described.text).not.toMatch(/no server or database stated/i);
    expect(described.text).toContain("orders");
    // The blank field contributes no empty segment of its own.
    expect(described.text).not.toMatch(/(^|·)\s*·/);
  });

  it("keeps 'states no credentials' apart from 'auth not determined'", () => {
    const none = describeDisplay(display({ authMode: "noneStated" })).text;
    const unknown = describeDisplay(display({ authMode: "unknown" })).text;
    expect(none).not.toBe(unknown);
  });

  it("gives every auth mode its own wording", () => {
    const modes = ["integrated", "password", "noneStated", "unknown"] as const;
    const texts = modes.map((authMode) => describeDisplay(display({ authMode })).text);
    expect(new Set(texts).size).toBe(modes.length);
  });

  it("names a file-backed database with no server", () => {
    const described = describeDisplay(
      display({ engine: "sqlite", server: null, database: "./app.db", authMode: "noneStated" }),
    );
    expect(described.text).toContain("./app.db");
  });

  it("never claims a connection is safe or sandboxed", () => {
    const all: SqlConnectionDisplay[] = [
      display(),
      display({ confidence: "refused" }),
      display({ server: null, database: null, authMode: "unknown" }),
      display({ authMode: "integrated" }),
      display({ authMode: "noneStated" }),
    ];
    for (const d of all) {
      expect(describeDisplay(d).text).not.toMatch(/\bsafe\b|sandbox/i);
    }
  });
});

// ---------------------------------------------------------------------------
// secretOrigin
// ---------------------------------------------------------------------------

describe("secretOrigin", () => {
  it("names the file and key a reference points at", () => {
    const origin = secretOrigin({ kind: "appSettings", path: "src/appsettings.json", key: "Default" }) ?? "";
    expect(origin).toContain("src/appsettings.json");
    expect(origin).toContain("Default");
  });

  it("names the project a user-secrets reference belongs to", () => {
    const origin = secretOrigin({ kind: "userSecrets", project: "Api", key: "Db" }) ?? "";
    expect(origin).toContain("Api");
    expect(origin).toContain("Db");
    expect(origin).toMatch(/secret/i);
  });

  it("names the dotenv file and key", () => {
    const origin = secretOrigin({ kind: "dotEnv", path: ".env.local", key: "DATABASE_URL" }) ?? "";
    expect(origin).toContain(".env.local");
    expect(origin).toContain("DATABASE_URL");
  });

  it("has no origin for a stored literal — there is no file to name", () => {
    const literal: SqlSecretView = {
      kind: "literal",
      display: display(),
    };
    expect(secretOrigin(literal)).toBe(null);
  });

  it("never quotes a connection string for a literal", () => {
    const literal: SqlSecretView = { kind: "literal", display: display() };
    expect(secretOrigin(literal)).toBe(null);
  });
});
