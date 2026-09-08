import { describe, expect, it } from "vitest";
import {
  RENAME_FIELD_WIDTH,
  acceptedNewName,
  bufferPaths,
  bufferVerdict,
  consumeRenameEdits,
  documentBoundsVerdict,
  identifierAt,
  placeRenameField,
  posAt,
  provisionalRenameWarning,
  refusalNote,
  renameOffer,
  renameReadiness,
  renameSummary,
  replacedTextMatches,
  shouldHandleRename,
  type RenameEditorState,
} from "./renameLogic";
import type { PrepareRenameResult, RenameResult } from "../ipc/types";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const READY: RenameEditorState = {
  lspEnabled: true,
  opened: true,
  changePending: false,
  docVersion: 7,
  syncedVersion: 7,
  syncError: null,
};

function prepare(over: Partial<PrepareRenameResult> = {}): PrepareRenameResult {
  return {
    outcome: "ready",
    renameable: true,
    startLine: 31,
    startCharacter: 22,
    endLine: 31,
    endCharacter: 28,
    placeholder: null,
    message: null,
    server: "roslyn",
    ...over,
  };
}

function result(over: Partial<RenameResult> = {}): RenameResult {
  return {
    outcome: "ready",
    total: 2,
    written: [],
    buffers: [],
    failures: [],
    message: null,
    server: "roslyn",
    ...over,
  };
}

// ---------------------------------------------------------------------------
// Which editor answers, and whether it may ask
// ---------------------------------------------------------------------------

describe("shouldHandleRename", () => {
  it("acts only for the editor that is actually on screen", () => {
    expect(shouldHandleRename({ rendered: true, focusElsewhere: false })).toBe(true);
    expect(shouldHandleRename({ rendered: false, focusElsewhere: false })).toBe(false);
  });

  it("declines while the caret is in another text-entry surface", () => {
    // F2 is `allowInText`, so it fires with focus in a terminal, the Notes
    // textarea or the search box — none of which unmount the editor behind
    // them. Acting anyway opened an autofocusing rename field over a file the
    // user was not looking at, at a caret left over from whenever that editor
    // was last clicked. Visibility is still the test for *which* editor; this
    // only says nobody is typing somewhere else.
    expect(shouldHandleRename({ rendered: true, focusElsewhere: true })).toBe(false);
  });
});

describe("renameReadiness", () => {
  it("is ready when the server has this exact text", () => {
    expect(renameReadiness(READY)).toEqual({ kind: "ready" });
  });

  it("asks for a flush while the debounce is still armed", () => {
    // The whole ordering guarantee: the ranges coming back are applied to this
    // text, so they must have been computed from this text.
    expect(renameReadiness({ ...READY, changePending: true })).toEqual({ kind: "flush" });
  });

  it("asks for a flush when the versions disagree even with no timer armed", () => {
    // The timer has fired and the round trip has not resolved — the window a
    // `changeTimer`-only check misses entirely.
    expect(renameReadiness({ ...READY, docVersion: 8 })).toEqual({ kind: "flush" });
  });

  it("retries a previous sync failure rather than reporting it", () => {
    // A stale `syncError` must not strand F2 for the life of the tab: flushing
    // again either fixes it or produces a fresh, current reason.
    expect(renameReadiness({ ...READY, syncError: "pipe closed" })).toEqual({ kind: "flush" });
  });

  it("refuses outright for a tab with no language server, rather than waiting", () => {
    const answer = renameReadiness({ ...READY, lspEnabled: false });
    expect(answer.kind).toBe("refuse");
    // Nothing to flush to, so a "flush" answer would arm a timer that never
    // resolves — the same trap `flushChange`'s own early return exists for.
    if (answer.kind === "refuse") expect(answer.reason.length).toBeGreaterThan(0);
  });

  it("refuses while the document has not been opened yet", () => {
    const answer = renameReadiness({ ...READY, opened: false });
    expect(answer.kind).toBe("refuse");
  });

  it("prefers the refusals over the flush when both apply", () => {
    // A secrets tab is dirty like any other; ordering the branches the other way
    // round would ask it to flush to a server it has none of.
    expect(
      renameReadiness({ ...READY, lspEnabled: false, changePending: true, docVersion: 9 }).kind,
    ).toBe("refuse");
  });
});

// ---------------------------------------------------------------------------
// The prepare answer
// ---------------------------------------------------------------------------

describe("renameOffer", () => {
  it("offers the span the server named", () => {
    const offer = renameOffer(prepare());
    expect(offer).toEqual({
      kind: "offer",
      start: { line: 31, character: 22 },
      end: { line: 31, character: 28 },
      placeholder: null,
      caveat: null,
    });
  });

  it("still offers when the server named no span at all", () => {
    // `{"defaultBehavior": true}` — renameable, span unstated. Treating this as
    // a refusal would break every server that answers that way.
    const offer = renameOffer(
      prepare({ startLine: null, startCharacter: null, endLine: null, endCharacter: null }),
    );
    expect(offer.kind).toBe("offer");
    if (offer.kind === "offer") {
      expect(offer.start).toBe(null);
      expect(offer.end).toBe(null);
    }
  });

  it("drops a half-specified span rather than inventing the missing half", () => {
    const offer = renameOffer(prepare({ endCharacter: null }));
    expect(offer.kind).toBe("offer");
    if (offer.kind === "offer") expect(offer.end).toBe(null);
  });

  it("turns renameable:false into a refusal and never an empty field", () => {
    const offer = renameOffer(prepare({ renameable: false }));
    expect(offer.kind).toBe("refuse");
    if (offer.kind === "refuse") {
      // "there are none" and "you cannot do this here" are opposite claims and a
      // box that opens and renames nothing is the worse of the two.
      expect(offer.reason).toMatch(/rename/i);
    }
  });

  it("prefers the server's own words for a refusal when it sent any", () => {
    const offer = renameOffer(prepare({ renameable: false, message: "keywords cannot be renamed" }));
    expect(offer.kind).toBe("refuse");
    if (offer.kind === "refuse") expect(offer.reason).toBe("keywords cannot be renamed");
  });

  it("refuses every outcome that is not an answer, and says which it was", () => {
    // `unsupported` is deliberately NOT in this list any more — see the test
    // below. Every other non-answer means nobody could be asked at all.
    for (const outcome of ["starting", "loading", "notConfigured", "failed"] as const) {
      const offer = renameOffer(prepare({ outcome, renameable: false }));
      expect(offer.kind).toBe("refuse");
    }
    const loading = renameOffer(prepare({ outcome: "loading", renameable: false }));
    if (loading.kind === "refuse") expect(loading.reason).toMatch(/loading/i);
  });

  it("offers anyway when the server cannot PREPARE, because it can still rename", () => {
    // Changed deliberately from a refusal, and the reason is written here so
    // nobody restores it: `prepareRename` and `rename` are two capabilities.
    // A server declaring the bare `"renameProvider": true` provides rename and
    // no prepare, so `Client::prepare_rename` refuses on the capability alone
    // and answers `unsupported` — which used to disable F2 entirely for a
    // server whose rename would have worked. `protocol.rs` keeps the two facts
    // in separate fields expressly so a caller cannot collapse them, and the
    // session test `a_server_that_renames_but_cannot_prepare_is_unsupported_only_for_the_prepare`
    // pins the split all the way to this boundary. The prefill never came from
    // the prepare answer anyway (`identifierAt` reads the buffer), so nothing
    // is missing here: the span is simply unknown.
    const offer = renameOffer(prepare({ outcome: "unsupported", renameable: false, message: null }));
    expect(offer.kind).toBe("offer");
    if (offer.kind === "offer") {
      expect(offer.start).toBe(null);
      expect(offer.end).toBe(null);
      expect(offer.caveat).toBe(null);
    }
  });

  it("carries a ready answer's caveat through instead of swallowing it", () => {
    const offer = renameOffer(prepare({ message: "the server never finished loading" }));
    expect(offer.kind).toBe("offer");
    if (offer.kind === "offer") expect(offer.caveat).toBe("the server never finished loading");
  });
});

// ---------------------------------------------------------------------------
// Reading the identifier out of the buffer
// ---------------------------------------------------------------------------

describe("identifierAt", () => {
  it("finds the identifier the caret is inside", () => {
    expect(identifierAt("var walker = new Walker();", 18)).toEqual({
      text: "Walker",
      start: 17,
      end: 23,
    });
  });

  it("finds the identifier the caret is at the start of", () => {
    expect(identifierAt("var walker = new Walker();", 17)?.text).toBe("Walker");
  });

  it("finds the identifier the caret sits immediately after", () => {
    // Pressing F2 with the caret at the end of a word is how people actually do
    // this, and the Rust side's `enclosing_identifier` takes the same view.
    expect(identifierAt("var walker = new Walker();", 23)?.text).toBe("Walker");
  });

  it("abstains rather than guessing at a nearby word", () => {
    // Column 3 is the space after `var`... which is the end boundary of `var`,
    // so pick a position with a word on neither side.
    expect(identifierAt("a  b", 2)).toBe(null);
    expect(identifierAt("   ", 1)).toBe(null);
    expect(identifierAt("", 0)).toBe(null);
  });

  it("counts _ and $ as part of a name", () => {
    expect(identifierAt("const _private$ = 1;", 8)?.text).toBe("_private$");
  });

  it("does not truncate a non-ASCII identifier", () => {
    // A narrow [A-Za-z0-9_] class would prefill the field with "caf" and the
    // backend's token check would then reject every edit.
    expect(identifierAt("var café = 1;", 5)?.text).toBe("café");
  });

  it("stops at a dot rather than swallowing a member access", () => {
    expect(identifierAt("walker.Scan()", 8)?.text).toBe("Scan");
    expect(identifierAt("walker.Scan()", 2)?.text).toBe("walker");
  });

  it("clamps a column past the end of the line instead of throwing", () => {
    expect(identifierAt("abc", 99)?.text).toBe("abc");
    expect(identifierAt("abc ", 99)).toBe(null);
  });

  it("abstains on a column that is not a number", () => {
    expect(identifierAt("abc", Number.NaN)).toBe(null);
  });
});

describe("posAt", () => {
  it("adds the column to the line's start", () => {
    expect(posAt(100, 20, 5)).toBe(105);
  });

  it("clamps a column past the line's end to the end of that line", () => {
    // A server describing a document it believes is longer must not produce an
    // offset CodeMirror throws on — that throw reads as the app breaking.
    expect(posAt(100, 20, 99)).toBe(120);
  });

  it("clamps a negative or nonsense column to the line's start", () => {
    expect(posAt(100, 20, -5)).toBe(100);
    expect(posAt(100, 20, Number.NaN)).toBe(100);
  });
});

// ---------------------------------------------------------------------------
// The name the user typed
// ---------------------------------------------------------------------------

describe("acceptedNewName", () => {
  it("accepts an ordinary new name", () => {
    expect(acceptedNewName("HeapWalker", "Walker")).toEqual({ kind: "ok", name: "HeapWalker" });
  });

  it("trims surrounding whitespace rather than refusing it", () => {
    expect(acceptedNewName("  HeapWalker \n", "Walker")).toEqual({
      kind: "ok",
      name: "HeapWalker",
    });
  });

  it("rejects nothing at all", () => {
    expect(acceptedNewName("", "Walker").kind).toBe("reject");
    expect(acceptedNewName("   ", "Walker").kind).toBe("reject");
  });

  it("rejects a name with whitespace inside it", () => {
    expect(acceptedNewName("Heap Walker", "Walker").kind).toBe("reject");
    expect(acceptedNewName("Heap\tWalker", "Walker").kind).toBe("reject");
    expect(acceptedNewName("Heap\nWalker", "Walker").kind).toBe("reject");
  });

  it("rejects the old name, because a no-op would read as a rename that worked", () => {
    const answer = acceptedNewName("Walker", "Walker");
    expect(answer.kind).toBe("reject");
    if (answer.kind === "reject") expect(answer.reason).toMatch(/already/i);
  });

  it("abstains on every language rule, and that is the point", () => {
    // Each of these is a legal identifier somewhere this app opens files, and a
    // validator written here would be a second, worse opinion than the server's.
    for (const name of ["@class", "@event", "café", "Ω", "$scope", "#private", "r#type", "_", "x2"]) {
      expect(acceptedNewName(name, "Walker")).toEqual({ kind: "ok", name });
    }
  });

  it("does not reject a name that merely looks wrong", () => {
    // Leading digit, punctuation, a keyword: all refused by the *server*, with
    // its own words, which is where that judgement belongs.
    expect(acceptedNewName("1abc", "Walker").kind).toBe("ok");
    expect(acceptedNewName("class", "Walker").kind).toBe("ok");
    expect(acceptedNewName("a-b", "Walker").kind).toBe("ok");
  });
});

// ---------------------------------------------------------------------------
// Reasons and caveats
// ---------------------------------------------------------------------------

describe("refusalNote", () => {
  it("uses availabilityPhrase's words rather than a second set", () => {
    expect(refusalNote({ outcome: "loading", message: null })).toMatch(/loading/i);
    expect(refusalNote({ outcome: "notConfigured", message: null })).toMatch(/no language server/i);
  });

  it("says nothing was renamed, so the sentence is about this action", () => {
    expect(refusalNote({ outcome: "failed", message: null })).toMatch(/nothing was renamed/i);
  });

  it("appends the server's own message when there is one", () => {
    expect(refusalNote({ outcome: "failed", message: "roslyn exited with code 1" })).toContain(
      "roslyn exited with code 1",
    );
  });

  it("never claims there was nothing to rename when the server cannot answer", () => {
    // "This server cannot answer" and "there are none" are opposite claims.
    const note = refusalNote({ outcome: "unsupported", message: null });
    expect(note).not.toMatch(/\bnothing to rename\b/i);
  });
});

describe("provisionalRenameWarning", () => {
  it("warns about a ready answer that carried a message", () => {
    const warning = provisionalRenameWarning({
      outcome: "ready",
      message: "this answer came from a server that never finished loading",
    });
    expect(warning).not.toBe(null);
    // The consequence, not just the caveat: a missed call site does not compile.
    expect(warning).toMatch(/missed/i);
  });

  it("is silent for an ordinary ready answer", () => {
    expect(provisionalRenameWarning({ outcome: "ready", message: null })).toBe(null);
  });

  it("is silent for an outcome that is not an answer at all", () => {
    // Those go through `refusalNote`; a "may have missed call sites" sentence
    // about a rename that never ran would invent a partial edit.
    expect(provisionalRenameWarning({ outcome: "failed", message: "died" })).toBe(null);
  });

  it("serves the prepare answer and the rename answer identically", () => {
    // One function on purpose: the caveat is knowable before Enter (from
    // prepareRename, when a confirmation is worth something) and only after the
    // writes (from rename), and the two must not describe it two ways.
    const same = "the server never finished loading";
    expect(provisionalRenameWarning(prepare({ message: same }))).toBe(
      provisionalRenameWarning(result({ message: same })),
    );
  });
});

// ---------------------------------------------------------------------------
// The stale-mirror check over an open buffer
// ---------------------------------------------------------------------------

describe("replacedTextMatches", () => {
  it("accepts a zero-width insertion aimed inside the old identifier", () => {
    // MEASURED: Roslyn renames `Walker` to `HeapWalker` by *inserting* "Heap",
    // so the replaced text is the empty string. A "replaced text contains the
    // old name" rule refuses every correct Roslyn rename; the enclosing token
    // does not.
    expect(replacedTextMatches("        return new Walker(heap);", 23, "Walker")).toBe(true);
  });

  it("accepts a whole-identifier replacement too", () => {
    expect(replacedTextMatches("        return new Walker(heap);", 19, "Walker")).toBe(true);
  });

  it("rejects an edit aimed at a different token", () => {
    expect(replacedTextMatches("        return new Walker(heap);", 26, "Walker")).toBe(false);
  });

  it("rejects an edit aimed at no token at all", () => {
    expect(replacedTextMatches("   ", 1, "Walker")).toBe(false);
  });

  it("abstains when there is no name to check against", () => {
    // Comparing against "" — which no token equals — would refuse every rename.
    expect(replacedTextMatches("   ", 1, "")).toBe(true);
  });
});

describe("documentBoundsVerdict", () => {
  const edit = (startLine: number, endLine: number) => ({ startLine, endLine });

  it("applies when every edit names a line this buffer has", () => {
    expect(documentBoundsVerdict(250, [edit(1, 1), edit(250, 250)], "src/B.cs")).toEqual({
      kind: "apply",
      note: null,
    });
  });

  it("refuses the file when an edit names a line past its end", () => {
    // The stale-mirror bug in its most damaging form, and the half that only
    // the frontend can catch. `edits::apply` refuses exactly this condition for
    // a closed file (`EditError::OutOfDocument`); an open buffer was instead
    // handed to `lineToPos`, which CLAMPS — so a server computing from a mirror
    // in which this file still had 400 lines had two of its insertions dropped
    // into arbitrary columns of line 250. `bufferVerdict` cannot see it: a
    // clamped edit that misses the token is indistinguishable from a legitimate
    // rename-in-comment, so with any edit matching it returns `apply`.
    const verdict = documentBoundsVerdict(250, [edit(1, 1), edit(380, 380)], "src/B.cs");
    expect(verdict.kind).toBe("refuse");
    if (verdict.kind === "refuse") {
      expect(verdict.reason).toContain("src/B.cs");
      expect(verdict.reason).toContain("380");
      expect(verdict.reason).toContain("250");
    }
  });

  it("refuses a line before the first one, and a line that is not a number", () => {
    expect(documentBoundsVerdict(10, [edit(0, 1)], "a.ts").kind).toBe("refuse");
    expect(documentBoundsVerdict(10, [edit(1, Number.NaN)], "a.ts").kind).toBe("refuse");
  });

  it("applies an empty edit list", () => {
    expect(documentBoundsVerdict(10, [], "a.ts")).toEqual({ kind: "apply", note: null });
  });
});

describe("consumeRenameEdits", () => {
  it("drops the path an editor has taken, so a later remount cannot re-apply it", () => {
    // `FileEditor`'s guard is a per-mount ref starting at 0, so closing a tab
    // and reopening it re-consumed a finished rename — against a buffer that
    // already holds the new name, and before its CodeMirror view exists, which
    // raised a never-dismissed error saying the file "still holds the old
    // name". Both claims false. The map has to forget.
    const state = { token: 3, byPath: { "a.ts": 1, "b.ts": 2 } };
    const next = consumeRenameEdits(state, "a.ts");
    expect(next.byPath).toEqual({ "b.ts": 2 });
    expect(next.token).toBe(3);
  });

  it("returns the same state when there is nothing to consume", () => {
    // Same reference, so consuming twice costs no render.
    const state = { token: 3, byPath: { "a.ts": 1 } };
    expect(consumeRenameEdits(state, "b.ts")).toBe(state);
  });
});

describe("bufferVerdict", () => {
  it("applies when every edit lands on the name", () => {
    expect(bufferVerdict([true, true, true], "src/a.ts")).toEqual({ kind: "apply", note: null });
  });

  it("applies with a note when only some do", () => {
    // Roslyn's rename-in-comments and TypeScript's shorthand-property expansion
    // (`{ foo }` -> `{ newFoo: foo }`) both legitimately touch text that is not
    // the bare identifier. Refusing the file over that would refuse correct
    // renames.
    const verdict = bufferVerdict([true, false, true], "src/a.ts");
    expect(verdict.kind).toBe("apply");
    if (verdict.kind === "apply") {
      expect(verdict.note).not.toBe(null);
      expect(verdict.note).toContain("src/a.ts");
    }
  });

  it("refuses only when no edit in the file lands on the name", () => {
    const verdict = bufferVerdict([false, false], "src/a.ts");
    expect(verdict.kind).toBe("refuse");
    if (verdict.kind === "refuse") expect(verdict.reason).toContain("src/a.ts");
  });

  it("applies an empty edit list silently", () => {
    // The server named the file and asked for no changes. Legal, and not a
    // refusal.
    expect(bufferVerdict([], "src/a.ts")).toEqual({ kind: "apply", note: null });
  });
});

// ---------------------------------------------------------------------------
// Reporting what happened
// ---------------------------------------------------------------------------

describe("renameSummary", () => {
  it("reports a refusal with the reason and never as a success", () => {
    const report = renameSummary("Walker", "HeapWalker", result({ outcome: "failed", total: null, message: "died" }), []);
    expect(report.kind).toBe("error");
    expect(report.title).toContain("was not renamed");
    expect(report.detail).toContain("died");
  });

  it("names the file left holding part of a rename, on the outcome that carries it", () => {
    // `failures` is populated ONLY on a non-`ready` outcome — `write_plan`'s
    // failure return is the one place that fills it — so the early return here
    // used to discard every path in it and the loops below were dead. In the
    // unrecoverable case the backend's own message carries a COUNT and no path,
    // so `failures[i].path` was the only place the answer existed.
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({
        outcome: "failed",
        total: null,
        message: "the rename could not be completed, and 1 file(s) could not be restored afterwards.",
        failures: [
          { path: "src/B.cs", detail: "locked", unrecoverable: false },
          { path: "src/A.cs", detail: "restore failed", unrecoverable: true },
        ],
      }),
      [],
    );
    expect(report.kind).toBe("error");
    expect(report.detail).toContain("src/A.cs");
    expect(report.detail).toMatch(/git/i);
    // And it must not claim the file was left alone: it holds half a rename.
    expect(report.title).not.toMatch(/was not renamed/i);
  });

  it("still says nothing was renamed when every file really was restored", () => {
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({
        outcome: "failed",
        total: null,
        message: "b.cs could not be written. Every file this app had already changed was restored.",
        failures: [{ path: "src/B.cs", detail: "locked", unrecoverable: false }],
      }),
      [],
    );
    expect(report.title).toContain("was not renamed");
    expect(report.detail).toContain("restored");
  });

  it("says plainly that files written to disk are not undoable, and where to go", () => {
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({ total: 14, written: [{ path: "src/A.cs", edits: 9 }, { path: "src/B.cs", edits: 5 }] }),
      [],
    );
    expect(report.detail).toMatch(/cannot be undone/i);
    // One click from the line-level revert this app is actually good at.
    expect(report.detail).toMatch(/Changes tab/i);
  });

  it("counts the buffers the editors actually applied, not the ones asked for", () => {
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({ total: 3, buffers: [{ path: "src/A.cs", edits: [] }] }),
      ["src/A.cs"],
    );
    expect(report.detail).toContain("1 file");
    expect(report.kind).toBe("info");
  });

  it("reports a buffer no editor received rather than dropping it", () => {
    // The one window this design cannot close, one promise resolution wide.
    // Dropping it leaves a file holding the old name with nothing saying so.
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({
        total: 3,
        buffers: [
          { path: "src/A.cs", edits: [] },
          {
            path: "src/B.cs",
            edits: [
              { startLine: 1, startCharacter: 0, endLine: 1, endCharacter: 0, newText: "Heap" },
            ],
          },
        ],
      }),
      ["src/A.cs"],
    );
    expect(report.kind).toBe("error");
    expect(report.detail).toContain("src/B.cs");
    expect(report.detail).toMatch(/still holds the old name/i);
  });

  it("escalates an unrecoverable failure and does not soften it", () => {
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({
        total: 4,
        written: [{ path: "src/A.cs", edits: 4 }],
        failures: [{ path: "src/B.cs", detail: "restore failed", unrecoverable: true }],
      }),
      [],
    );
    expect(report.kind).toBe("error");
    expect(report.detail).toMatch(/git/i);
    expect(report.detail).toContain("src/B.cs");
  });

  it("lists a recoverable failure without sending the user to git", () => {
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({ total: 1, failures: [{ path: "src/B.cs", detail: "read-only", unrecoverable: false }] }),
      [],
    );
    expect(report.kind).toBe("error");
    expect(report.detail).toContain("read-only");
    expect(report.detail).not.toMatch(/could not be restored/i);
  });

  it("says nothing was found rather than reporting a successful empty rename", () => {
    const report = renameSummary("Walker", "HeapWalker", result({ total: 0 }), []);
    expect(report.kind).toBe("info");
    expect(report.title).toMatch(/nothing to rename/i);
  });

  it("warns rather than merely informing when a ready answer was qualified", () => {
    const report = renameSummary(
      "Walker",
      "HeapWalker",
      result({ total: 2, written: [{ path: "src/A.cs", edits: 2 }], message: "server never primed" }),
      [],
    );
    expect(report.kind).toBe("warning");
    expect(report.detail).toMatch(/missed/i);
  });
});

describe("bufferPaths", () => {
  it("is the list renameSummary compares against", () => {
    expect(
      bufferPaths([
        { path: "src/A.cs", edits: [] },
        { path: "src/B.cs", edits: [] },
      ]),
    ).toEqual(["src/A.cs", "src/B.cs"]);
  });
});

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

describe("placeRenameField", () => {
  const bounds = { left: 100, top: 50, width: 800, height: 400 };

  it("places the field relative to the frame, just below the caret", () => {
    const place = placeRenameField(300, 150, bounds);
    expect(place.left).toBe(200);
    expect(place.top).toBe(104);
  });

  it("keeps the field inside the pane on the right", () => {
    const place = placeRenameField(880, 150, bounds);
    expect(place.left).toBe(800 - RENAME_FIELD_WIDTH - 4);
  });

  it("has a floor, so a pane narrower than the field does not go off the left", () => {
    const place = placeRenameField(10, 20, { left: 0, top: 0, width: 100, height: 40 });
    expect(place.left).toBeGreaterThanOrEqual(4);
    expect(place.top).toBeGreaterThanOrEqual(4);
  });

  it("answers something usable before the frame has a rectangle", () => {
    expect(placeRenameField(300, 150, null)).toEqual({ left: 4, top: 4, maxHeight: 0 });
  });
});
