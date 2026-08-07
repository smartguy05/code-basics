import { describe, expect, it } from "vitest";
import { allChangedIndices, hunkIndices } from "./diffLogic";
import type { DiffLine, FileDiff, Hunk, LineOrigin } from "../ipc/types";

function line(index: number, origin: LineOrigin): DiffLine {
  return {
    index,
    origin,
    content: `line ${index}`,
    oldLineno: origin === "addition" ? null : index,
    newLineno: origin === "deletion" ? null : index,
    noNewline: false,
  };
}

function hunk(lines: DiffLine[]): Hunk {
  return {
    oldStart: 1,
    oldLines: lines.filter((l) => l.origin !== "addition").length,
    newStart: 1,
    newLines: lines.filter((l) => l.origin !== "deletion").length,
    header: "",
    lines,
  };
}

function diffOf(hunks: Hunk[]): FileDiff {
  return { path: "a.ts", oldPath: null, hunks, isBinary: false };
}

/** Two hunks whose indices continue across the hunk boundary. */
const multiHunk = diffOf([
  hunk([line(0, "context"), line(1, "addition"), line(2, "deletion")]),
  hunk([line(3, "context"), line(4, "deletion"), line(5, "addition"), line(6, "context")]),
]);

describe("allChangedIndices", () => {
  it("collects additions and deletions across every hunk, in order", () => {
    expect(allChangedIndices(multiHunk)).toEqual([1, 2, 4, 5]);
  });

  it("drops context lines", () => {
    const contextOnly = diffOf([hunk([line(0, "context"), line(1, "context")])]);
    expect(allChangedIndices(contextOnly)).toEqual([]);
  });

  it("returns an empty list for a diff with no hunks", () => {
    expect(allChangedIndices(diffOf([]))).toEqual([]);
  });

  it("preserves the diff's own index numbering rather than positions", () => {
    const sparse = diffOf([hunk([line(17, "addition"), line(42, "deletion")])]);
    expect(allChangedIndices(sparse)).toEqual([17, 42]);
  });
});

describe("hunkIndices", () => {
  it("returns only the requested hunk's changed lines", () => {
    expect(hunkIndices(multiHunk, 0)).toEqual([1, 2]);
    expect(hunkIndices(multiHunk, 1)).toEqual([4, 5]);
  });

  it("partitions allChangedIndices exactly", () => {
    const combined = multiHunk.hunks.flatMap((_, i) => hunkIndices(multiHunk, i));
    expect(combined).toEqual(allChangedIndices(multiHunk));
  });

  it("returns an empty list for an out-of-range hunk", () => {
    expect(hunkIndices(multiHunk, 2)).toEqual([]);
    expect(hunkIndices(multiHunk, -1)).toEqual([]);
  });
});
