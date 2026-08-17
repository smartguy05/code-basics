import { describe, expect, it } from "vitest";
import {
  allChangedIndices,
  changeMarks,
  focusedBaseline,
  hunkIndices,
  mapOffset,
  nextChangeLine,
  normaliseEndings,
  normaliseWhitespace,
  onlyHunks,
  scrollLeftForThumb,
  scrollThumb,
} from "./diffLogic";
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

/** A hunk placed at a known position in the working document. */
function placed(newStart: number, origins: LineOrigin[]): Hunk {
  const lines = origins.map((origin, i) => line(i, origin));
  return {
    oldStart: newStart,
    oldLines: origins.filter((o) => o !== "addition").length,
    newStart,
    newLines: origins.filter((o) => o !== "deletion").length,
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

describe("onlyHunks", () => {
  it("keeps just the named hunks, in diff order", () => {
    const scoped = onlyHunks(multiHunk, [1]);
    expect(scoped.hunks).toEqual([multiHunk.hunks[1]]);
    expect(scoped.path).toBe(multiHunk.path);
  });

  it("ignores indices the diff does not have", () => {
    expect(onlyHunks(multiHunk, [1, 99]).hunks).toEqual([multiHunk.hunks[1]]);
  });

  it("orders by the diff, not by the requested list", () => {
    expect(onlyHunks(multiHunk, [1, 0]).hunks).toEqual(multiHunk.hunks);
  });

  it("returns no hunks for an empty request", () => {
    expect(onlyHunks(multiHunk, []).hunks).toEqual([]);
  });
});

describe("changeMarks", () => {
  it("places a mark at the hunk's proportional position in the document", () => {
    // A hunk of two lines starting at line 51 of a 100-line file.
    const diff = diffOf([placed(51, ["addition", "addition"])]);
    expect(changeMarks(diff, 100)).toEqual([{ top: 0.5, height: 0.02, kind: "addition" }]);
  });

  it("calls a hunk with both additions and deletions a modification", () => {
    const diff = diffOf([placed(1, ["deletion", "addition"])]);
    expect(changeMarks(diff, 10).at(0)?.kind).toBe("modification");
  });

  it("calls a hunk with only deletions a deletion", () => {
    const diff = diffOf([placed(1, ["deletion", "deletion"])]);
    expect(changeMarks(diff, 10).at(0)?.kind).toBe("deletion");
  });

  it("still marks a pure deletion, which occupies no line in the working copy", () => {
    // `newLines` is 0 for a hunk that only removes text, so a naive
    // height-from-newLines would draw nothing at all — the one change most
    // worth being able to find.
    const diff = diffOf([placed(20, ["deletion"])]);
    const marks = changeMarks(diff, 100);

    expect(marks).toHaveLength(1);
    expect(marks.at(0)?.height).toBeGreaterThan(0);
    expect(marks.at(0)?.kind).toBe("deletion");
  });

  it("ignores context-only hunks, which are not changes", () => {
    expect(changeMarks(diffOf([placed(1, ["context", "context"])]), 10)).toEqual([]);
  });

  it("keeps every mark inside the strip even when the diff overruns the document", () => {
    // The diff and the editor document are fetched separately, so a file
    // written between the two calls can leave a hunk pointing past the end.
    const diff = diffOf([placed(90, ["addition", "addition", "addition"])]);
    const mark = changeMarks(diff, 10).at(0);

    expect(mark).toBeDefined();
    expect(mark!.top).toBeGreaterThanOrEqual(0);
    expect(mark!.top).toBeLessThanOrEqual(1);
    expect(mark!.top + mark!.height).toBeLessThanOrEqual(1);
  });

  it("returns nothing for an empty diff or an empty document", () => {
    expect(changeMarks(diffOf([]), 100)).toEqual([]);
    expect(changeMarks(diffOf([placed(1, ["addition"])]), 0)).toEqual([]);
  });

  it("marks every hunk, in document order", () => {
    const diff = diffOf([placed(10, ["addition"]), placed(50, ["deletion", "addition"])]);
    const marks = changeMarks(diff, 100);

    expect(marks).toHaveLength(2);
    expect(marks.at(0)!.top).toBeLessThan(marks.at(1)!.top);
  });
});

describe("nextChangeLine", () => {
  const diff = diffOf([
    placed(10, ["addition"]),
    placed(40, ["deletion", "addition"]),
    placed(80, ["addition"]),
  ]);

  it("finds the next change after the cursor", () => {
    expect(nextChangeLine(diff, 1, 1)).toBe(10);
    expect(nextChangeLine(diff, 10, 1)).toBe(40);
    expect(nextChangeLine(diff, 41, 1)).toBe(80);
  });

  it("finds the previous change before the cursor", () => {
    expect(nextChangeLine(diff, 100, -1)).toBe(80);
    expect(nextChangeLine(diff, 80, -1)).toBe(40);
    expect(nextChangeLine(diff, 41, -1)).toBe(40);
  });

  it("wraps around at both ends", () => {
    // Rider wraps, and stopping dead at the last change reads as a broken key.
    expect(nextChangeLine(diff, 80, 1)).toBe(10);
    expect(nextChangeLine(diff, 10, -1)).toBe(80);
  });

  it("returns null when there is nothing to jump to", () => {
    expect(nextChangeLine(diffOf([]), 1, 1)).toBeNull();
    expect(nextChangeLine(diffOf([placed(1, ["context"])]), 1, 1)).toBeNull();
  });

  it("goes to the single change from either direction rather than standing still", () => {
    const one = diffOf([placed(5, ["addition"])]);
    expect(nextChangeLine(one, 1, 1)).toBe(5);
    expect(nextChangeLine(one, 9, -1)).toBe(5);
    // Already on it: wrapping lands back on the same change, which is correct —
    // there is nowhere else to go.
    expect(nextChangeLine(one, 5, 1)).toBe(5);
  });
});

describe("normaliseWhitespace", () => {
  it("drops leading and trailing whitespace on every line", () => {
    expect(normaliseWhitespace("   let a = 1;   \n\tlet b = 2;\t").text).toBe(
      "let a = 1;\nlet b = 2;",
    );
  });

  it("collapses runs of internal whitespace to one space", () => {
    expect(normaliseWhitespace("let  a   =    1;").text).toBe("let a = 1;");
  });

  it("keeps the line structure, so line-level chunks still line up", () => {
    expect(normaliseWhitespace("a\n  b  \nc").text).toBe("a\nb\nc");
  });

  it("drops a carriage return, so a line-ending change is not a change", () => {
    expect(normaliseWhitespace("a\r\nb\r\n").text).toBe(normaliseWhitespace("a\nb\n").text);
  });

  it("maps every kept character back to where it came from", () => {
    // "  ab" -> "ab": the 'a' came from offset 2, the 'b' from offset 3.
    const { text, map } = normaliseWhitespace("  ab");

    expect(text).toBe("ab");
    expect(map[0]).toBe(2);
    expect(map[1]).toBe(3);
  });

  it("has one more map entry than characters, for an end offset", () => {
    // An exclusive end offset must be mappable, so the map carries a final
    // entry pointing past the last character.
    const source = "  ab  ";
    const { text, map } = normaliseWhitespace(source);

    expect(map).toHaveLength(text.length + 1);
    expect(map[text.length]).toBe(source.length);
  });

  it("leaves text with no redundant whitespace untouched", () => {
    const source = "let a = 1;\nlet b = 2;";
    expect(normaliseWhitespace(source).text).toBe(source);
  });

  it("handles an empty document", () => {
    const { text, map } = normaliseWhitespace("");
    expect(text).toBe("");
    expect(map).toEqual([0]);
  });
});

describe("mapOffset", () => {
  it("translates a normalised offset back to the original document", () => {
    const { map } = normaliseWhitespace("    let a = 1;");
    expect(mapOffset(map, 0)).toBe(4);
  });

  it("clamps an offset past the end rather than returning undefined", () => {
    // The offsets come from a diff over the normalised text, so they should
    // always be in range — but a NaN reaching a CodeMirror range throws, and a
    // silently wrong highlight is cheaper than a broken editor.
    const { map } = normaliseWhitespace("abc");
    expect(mapOffset(map, 99)).toBe(3);
  });

  it("clamps a negative offset to the start", () => {
    const { map } = normaliseWhitespace("abc");
    expect(mapOffset(map, -1)).toBe(0);
  });
});

describe("scrollThumb", () => {
  it("sizes the thumb by the visible fraction of the content", () => {
    const thumb = scrollThumb({ contentWidth: 1000, viewportWidth: 250, scrollLeft: 0, trackWidth: 400 });
    expect(thumb.width).toBe(100);
    expect(thumb.left).toBe(0);
  });

  it("moves the thumb proportionally through the track", () => {
    // 750px of scrollable range, half consumed, 300px of free track.
    const thumb = scrollThumb({ contentWidth: 1000, viewportWidth: 250, scrollLeft: 375, trackWidth: 400 });
    expect(thumb.left).toBeCloseTo(150);
  });

  it("pins the thumb to the end when scrolled fully right", () => {
    const thumb = scrollThumb({ contentWidth: 1000, viewportWidth: 250, scrollLeft: 750, trackWidth: 400 });
    expect(thumb.left + thumb.width).toBeCloseTo(400);
  });

  it("reports nothing to scroll when the content fits", () => {
    const thumb = scrollThumb({ contentWidth: 200, viewportWidth: 250, scrollLeft: 0, trackWidth: 400 });
    expect(thumb.scrollable).toBe(false);
  });

  it("asks for no scroll at either end of a track that cannot scroll", () => {
    // Content that fits, and a track with no width yet: both must answer 0
    // rather than dividing by zero.
    expect(
      scrollLeftForThumb({ contentWidth: 200, viewportWidth: 250, scrollLeft: 0, trackWidth: 400 }, 100),
    ).toBe(0);
    expect(
      scrollLeftForThumb({ contentWidth: 1000, viewportWidth: 250, scrollLeft: 0, trackWidth: 0 }, 100),
    ).toBe(0);
  });

  it("clamps a thumb dragged past either end of the track", () => {
    const metrics = { contentWidth: 1000, viewportWidth: 250, scrollLeft: 0, trackWidth: 400 };

    expect(scrollLeftForThumb(metrics, -500)).toBe(0);
    expect(scrollLeftForThumb(metrics, 99_999)).toBe(750);
  });

  it("cannot scroll when the thumb fills the whole track", () => {
    // The minimum thumb width can equal the track on a very narrow pane;
    // dividing by the remaining travel would be a divide by zero.
    const metrics = { contentWidth: 1000, viewportWidth: 999, scrollLeft: 0, trackWidth: 20 };

    expect(scrollLeftForThumb(metrics, 10)).toBe(0);
  });

  it("keeps the thumb grabbable for very wide content", () => {
    // A minified line can be 100x the viewport; a proportional thumb would be
    // sub-pixel and impossible to hit.
    const thumb = scrollThumb({ contentWidth: 100_000, viewportWidth: 250, scrollLeft: 0, trackWidth: 400 });
    expect(thumb.width).toBeGreaterThanOrEqual(20);
  });

  it("round-trips with scrollLeftForThumb, so a dropped thumb lands where it was drawn", () => {
    const metrics = { contentWidth: 1000, viewportWidth: 250, scrollLeft: 375, trackWidth: 400 };
    const thumb = scrollThumb(metrics);

    expect(scrollLeftForThumb(metrics, thumb.left)).toBeCloseTo(metrics.scrollLeft);
  });

  it("survives a zero-width track without producing NaN", () => {
    // The bar is measured on mount, before layout has given it a width.
    const thumb = scrollThumb({ contentWidth: 1000, viewportWidth: 0, scrollLeft: 0, trackWidth: 0 });
    expect(Number.isFinite(thumb.left)).toBe(true);
    expect(Number.isFinite(thumb.width)).toBe(true);
  });
});

describe("normaliseEndings", () => {
  it("brings CRLF, lone CR and LF all to LF", () => {
    expect(normaliseEndings("a\r\nb\rc\nd")).toBe("a\nb\nc\nd");
  });

  it("leaves an already-LF document untouched", () => {
    expect(normaliseEndings("a\nb\nc\n")).toBe("a\nb\nc\n");
  });
});

describe("focusedBaseline", () => {
  /** A diff line with explicit content and working-side line number. */
  function ln(origin: LineOrigin, content: string, newLineno: number | null): DiffLine {
    return { index: 0, origin, content, oldLineno: null, newLineno, noNewline: false };
  }

  it("reverts only the named hunk, leaving the rest of the file identical", () => {
    // Working file: an insertion of two lines and a replaced line, framed by
    // unchanged text above and below.
    const working = "top\nINS-1\nINS-2\nkeep\nbottom";
    const h: Hunk = {
      oldStart: 2,
      oldLines: 2,
      newStart: 2,
      newLines: 3, // INS-1, INS-2, keep
      header: "",
      lines: [
        ln("addition", "INS-1", 2),
        ln("addition", "INS-2", 3),
        ln("deletion", "OLD", null),
        ln("context", "keep", 4),
      ],
    };

    // The hunk is reverted (additions gone, deletion restored); top/bottom stay.
    expect(focusedBaseline(working, [h])).toBe("top\nOLD\nkeep\nbottom");
  });

  it("applies several hunks without their positions drifting", () => {
    const working = "a\nADD-1\nb\nADD-2\nc";
    const first: Hunk = {
      oldStart: 2, oldLines: 0, newStart: 2, newLines: 1, header: "",
      lines: [ln("addition", "ADD-1", 2)],
    };
    const second: Hunk = {
      oldStart: 3, oldLines: 0, newStart: 4, newLines: 1, header: "",
      lines: [ln("addition", "ADD-2", 4)],
    };

    // Both insertions removed; the surviving lines keep their order.
    expect(focusedBaseline(working, [first, second])).toBe("a\nb\nc");
  });

  it("skips a hunk pointing past the end rather than throwing", () => {
    const working = "a\nb";
    const stale: Hunk = {
      oldStart: 99, oldLines: 0, newStart: 99, newLines: 1, header: "",
      lines: [ln("addition", "gone", 99)],
    };
    expect(focusedBaseline(working, [stale])).toBe("a\nb");
  });
});
