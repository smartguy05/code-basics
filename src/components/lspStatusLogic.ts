import type { Availability, LspStatus, ServerStatus } from "../ipc/types";

/**
 * What to tell the user about the language servers, and when to say nothing.
 *
 * Every decision the status indicator makes lives here, because the component
 * cannot be tested (vitest runs in the node environment) and because the
 * decisions are the whole point: the backend went to some trouble to keep six
 * `Availability` variants apart, and a surface that renders them as one dot —
 * or worse, as a zero — throws that away. See `Availability` in `ipc/types.ts`.
 */

/** How loudly a state is worth saying. `"ok"` is not worth saying at all. */
export type LspTone = "ok" | "busy" | "warn" | "error";

/**
 * The tone of one server state.
 *
 * `notConfigured` and `unsupported` share a tone but never a sentence: one means
 * install something, the other means this server will never answer. Only the
 * wording distinguishes them, which is why `headlineFor` is not optional.
 *
 * `caveat` is the second input because **`ready` is not one state**. A server
 * promoted at the backend's readiness ceiling is `ready` — requests do proceed —
 * and every count it answers is a lower bound. This function returning `"ok"` for
 * it meant `summariseLspStatus` dropped the row and the indicator drew nothing at
 * all, while the same server's counts on screen all said they might be low. So a
 * caveat is `warn`, and only an unqualified ready server is silent.
 */
export function toneFor(state: Availability, caveat: string | null = null): LspTone {
  if (state === "ready" && caveat !== null) return "warn";
  switch (state) {
    case "ready":
      return "ok";
    case "starting":
    case "loading":
      return "busy";
    case "notConfigured":
    case "unsupported":
      return "warn";
    case "failed":
      return "error";
  }
}

/** Worst first. Used for ordering, never rendered. */
const SEVERITY: Record<LspTone, number> = { error: 3, warn: 2, busy: 1, ok: 0 };

/**
 * One sentence naming what this server is doing, in the user's terms.
 *
 * Deliberately six distinct sentences. "No language server found" and "this
 * server does not answer these questions" are the pair most at risk of being
 * collapsed, and they call for opposite actions — install one, or stop waiting
 * for an answer that is not coming.
 */
function headlineFor(language: string, state: Availability, caveat: string | null): string {
  switch (state) {
    case "ready":
      // Two sentences for one `Availability`, because the backend promotes a
      // server it gave up waiting for and the user's question about that server
      // ("why does every count say it may be low?") is answered here or nowhere.
      return caveat === null
        ? `${language}: language server ready`
        : `${language}: answers may be incomplete`;
    case "starting":
      return `${language}: language server starting…`;
    case "loading":
      return `${language}: loading projects…`;
    case "notConfigured":
      return `${language}: no language server found`;
    case "failed":
      return `${language}: language server failed`;
    case "unsupported":
      return `${language}: this server does not answer these questions`;
  }
}

/** One server, ready to render: nothing here needs deciding again. */
export interface LspServerLine {
  id: string;
  language: string;
  state: Availability;
  tone: LspTone;
  /** The sentence to show. Complete on its own, without the state beside it. */
  headline: string;
  /** The version line, the exit code, the error — whatever the backend had. */
  detail: string | null;
  /** Why this server's answers may be incomplete; `null` when they may not. */
  caveat: string | null;
  /** What the user could do about it, for `notConfigured` above all. */
  hint: string | null;
  /** Everywhere the program was looked for; empty once one was found. */
  lookedFor: string[];
}

/** What the indicator draws, or `null` when it draws nothing. */
export interface LspStatusSummary {
  /** The worst tone among the lines. Never `"ok"` — such a summary is `null`. */
  tone: Exclude<LspTone, "ok">;
  /** The collapsed label: the worst line, and how many others there are. */
  label: string;
  /** Every line, for the `title` tooltip, so hovering needs no click. */
  title: string;
  /** Worst first; ready servers are absent. Never empty. */
  lines: LspServerLine[];
  /**
   * Whether offering to restart the session is worth it: true when a server has
   * failed. A restart recovers a server that ran and then crashed; it re-runs
   * identically for a server that never started (missing binary, failed
   * handshake) or a bad configuration — hence {@link LSP_RESTART_CAVEAT}.
   */
  restartable: boolean;
}

/** What a restart can and cannot fix, shown beside the Restart action. */
export const LSP_RESTART_CAVEAT =
  "Restart recovers a server that crashed. It won't fix a missing server binary or a bad configuration — see the detail above.";

/**
 * What is worth saying about the servers right now, or `null` for nothing.
 *
 * `null` in three situations, all of them "be quiet": the status has not been
 * read (or the read failed), no server has ever been asked anything — a language
 * the workspace does not contain never starts one, and warning about it would be
 * pure noise — and every server that exists is ready.
 *
 * A ready server is dropped from `lines` rather than shown in a calmer colour.
 * It has nothing to tell anybody, and its presence would dilute the row that
 * does.
 */
export function summariseLspStatus(status: LspStatus | null): LspStatusSummary | null {
  if (!status) return null;

  const lines: LspServerLine[] = status.servers
    .map((server: ServerStatus) => ({
      id: server.id,
      language: server.language,
      state: server.state,
      tone: toneFor(server.state, server.caveat),
      headline: headlineFor(server.language, server.state, server.caveat),
      detail: server.detail,
      caveat: server.caveat,
      hint: server.hint,
      lookedFor: server.lookedFor,
    }))
    .filter((line) => line.tone !== "ok");

  // Stable within a tone: `sort` is stable in every engine this ships on, and
  // the backend's order is the only meaningful tiebreak there is.
  lines.sort((a, b) => SEVERITY[b.tone] - SEVERITY[a.tone]);

  // Destructured rather than indexed: `noUncheckedIndexedAccess` is on, and the
  // emptiness check and the first element are then one statement instead of two
  // that could drift apart.
  const [worst, ...rest] = lines;
  if (!worst) return null;
  const others = rest.length;

  return {
    tone: worst.tone as Exclude<LspTone, "ok">,
    label: others === 0 ? worst.headline : `${worst.headline} (+${others} more)`,
    // The hint belongs here as much as the detail does: it is the one actionable
    // string the backend composes ("Install it with `npm i -g …`"), and reaching
    // it only by opening the dropdown means hovering "no language server found"
    // says nothing about what to do about it.
    title: lines
      .map((line) =>
        [line.headline, line.caveat, line.detail, line.hint].filter(Boolean).join(" — "),
      )
      .join("\n"),
    lines,
    restartable: lines.some((line) => line.state === "failed"),
  };
}

/**
 * How many reads to take before believing an empty server list.
 *
 * A server is started by the `didOpen` the editor sends, so the read that
 * follows a file opening can legitimately arrive before the row exists. Stopping
 * there would leave a starting server invisible until the next file change —
 * which is exactly the silence this surface exists to break.
 */
export const MIN_LSP_READS = 3;

/**
 * Whether to read the status again, following `symbolIndexStatus`'s precedent in
 * `SearchEverywhere`: a state in flight earns another call, a settled one does
 * not.
 *
 * `attempt` is the number of reads that have completed, so a caller that has
 * read once passes 1. A failed read (`null`) stops: the status surface is not
 * worth a retry loop of its own, and the next file the user opens re-arms it.
 */
export function shouldPollLspAgain(status: LspStatus | null, attempt: number): boolean {
  if (!status) return false;
  if (status.servers.some((server) => toneFor(server.state) === "busy")) return true;
  return attempt < MIN_LSP_READS;
}

/** How often to re-read while something is still in flight. */
export const LSP_POLL_FAST_MS = 700;

/**
 * How often to re-read once everything has settled.
 *
 * Slow, because nothing is expected to change — and not `null`, because things
 * do: a server that was `ready` at the last read can die at any moment, and this
 * indicator shows *nothing at all* for an all-ready status, so a crash after the
 * poll stopped would be invisible here until the open-file set happened to change.
 * A caveat is withdrawn the same way: the backend upgrades a ceiling-promoted
 * server to plain `ready` if the signal it gave up on eventually arrives, and this
 * is the only thing that would notice.
 * The same interval also covers a server that has not registered yet, which is the
 * other way {@link MIN_LSP_READS} can run out too early: the row is created by the
 * editor's `didOpen`, and that waits on a file read from disk.
 */
export const LSP_POLL_SLOW_MS = 5000;

/**
 * How long to wait before reading the status again, or `null` to stop.
 *
 * Wraps {@link shouldPollLspAgain} rather than re-deciding what it decides: while
 * it says yes the reads are fast, and afterwards they continue slowly for as long
 * as there is a file open to have started a server.
 *
 * `watching` is that condition — no open file means nothing can start a server,
 * and a poll for the life of the app would be a cost with no reader.
 */
export function lspPollDelay(
  status: LspStatus | null,
  attempt: number,
  watching: boolean,
): number | null {
  if (!status || !watching) return null;
  return shouldPollLspAgain(status, attempt) ? LSP_POLL_FAST_MS : LSP_POLL_SLOW_MS;
}
