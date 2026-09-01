import type {
  SqlAuthMode,
  SqlCandidate,
  SqlCandidateState,
  SqlConnectionDisplay,
  SqlConnectionView,
  SqlEngine,
  SqlSecretView,
} from "../ipc/types";

function pathParts(path: string): string[] {
  return path.split(/[\\/]/).filter(Boolean);
}

function sourceContext(source: string): string {
  if (source.toLowerCase().startsWith("user secrets")) return "User secrets";
  const file = pathParts(source)[pathParts(source).length - 1] ?? source;
  const appsettings = /^appsettings(?:\.([^.]+))?\.json$/i.exec(file);
  if (appsettings !== null) return appsettings[1] ?? "Default";
  return file;
}

/** A stable, human identity that distinguishes the same key across projects and environments. */
export function candidateConnectionLabel(candidate: SqlCandidate): string {
  return [candidate.project, sourceContext(candidate.origin), candidate.name]
    .filter((part): part is string => part !== null && part.trim() !== "")
    .join(" · ");
}

/** The exact location and configuration key shown beneath a candidate label. */
export function candidateSourceDetail(candidate: SqlCandidate): string {
  switch (candidate.source.kind) {
    case "literal":
      return candidate.origin;
    case "appSettings":
    case "userSecrets":
    case "dotEnv":
      return `${candidate.origin} → ${candidate.source.key}`;
  }
}

/** Display identity for saved references, including profiles saved before composite names existed. */
export function savedConnectionLabel(connection: SqlConnectionView): string {
  if (connection.name.includes(" · ")) return connection.name;
  switch (connection.secret.kind) {
    case "literal":
      return connection.name;
    case "appSettings": {
      const parts = pathParts(connection.secret.path);
      const project = parts.at(-2) ?? null;
      return [project, sourceContext(connection.secret.path), connection.name]
        .filter((part): part is string => part !== null && part.trim() !== "")
        .join(" · ");
    }
    case "userSecrets": {
      const parts = pathParts(connection.secret.project);
      const projectFile = parts.at(-1) ?? "";
      const project = projectFile.replace(/\.csproj$/i, "");
      return [project, "User secrets", connection.name].filter(Boolean).join(" · ");
    }
    case "dotEnv": {
      const parts = pathParts(connection.secret.path);
      const project = parts.at(-2) ?? null;
      return [project, sourceContext(connection.secret.path), connection.name]
        .filter((part): part is string => part !== null && part.trim() !== "")
        .join(" · ");
    }
  }
}

/** Values collected before a literal connection has passed its required test. */
export interface ManualConnectionDraft {
  name: string;
  engine: SqlEngine | null;
  connectionString: string;
  global: boolean;
}

/** The first reason a manual connection cannot be tested and added. */
export function manualConnectionError(draft: ManualConnectionDraft): string | null {
  if (draft.name.trim() === "") return "Enter a connection name.";
  if (draft.engine === null) return "Choose a database engine.";
  if (draft.connectionString.trim() === "") return "Enter a connection string.";
  return null;
}

/**
 * The decisions behind `SqlConnectionPicker`.
 *
 * These sit here rather than in `views/sqlLogic.ts` because that module is the
 * SQL *console*'s logic — result streaming, cells, column widths, the run guard.
 * What the picker needs is a different vocabulary (a discovered candidate, a
 * redacted display, where a stored secret lives) and it is only the picker that
 * needs it.
 *
 * The governing rule throughout, from `sql/mod.rs`: **a connection string never
 * crosses IPC and is never rendered.** Only `SqlConnectionDisplay` does, and a
 * display whose `confidence` is `refused` must be reported as unreadable rather
 * than described from the fields it happens to carry — anything it reported
 * might be a slice of the password.
 */

/**
 * Why a discovered candidate cannot be connected to, or `null` when it can.
 *
 * `engineUnknown` and `unresolved` are two different problems and get two
 * different sentences: an unresolved value is still a variable reference, so
 * there is nothing to connect *to* — which is not the same as having a target
 * whose engine nobody determined.
 */
export function candidateBlocker(state: SqlCandidateState): string | null {
  switch (state.kind) {
    case "ready":
      return null;
    case "engineUnknown":
      return withReason("Engine not determined — cannot connect", state.reason);
    case "unresolved":
      return withReason("Value not resolved — there is nothing to connect to yet", state.reason);
  }
}

/** Append the backend's own reason, when it gave one worth showing. */
function withReason(sentence: string, reason: string): string {
  const trimmed = reason.trim();
  return trimmed === "" ? `${sentence}.` : `${sentence}: ${trimmed}`;
}

/** A redacted connection description, and whether it is a description at all. */
export interface DescribedDisplay {
  /** What to show. Never empty, and never any part of a connection string. */
  text: string;
  /**
   * The backend refused to describe the string. `text` then says so — it is not
   * a partial description, and the caller must not dress it up as one.
   */
  refused: boolean;
}

const AUTH_WORDING: Record<SqlAuthMode, string> = {
  integrated: "integrated authentication",
  password: "password authentication",
  // "It stated no credentials" and "this could not be read" are different
  // facts and are worded as such.
  noneStated: "states no credentials",
  unknown: "authentication not determined",
};

/**
 * Word a `SqlConnectionDisplay` for a row.
 *
 * A `refused` display yields one sentence and nothing else — not the server,
 * not the database, not the auth mode, even where the backend sent them.
 * A `described` display with every field null is a different answer again: it
 * *was* read, and it states nothing.
 */
export function describeDisplay(display: SqlConnectionDisplay): DescribedDisplay {
  if (display.confidence === "refused") {
    return {
      text: "This connection string could not be read — nothing about it is shown.",
      refused: true,
    };
  }

  const parts: string[] = [];
  if (isStated(display.server)) parts.push(display.server.trim());
  if (isStated(display.database)) parts.push(display.database.trim());
  parts.push(AUTH_WORDING[display.authMode]);

  const stated =
    !isStated(display.server) && !isStated(display.database)
      ? "no server or database stated"
      : null;

  const text = stated === null ? parts.join(" · ") : `${stated} · ${parts.join(" · ")}`;
  return { text, refused: false };
}

/**
 * Did the string state this field?
 *
 * A blank value counts as **absent**, not as a value worth showing, and this is
 * the one predicate both halves of `describeDisplay` use — they once disagreed,
 * so a display with a blank server and a null database dropped the server from
 * the parts *and* failed to raise the stated-nothing clause, silently saying
 * nothing at all about the server.
 *
 * Blank is absent because `Server=` names no server: there is nothing to render
 * but an empty segment, and an empty segment reads as "not mentioned" — the
 * exact collapse this module exists to prevent. Saying "no server or database
 * stated" is the abstention; showing a blank is a guess that the reader will
 * read as a description.
 */
function isStated(field: string | null): field is string {
  return field !== null && field.trim() !== "";
}

/**
 * Where a saved connection's string is defined — a file and a key, which is not
 * a secret and is the only thing that can be said about a reference.
 *
 * `null` for a `literal`, which points at no file. That is not a gap to fill in:
 * the stored value is the secret, so the row says *that* it holds one
 * (`SqlConnectionView.holdsASecret`) and never where it came from.
 */
export function secretOrigin(secret: SqlSecretView): string | null {
  switch (secret.kind) {
    case "literal":
      return null;
    case "appSettings":
      return `${secret.path} → ${secret.key}`;
    case "userSecrets":
      return `user secrets (${secret.project}) → ${secret.key}`;
    case "dotEnv":
      return `${secret.path} → ${secret.key}`;
  }
}
