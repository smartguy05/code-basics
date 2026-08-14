import { describe, expect, it } from "vitest";
import {
  LSP_POLL_FAST_MS,
  LSP_POLL_SLOW_MS,
  MIN_LSP_READS,
  lspPollDelay,
  summariseLspStatus,
  shouldPollLspAgain,
  toneFor,
} from "./lspStatusLogic";
import type { Availability, LspStatus, ServerStatus } from "../ipc/types";

/** A server row with everything absent, so each test states only what it means. */
function server(over: Partial<ServerStatus> & { state: Availability }): ServerStatus {
  return {
    id: "typescript",
    language: "TypeScript",
    detail: null,
    caveat: null,
    lookedFor: [],
    hint: null,
    ...over,
  };
}

const status = (...servers: ServerStatus[]): LspStatus => ({ servers });

describe("toneFor", () => {
  it("gives every availability variant exactly one tone", () => {
    expect(toneFor("ready")).toBe("ok");
    expect(toneFor("starting")).toBe("busy");
    expect(toneFor("loading")).toBe("busy");
    expect(toneFor("notConfigured")).toBe("warn");
    expect(toneFor("unsupported")).toBe("warn");
    expect(toneFor("failed")).toBe("error");
  });

  it("does not call a server with a caveat plainly ok", () => {
    // `ready` is the state the whole surface is built to stay quiet about, and a
    // ceiling-promoted server is `ready` — so without this the one state the
    // indicator was added to explain is the one state it cannot say.
    expect(toneFor("ready", "the projects never finished loading")).toBe("warn");
    expect(toneFor("ready", null)).toBe("ok");
  });
});

describe("summariseLspStatus", () => {
  it("says nothing at all when the status has not been read yet", () => {
    expect(summariseLspStatus(null)).toBeNull();
  });

  it("says nothing when no server has ever been asked anything", () => {
    // A workspace holding no file any configured server serves never starts
    // one, so `servers` is empty — and a warning about a language that is not
    // present is the noise this rule exists to prevent.
    expect(summariseLspStatus(status())).toBeNull();
  });

  it("says nothing when every server is ready", () => {
    expect(
      summariseLspStatus(
        status(server({ state: "ready" }), server({ id: "csharp", state: "ready" })),
      ),
    ).toBeNull();
  });

  it("speaks up about a ready server whose answers may be incomplete", () => {
    // The observed failure: Roslyn promoted at the 90 s ceiling. Every count in
    // the file said it might be low, and the titlebar showed nothing at all —
    // byte-identical to a healthy workspace.
    const summary = summariseLspStatus(
      status(
        server({
          id: "csharp",
          language: "C#",
          state: "ready",
          detail: "Microsoft.CodeAnalysis.LanguageServer.exe",
          caveat: "the server did not send `workspace/projectInitializationComplete` within 90s",
        }),
      ),
    );

    expect(summary).not.toBeNull();
    expect(summary?.tone).toBe("warn");
    expect(summary?.lines).toHaveLength(1);
    expect(summary?.lines[0]?.caveat).toBe(
      "the server did not send `workspace/projectInitializationComplete` within 90s",
    );
    expect(summary?.label).not.toBe("C#: language server ready");
    // The reason has to be readable without opening anything, like the hint is.
    expect(summary?.title).toContain("projectInitializationComplete");
  });

  it("still says nothing about a ready server that carries no caveat", () => {
    expect(
      summariseLspStatus(status(server({ state: "ready", detail: "tsserver 5.4" }))),
    ).toBeNull();
  });

  it("shows a missing server with its hint, never a count", () => {
    const summary = summariseLspStatus(
      status(
        server({
          id: "typescript",
          language: "TypeScript",
          state: "notConfigured",
          detail: "typescript-language-server was not found",
          hint: "Install it with `npm i -g typescript-language-server typescript`",
          lookedFor: ["C:/tools/typescript-language-server.cmd"],
        }),
      ),
    );

    expect(summary).not.toBeNull();
    expect(summary?.tone).toBe("warn");
    expect(summary?.label).toBe("TypeScript: no language server found");
    expect(summary?.lines).toHaveLength(1);
    expect(summary?.lines[0]?.hint).toBe(
      "Install it with `npm i -g typescript-language-server typescript`",
    );
    expect(summary?.lines[0]?.lookedFor).toEqual([
      "C:/tools/typescript-language-server.cmd",
    ]);
    expect(summary?.title).toContain("typescript-language-server was not found");
  });

  it("shows a dead server as an error carrying its message", () => {
    const summary = summariseLspStatus(
      status(server({ id: "csharp", language: "C#", state: "failed", detail: "exit code 134" })),
    );

    expect(summary?.tone).toBe("error");
    expect(summary?.label).toBe("C#: language server failed");
    expect(summary?.lines[0]?.detail).toBe("exit code 134");
  });

  it("distinguishes starting from loading, and neither reads as an answer", () => {
    expect(summariseLspStatus(status(server({ state: "starting" })))?.label).toBe(
      "TypeScript: language server starting…",
    );
    expect(summariseLspStatus(status(server({ state: "loading" })))?.label).toBe(
      "TypeScript: loading projects…",
    );
    expect(summariseLspStatus(status(server({ state: "loading" })))?.tone).toBe("busy");
  });

  it("does not let an unsupported capability read as 'there are none'", () => {
    const summary = summariseLspStatus(
      status(server({ id: "python", language: "Python", state: "unsupported" })),
    );

    expect(summary?.tone).toBe("warn");
    expect(summary?.label).toBe("Python: this server does not answer these questions");
  });

  it("leads with the worst state and counts the rest", () => {
    const summary = summariseLspStatus(
      status(
        server({ id: "typescript", language: "TypeScript", state: "loading" }),
        server({ id: "python", language: "Python", state: "notConfigured" }),
        server({ id: "csharp", language: "C#", state: "failed" }),
      ),
    );

    expect(summary?.tone).toBe("error");
    expect(summary?.label).toBe("C#: language server failed (+2 more)");
    // Worst first, so the row that needs acting on is not below two that do not.
    expect(summary?.lines.map((line) => line.id)).toEqual([
      "csharp",
      "python",
      "typescript",
    ]);
  });

  it("keeps a ready server out of the list but still speaks for its neighbour", () => {
    const summary = summariseLspStatus(
      status(
        server({ id: "typescript", language: "TypeScript", state: "ready" }),
        server({ id: "csharp", language: "C#", state: "starting" }),
      ),
    );

    expect(summary?.tone).toBe("busy");
    expect(summary?.label).toBe("C#: language server starting…");
    expect(summary?.lines.map((line) => line.id)).toEqual(["csharp"]);
  });

  it("keeps two rows of one tone in the order the backend listed them", () => {
    const summary = summariseLspStatus(
      status(
        server({ id: "python", language: "Python", state: "notConfigured" }),
        server({ id: "typescript", language: "TypeScript", state: "unsupported" }),
      ),
    );

    expect(summary?.lines.map((line) => line.id)).toEqual(["python", "typescript"]);
    expect(summary?.label).toBe("Python: no language server found (+1 more)");
  });

  it("puts every line, and its detail, into the tooltip", () => {
    const summary = summariseLspStatus(
      status(
        server({ id: "csharp", language: "C#", state: "failed", detail: "exit code 134" }),
        server({ id: "python", language: "Python", state: "notConfigured" }),
      ),
    );

    expect(summary?.title).toBe(
      "C#: language server failed — exit code 134\nPython: no language server found",
    );
  });

  it("puts the hint in the tooltip too, because it is the only actionable line", () => {
    // `hint` is the install command the backend composes. Reaching it only by
    // opening the dropdown means hovering "no language server found" says nothing
    // about what to do about it.
    const summary = summariseLspStatus(
      status(
        server({
          id: "typescript",
          language: "TypeScript",
          state: "notConfigured",
          detail: "typescript-language-server was not found",
          hint: "Install it with `npm i -g typescript-language-server typescript`",
        }),
      ),
    );

    expect(summary?.title).toBe(
      "TypeScript: no language server found — typescript-language-server was not found — " +
        "Install it with `npm i -g typescript-language-server typescript`",
    );
  });
});

describe("shouldPollLspAgain", () => {
  it("keeps reading while a server is starting or loading", () => {
    expect(shouldPollLspAgain(status(server({ state: "starting" })), 99)).toBe(true);
    expect(shouldPollLspAgain(status(server({ state: "loading" })), 99)).toBe(true);
  });

  it("stops once every server has settled", () => {
    expect(
      shouldPollLspAgain(status(server({ state: "ready" })), MIN_LSP_READS),
    ).toBe(false);
    expect(
      shouldPollLspAgain(status(server({ state: "failed" })), MIN_LSP_READS),
    ).toBe(false);
  });

  it("reads a few more times when nothing has appeared yet", () => {
    // A server is started by the `didOpen` the editor sends, so the first read
    // after a file opens can legitimately see an empty list. Stopping there
    // would leave a starting server invisible until the next file change.
    expect(shouldPollLspAgain(status(), 1)).toBe(true);
    expect(shouldPollLspAgain(status(), MIN_LSP_READS - 1)).toBe(true);
    expect(shouldPollLspAgain(status(), MIN_LSP_READS)).toBe(false);
  });

  it("stops when the status call itself failed", () => {
    expect(shouldPollLspAgain(null, 1)).toBe(false);
  });
});

describe("lspPollDelay", () => {
  it("reads quickly while a server is still coming up", () => {
    expect(lspPollDelay(status(server({ state: "starting" })), 99, true)).toBe(LSP_POLL_FAST_MS);
    expect(lspPollDelay(status(), 1, true)).toBe(LSP_POLL_FAST_MS);
  });

  it("keeps watching slowly after everything has settled", () => {
    // The failure this closes: a server that was ready at the last read and dies
    // later. `shouldPollLspAgain` stops, and the indicator — which shows nothing
    // for an all-ready status — then stays absent until the open-file set changes,
    // so a dead server is invisible on this surface for as long as the user keeps
    // editing the same file.
    expect(lspPollDelay(status(server({ state: "ready" })), MIN_LSP_READS, true)).toBe(
      LSP_POLL_SLOW_MS,
    );
    expect(lspPollDelay(status(server({ state: "failed" })), MIN_LSP_READS, true)).toBe(
      LSP_POLL_SLOW_MS,
    );
  });

  it("keeps watching slowly while no server has appeared at all", () => {
    // A server row is created by the `didOpen` the editor sends after reading the
    // file off disk, so a slow read can push registration past the three fast
    // reads. Counting reads and then stopping leaves a starting server invisible.
    expect(lspPollDelay(status(), MIN_LSP_READS, true)).toBe(LSP_POLL_SLOW_MS);
  });

  it("stops entirely when no file is open", () => {
    // Nothing can start a server, so there is nothing to watch — and a poll for
    // the life of the app is a cost with no reader.
    expect(lspPollDelay(status(server({ state: "starting" })), 1, false)).toBeNull();
    expect(lspPollDelay(status(server({ state: "ready" })), 9, false)).toBeNull();
  });

  it("stops when the status call itself failed", () => {
    expect(lspPollDelay(null, 1, true)).toBeNull();
  });

  it("is slower when settled than when busy, so the idle cost is bounded", () => {
    expect(LSP_POLL_SLOW_MS).toBeGreaterThan(LSP_POLL_FAST_MS);
  });
});
