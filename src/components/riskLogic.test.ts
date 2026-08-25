import { describe, expect, it } from "vitest";
import {
  fileRisk,
  hunkRisk,
  isSensitivePath,
  moreSevereHunkRisk,
} from "./riskLogic";
import type {
  Confidence,
  DiffLine,
  ErosionCategory,
  ErosionFlag,
  FileDiff,
  GroupFile,
  GroupKind,
  Hunk,
  IntentGroup,
  LineOrigin,
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
  const lines = indices.map((i) => line(i, "addition"));
  return {
    oldStart: 1,
    oldLines: 0,
    newStart: 1,
    newLines: indices.length,
    header: "",
    lines,
  };
}

function diffOf(path: string, hunks: Hunk[]): FileDiff {
  return { path, oldPath: null, hunks, isBinary: false };
}

function flag(
  path: string,
  index: number,
  category: ErosionCategory,
): ErosionFlag {
  return {
    path,
    line: index + 1,
    index,
    origin: "addition",
    category,
    ruleId: `${category}-rule`,
    message: `${category} here`,
    content: "x",
  };
}

function group(
  kind: GroupKind,
  files: GroupFile[],
  confidence: Confidence = "high",
): IntentGroup {
  return {
    id: `g-${kind}-${files.map((f) => f.path).join(",")}`,
    kind,
    label: kind,
    files,
    lineCount: files.reduce((n, f) => n + f.lineIndices.length, 0),
    confidence,
  };
}

function gfile(path: string, lineIndices: number[], hunks: number[]): GroupFile {
  return { path, lineIndices, hunks };
}

describe("isSensitivePath", () => {
  it("matches security-relevant path markers at a boundary", () => {
    expect(isSensitivePath("src/auth/login.ts")).toBe(true);
    expect(isSensitivePath("db/migrations/001_init.sql")).toBe(true);
    expect(isSensitivePath(".env.local")).toBe(true);
    expect(isSensitivePath("services/payment.rs")).toBe(true);
  });

  it("does not fire on a bare substring — abstains rather than over-flags", () => {
    expect(isSensitivePath("src/author/profile.ts")).toBe(false);
    expect(isSensitivePath("src/config/app.ts")).toBe(false);
    expect(isSensitivePath("src/components/DiffView.tsx")).toBe(false);
  });
});

describe("fileRisk", () => {
  it("returns null when nothing elevates the file (abstain)", () => {
    const groups = [group("intent", [gfile("src/app.ts", [0], [0])])];
    expect(fileRisk("src/app.ts", [], groups)).toBeNull();
  });

  it("elevates a sensitive path with no other signal", () => {
    expect(fileRisk("src/auth/token.ts", [], [])).toEqual({
      level: "elevated",
      score: 3,
    });
  });

  it("makes a high-risk erosion flag on the file high", () => {
    const risk = fileRisk("src/app.ts", [flag("src/app.ts", 2, "deletedAssertion")], []);
    expect(risk?.level).toBe("high");
  });

  it("keeps a low-severity erosion flag merely elevated", () => {
    const risk = fileRisk("src/app.ts", [flag("src/app.ts", 2, "droppedLog")], []);
    expect(risk).toEqual({ level: "elevated", score: 1 });
  });

  it("only counts flags on this file's own path", () => {
    expect(fileRisk("src/a.ts", [flag("src/b.ts", 2, "secret")], [])).toBeNull();
  });

  it("adds weight for an unexplained or low-confidence card touching the file", () => {
    const other = group("other", [gfile("src/a.ts", [0], [0])]);
    const low = group("intent", [gfile("src/a.ts", [1], [1])], "low");
    // other(+2) + low-confidence(+1) = 3, elevated.
    expect(fileRisk("src/a.ts", [], [other, low])).toEqual({
      level: "elevated",
      score: 3,
    });
  });

  it("scores a high file above an elevated one so it sorts first", () => {
    const highFile = fileRisk("src/app.ts", [flag("src/app.ts", 0, "secret")], []);
    const elevatedFile = fileRisk("src/auth/x.ts", [], []);
    expect(highFile!.score).toBeGreaterThan(elevatedFile!.score);
  });
});

describe("hunkRisk", () => {
  const diff = diffOf("src/app.ts", [hunkWith([0, 1]), hunkWith([2, 3])]);

  it("returns null for a hunk with no signal (abstain)", () => {
    expect(hunkRisk("src/app.ts", 0, diff, [], [])).toBeNull();
  });

  it("returns null for an out-of-range hunk index", () => {
    expect(hunkRisk("src/app.ts", 9, diff, [], [])).toBeNull();
  });

  it("is high when a high-severity erosion flag lands in the hunk", () => {
    const flags = [flag("src/app.ts", 1, "removedSafeguard")];
    expect(hunkRisk("src/app.ts", 0, diff, flags, [])).toBe("high");
  });

  it("is elevated for a lower-severity erosion flag in the hunk, order-independent", () => {
    const flags = [
      flag("src/app.ts", 0, "droppedLog"),
      flag("src/app.ts", 1, "widenedCatch"),
    ];
    expect(hunkRisk("src/app.ts", 0, diff, flags, [])).toBe("elevated");
  });

  it("ignores an erosion flag whose index is in another hunk", () => {
    const flags = [flag("src/app.ts", 3, "secret")];
    // Flag is in hunk 1 (indices 2,3), not hunk 0.
    expect(hunkRisk("src/app.ts", 0, diff, flags, [])).toBeNull();
    expect(hunkRisk("src/app.ts", 1, diff, flags, [])).toBe("high");
  });

  it("recedes a formatting-only hunk to formatting", () => {
    const groups = [group("formatting", [gfile("src/app.ts", [0, 1], [0])])];
    expect(hunkRisk("src/app.ts", 0, diff, [], groups)).toBe("formatting");
  });

  it("does not recede a hunk a non-formatting card also owns", () => {
    const groups = [
      group("formatting", [gfile("src/app.ts", [0, 1], [0])]),
      group("intent", [gfile("src/app.ts", [0], [0])]),
    ];
    expect(hunkRisk("src/app.ts", 0, diff, [], groups)).toBeNull();
  });

  it("emphasises an intent/other change on a sensitive path", () => {
    const sens = diffOf("src/auth/login.ts", [hunkWith([0, 1])]);
    const groups = [group("intent", [gfile("src/auth/login.ts", [0, 1], [0])])];
    expect(hunkRisk("src/auth/login.ts", 0, sens, [], groups)).toBe("elevated");
  });

  it("does not emphasise a plain change on an ordinary path", () => {
    const groups = [group("intent", [gfile("src/app.ts", [0, 1], [0])])];
    expect(hunkRisk("src/app.ts", 0, diff, [], groups)).toBeNull();
  });

  it("lets erosion outrank a formatting owner on the same hunk", () => {
    const groups = [group("formatting", [gfile("src/app.ts", [0, 1], [0])])];
    const flags = [flag("src/app.ts", 0, "secret")];
    expect(hunkRisk("src/app.ts", 0, diff, flags, groups)).toBe("high");
  });
});

describe("moreSevereHunkRisk", () => {
  it("takes the incoming level when nothing is set yet", () => {
    expect(moreSevereHunkRisk(undefined, "formatting")).toBe("formatting");
  });

  it("keeps the more severe of the two", () => {
    expect(moreSevereHunkRisk("formatting", "high")).toBe("high");
    expect(moreSevereHunkRisk("high", "elevated")).toBe("high");
    expect(moreSevereHunkRisk("elevated", "formatting")).toBe("elevated");
  });
});
