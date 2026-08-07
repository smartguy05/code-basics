import { describe, expect, it } from "vitest";
import { formatTime, unifiedText } from "./historyLogic";
import type { DiffLine, FileDiff, Hunk, LineOrigin } from "../ipc/types";

function line(index: number, origin: LineOrigin, content: string): DiffLine {
  return { index, origin, content, oldLineno: null, newLineno: null, noNewline: false };
}

function hunk(overrides: Partial<Hunk>, lines: DiffLine[]): Hunk {
  return {
    oldStart: 1,
    oldLines: 2,
    newStart: 1,
    newLines: 3,
    header: "",
    lines,
    ...overrides,
  };
}

function diffOf(hunks: Hunk[]): FileDiff {
  return { path: "a.ts", oldPath: null, hunks, isBinary: false };
}

describe("formatTime", () => {
  it("interprets the value as Unix seconds, not milliseconds", () => {
    expect(formatTime(1_700_000_000)).toBe(
      new Date(1_700_000_000_000).toLocaleString(),
    );
  });

  it("renders the epoch itself", () => {
    expect(formatTime(0)).toBe(new Date(0).toLocaleString());
  });

  it("renders a pre-epoch (negative) timestamp", () => {
    expect(formatTime(-86_400)).toBe(new Date(-86_400_000).toLocaleString());
  });
});

describe("unifiedText", () => {
  it("writes a git-style hunk header followed by prefixed lines", () => {
    const diff = diffOf([
      hunk({ oldStart: 10, oldLines: 2, newStart: 10, newLines: 3, header: "fn main" }, [
        line(0, "context", "let a = 1;"),
        line(1, "deletion", "let b = 2;"),
        line(2, "addition", "let b = 3;"),
        line(3, "addition", "let c = 4;"),
      ]),
    ]);

    expect(unifiedText(diff)).toBe(
      [
        "@@ -10,2 +10,3 @@ fn main",
        " let a = 1;",
        "-let b = 2;",
        "+let b = 3;",
        "+let c = 4;",
      ].join("\n"),
    );
  });

  it("emits a header per hunk and joins them into one block", () => {
    const diff = diffOf([
      hunk({ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, header: "a" }, [
        line(0, "addition", "one"),
      ]),
      hunk({ oldStart: 20, oldLines: 1, newStart: 20, newLines: 1, header: "b" }, [
        line(1, "deletion", "two"),
      ]),
    ]);

    expect(unifiedText(diff).split("\n")).toEqual([
      "@@ -1,1 +1,1 @@ a",
      "+one",
      "@@ -20,1 +20,1 @@ b",
      "-two",
    ]);
  });

  it("keeps an empty header's trailing space in the marker line", () => {
    const diff = diffOf([
      hunk({ oldStart: 1, oldLines: 0, newStart: 1, newLines: 1, header: "" }, []),
    ]);
    expect(unifiedText(diff)).toBe("@@ -1,0 +1,1 @@ ");
  });

  it("preserves empty line content as a bare marker", () => {
    const diff = diffOf([hunk({ header: "h" }, [line(0, "context", "")])]);
    expect(unifiedText(diff).split("\n")[1]).toBe(" ");
  });

  it("returns an empty string for a diff with no hunks", () => {
    expect(unifiedText(diffOf([]))).toBe("");
  });
});
