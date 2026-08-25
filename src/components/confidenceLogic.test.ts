import { describe, expect, it } from "vitest";
import { confidenceClass, confidenceForFile } from "./confidenceLogic";
import type {
  DiffLine,
  FileDiff,
  GroupFile,
  GroupKind,
  Hunk,
  IntentGroup,
  LineOrigin,
  SelfConfidence,
} from "../ipc/types";

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

/** A hunk holding changed lines with the given `DiffLine.index` values. */
function hunkWith(indices: number[]): Hunk {
  return {
    oldStart: 1,
    oldLines: 0,
    newStart: 1,
    newLines: indices.length,
    header: "",
    lines: indices.map((i) => line(i, "addition")),
  };
}

function diffOf(path: string, indices: number[]): FileDiff {
  return { path, oldPath: null, hunks: [hunkWith(indices)], isBinary: false };
}

function gfile(path: string, lineIndices: number[]): GroupFile {
  return { path, lineIndices, hunks: [0] };
}

function group(
  files: GroupFile[],
  selfConfidence: SelfConfidence | undefined,
  kind: GroupKind = "intent",
): IntentGroup {
  const g: IntentGroup = {
    id: `g-${files.map((f) => f.path).join(",")}-${selfConfidence ?? "none"}`,
    kind,
    label: kind,
    files,
    lineCount: files.reduce((n, f) => n + f.lineIndices.length, 0),
    confidence: "high",
  };
  if (selfConfidence) g.selfConfidence = selfConfidence;
  return g;
}

/** Order-insensitive comparison — the overlay does not care about entry order. */
function sorted(
  entries: { index: number; level: SelfConfidence }[],
): { index: number; level: SelfConfidence }[] {
  return [...entries].sort((a, b) => a.index - b.index);
}

describe("confidenceForFile", () => {
  it("maps a group's changed lines to its stated self-confidence", () => {
    const diff = diffOf("src/app.ts", [0, 1, 2]);
    const groups = [group([gfile("src/app.ts", [0, 1, 2])], "low")];
    expect(sorted(confidenceForFile("src/app.ts", diff, groups))).toEqual([
      { index: 0, level: "low" },
      { index: 1, level: "low" },
      { index: 2, level: "low" },
    ]);
  });

  it("abstains — emits nothing — for lines whose group has no self-confidence", () => {
    const diff = diffOf("src/app.ts", [0, 1]);
    const groups = [group([gfile("src/app.ts", [0, 1])], undefined)];
    expect(confidenceForFile("src/app.ts", diff, groups)).toEqual([]);
  });

  it("only emits for the requested path", () => {
    const diff = diffOf("src/app.ts", [0, 1]);
    const groups = [
      group([gfile("src/app.ts", [0])], "high"),
      group([gfile("src/other.ts", [1])], "low"),
    ];
    expect(confidenceForFile("src/app.ts", diff, groups)).toEqual([
      { index: 0, level: "high" },
    ]);
  });

  it("takes the LOWEST confidence when two groups claim the same line", () => {
    const diff = diffOf("src/app.ts", [0, 1]);
    const groups = [
      group([gfile("src/app.ts", [0, 1])], "high"),
      group([gfile("src/app.ts", [1])], "low"),
    ];
    expect(sorted(confidenceForFile("src/app.ts", diff, groups))).toEqual([
      { index: 0, level: "high" },
      { index: 1, level: "low" },
    ]);
  });

  it("takes medium over high, and low over medium", () => {
    const diff = diffOf("src/app.ts", [5]);
    const groups = [
      group([gfile("src/app.ts", [5])], "high"),
      group([gfile("src/app.ts", [5])], "medium"),
    ];
    expect(confidenceForFile("src/app.ts", diff, groups)).toEqual([
      { index: 5, level: "medium" },
    ]);
  });

  it("does not emit an index that is not part of this file's diff", () => {
    const diff = diffOf("src/app.ts", [0]);
    // The group claims line 9, but the diff only holds line 0.
    const groups = [group([gfile("src/app.ts", [0, 9])], "low")];
    expect(confidenceForFile("src/app.ts", diff, groups)).toEqual([
      { index: 0, level: "low" },
    ]);
  });

  it("returns an empty list when no group has a self-confidence", () => {
    const diff = diffOf("src/app.ts", [0, 1]);
    const groups = [
      group([gfile("src/app.ts", [0])], undefined),
      group([gfile("src/app.ts", [1])], undefined, "formatting"),
    ];
    expect(confidenceForFile("src/app.ts", diff, groups)).toEqual([]);
  });
});

describe("confidenceClass", () => {
  it("names the per-level decoration class", () => {
    expect(confidenceClass("low")).toBe("cb-line-confidence-low");
    expect(confidenceClass("medium")).toBe("cb-line-confidence-medium");
    expect(confidenceClass("high")).toBe("cb-line-confidence-high");
  });
});
