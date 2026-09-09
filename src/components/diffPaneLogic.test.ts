import { describe, expect, it } from "vitest";
import {
  COLLAPSE_KEY,
  DIFF_LAYOUT_KEY,
  WHITESPACE_KEY,
  diffToolbarState,
  loadCollapse,
  loadDiffLayout,
  loadIgnoreWhitespace,
  revertAllLabel,
  riskIndices,
  shownDiff,
  viewBaseline,
} from "./diffPaneLogic";
import type {
  DiffLine,
  ErosionFlag,
  FileContents,
  FileDiff,
  Hunk,
  IntentGroup,
} from "../ipc/types";

function storage(entries: Record<string, string>): Pick<Storage, "getItem"> {
  return { getItem: (key: string) => entries[key] ?? null };
}

function line(over: Partial<DiffLine> & { index: number }): DiffLine {
  return {
    origin: "addition",
    content: "x",
    oldLineno: null,
    newLineno: over.index + 1,
    noNewline: false,
    ...over,
  };
}

function hunk(newStart: number, lines: DiffLine[]): Hunk {
  return {
    oldStart: newStart,
    oldLines: lines.length,
    newStart,
    newLines: lines.length,
    header: `@@ -${newStart} +${newStart} @@`,
    lines,
  };
}

function diffOf(hunks: Hunk[]): FileDiff {
  return { path: "a.ts", oldPath: null, hunks, isBinary: false };
}

/** Two hunks, one changed line each, at indices 0 and 1. */
const TWO_HUNKS = diffOf([
  hunk(1, [line({ index: 0 }), line({ index: 1, origin: "context" })]),
  hunk(5, [line({ index: 2 })]),
]);

function flag(over: Partial<ErosionFlag>): ErosionFlag {
  return {
    path: "a.ts",
    line: 1,
    index: 0,
    origin: "addition",
    category: "deletedAssertion",
    ruleId: "r",
    message: "m",
    content: "x",
    ...over,
  };
}

function group(over: Partial<IntentGroup>): IntentGroup {
  return {
    id: "g1",
    kind: "intent",
    label: "why",
    files: [],
    lineCount: 1,
    confidence: "high",
    ...over,
  };
}

describe("the preference loaders", () => {
  it("reads the stored diff layout, and defaults to side by side", () => {
    expect(loadDiffLayout(storage({ [DIFF_LAYOUT_KEY]: "inline" }))).toBe("inline");
    expect(loadDiffLayout(storage({}))).toBe("sideBySide");
    // Anything unrecognised is not a layout; abstain to the default rather than
    // passing an unknown value through to the editor.
    expect(loadDiffLayout(storage({ [DIFF_LAYOUT_KEY]: "sideways" }))).toBe("sideBySide");
  });

  it("keeps folding and whitespace-ignoring off unless explicitly turned on", () => {
    expect(loadCollapse(storage({}))).toBe(false);
    expect(loadCollapse(storage({ [COLLAPSE_KEY]: "false" }))).toBe(false);
    expect(loadCollapse(storage({ [COLLAPSE_KEY]: "true" }))).toBe(true);

    expect(loadIgnoreWhitespace(storage({}))).toBe(false);
    expect(loadIgnoreWhitespace(storage({ [WHITESPACE_KEY]: "1" }))).toBe(false);
    expect(loadIgnoreWhitespace(storage({ [WHITESPACE_KEY]: "true" }))).toBe(true);
  });
});

describe("shownDiff", () => {
  it("shows the whole diff when nothing scopes it", () => {
    expect(shownDiff(TWO_HUNKS, null)).toBe(TWO_HUNKS);
    expect(shownDiff(null, null)).toBeNull();
    expect(shownDiff(null, [0])).toBeNull();
  });

  it("shows only the scoped hunks", () => {
    expect(shownDiff(TWO_HUNKS, [1])?.hunks).toEqual([TWO_HUNKS.hunks[1]]);
  });

  it("treats an empty scope as showing nothing, not as showing everything", () => {
    expect(shownDiff(TWO_HUNKS, [])?.hunks).toEqual([]);
  });

  it("ignores a hunk index the diff does not have", () => {
    expect(shownDiff(TWO_HUNKS, [0, 99])?.hunks).toEqual([TWO_HUNKS.hunks[0]]);
  });
});

describe("viewBaseline", () => {
  const contents: FileContents = { baseline: "old\n", working: "new\n" };

  it("falls back to the file's own baseline for each way the scope can be absent", () => {
    expect(viewBaseline(contents, TWO_HUNKS, null)).toBe("old\n");
    expect(viewBaseline(contents, null, [0])).toBe("old\n");
    expect(viewBaseline({ baseline: "old\n", working: null }, TWO_HUNKS, [0])).toBe("old\n");
    expect(viewBaseline({ baseline: null, working: "new\n" }, TWO_HUNKS, [0])).toBeNull();
    expect(viewBaseline(null, TWO_HUNKS, [0])).toBeNull();
  });

  it("rebuilds the baseline from the working copy when a card scopes the file", () => {
    // One working line ("is") replacing one baseline line ("was"), with an
    // unrelated "tail" line after it that the focused baseline must not touch.
    const one = diffOf([
      {
        ...hunk(1, [
          { ...line({ index: 0 }), origin: "deletion", content: "was" },
          { ...line({ index: 1 }), origin: "addition", content: "is" },
        ]),
        newLines: 1,
      },
    ]);
    const focused = viewBaseline({ baseline: "unrelated", working: "is\ntail\n" }, one, [0]);
    // Only the card's hunk differs from the working copy; the rest is identical
    // on both sides, which is the whole point of the focused baseline.
    expect(focused).toContain("was");
    expect(focused).toContain("tail");
    expect(focused).not.toBe("unrelated");
  });
});

describe("riskIndices", () => {
  it("abstains with no file or no diff", () => {
    expect(riskIndices(null, TWO_HUNKS, [], [])).toEqual([]);
    expect(riskIndices("a.ts", null, [], [])).toEqual([]);
  });

  it("fans a hunk's level out to its changed lines only, never its context", () => {
    const risky = diffOf([
      hunk(1, [line({ index: 0 }), line({ index: 1, origin: "context" }), line({ index: 2 })]),
    ]);
    expect(riskIndices("a.ts", risky, [flag({ index: 0, category: "secret" })], [])).toEqual([
      { index: 0, level: "high" },
      { index: 2, level: "high" },
    ]);
  });

  it("ignores a flag belonging to another file", () => {
    expect(
      riskIndices("a.ts", TWO_HUNKS, [flag({ path: "b.ts", index: 0, category: "secret" })], []),
    ).toEqual([]);
  });

  /**
   * The invariant the carve could break silently: hunk indices are read against
   * the FULL diff, so a group naming hunk 1 must still light hunk 1's lines.
   * Scoping the diff first would renumber it and paint the wrong lines.
   */
  it("indexes hunks against the full diff, so a card's hunk numbers still line up", () => {
    const groups = [
      group({ kind: "formatting", files: [{ path: "a.ts", lineIndices: [2], hunks: [1] }] }),
    ];
    expect(riskIndices("a.ts", TWO_HUNKS, [], groups)).toEqual([{ index: 2, level: "formatting" }]);
  });
});

describe("revertAllLabel", () => {
  it("says what the revert acts on", () => {
    expect(revertAllLabel([0])).toBe("shown");
    expect(revertAllLabel([])).toBe("shown");
    expect(revertAllLabel(null)).toBe("file");
  });
});

describe("diffToolbarState", () => {
  const base = {
    shownDiff: TWO_HUNKS,
    scopedHunks: null,
    selectedLines: [] as number[],
    path: "a.ts" as string | null,
    busy: false,
  };

  it("counts differences by hunk and reads the selection", () => {
    expect(diffToolbarState(base).differences).toBe(2);
    expect(diffToolbarState({ ...base, shownDiff: null }).differences).toBe(0);
    expect(diffToolbarState(base).hasSelection).toBe(false);
    expect(diffToolbarState({ ...base, selectedLines: [1] }).hasSelection).toBe(true);
  });

  it("offers staging only for an open file", () => {
    expect(diffToolbarState(base).canStage).toBe(true);
    expect(diffToolbarState({ ...base, path: null }).canStage).toBe(false);
  });

  it("withholds every mutating action while an action is in flight", () => {
    const busy = diffToolbarState({ ...base, selectedLines: [1], busy: true });
    expect(busy.canStage).toBe(false);
    expect(busy.canRevertAll).toBe(false);
    expect(busy.canRevertSelected).toBe(false);
    // Stepping through the diff changes nothing, so it stays available.
    expect(busy.canStep).toBe(true);
  });

  it("keeps Restore offerable while busy, unlike the Revert button", () => {
    const busy = diffToolbarState({ ...base, busy: true });
    expect(busy.hasRevertableChanges).toBe(true);
    expect(busy.canRevertAll).toBe(false);
  });

  it("does not offer a whole-file revert for a diff with no changed lines", () => {
    const contextOnly = diffOf([hunk(1, [line({ index: 0, origin: "context" })])]);
    const state = diffToolbarState({ ...base, shownDiff: contextOnly });
    expect(state.differences).toBe(1);
    expect(state.hasRevertableChanges).toBe(false);
    expect(state.canRevertAll).toBe(false);
  });

  it("cannot step through a diff with no hunks", () => {
    expect(diffToolbarState({ ...base, shownDiff: diffOf([]) }).canStep).toBe(false);
  });

  it("carries the revert label through", () => {
    expect(diffToolbarState({ ...base, scopedHunks: [0] }).revertAllLabel).toBe("shown");
    expect(diffToolbarState(base).revertAllLabel).toBe("file");
  });
});
