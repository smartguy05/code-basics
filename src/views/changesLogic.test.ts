import { describe, expect, it } from "vitest";
import {
  buildSections,
  sortFilesByRisk,
  statusLetter,
  type FileSection,
} from "./changesLogic";
import type { ChangeKind, Changelist, FileChange } from "../ipc/types";

function change(
  path: string,
  staged: ChangeKind | null,
  unstaged: ChangeKind | null,
): FileChange {
  return { path, oldPath: null, staged, unstaged, isBinary: false };
}

describe("statusLetter", () => {
  it("maps added and untracked to A", () => {
    expect(statusLetter(change("a", "added", null), "staged").letter).toBe("A");
    expect(statusLetter(change("a", null, "untracked"), "unstaged").letter).toBe("A");
  });

  it("maps deleted to D and renamed to R", () => {
    expect(statusLetter(change("a", "deleted", null), "staged").letter).toBe("D");
    expect(statusLetter(change("a", "renamed", null), "staged").letter).toBe("R");
  });

  it("falls back to M for modified and for anything unrecognised", () => {
    expect(statusLetter(change("a", "modified", null), "staged").letter).toBe("M");
    expect(statusLetter(change("a", "typeChange", null), "staged").letter).toBe("M");
    expect(statusLetter(change("a", "copied", null), "staged").letter).toBe("M");
    expect(statusLetter(change("a", "unknown", null), "staged").letter).toBe("M");
  });

  it("uses the section's side, not the file, to pick the kind", () => {
    const partial = change("a", "added", "modified");
    expect(statusLetter(partial, "staged").letter).toBe("A");
    expect(statusLetter(partial, "unstaged").letter).toBe("M");
  });

  it("returns M when the requested side has no change", () => {
    expect(statusLetter(change("a", "added", null), "unstaged").letter).toBe("M");
  });

  it("classNames the row by the section side", () => {
    expect(statusLetter(change("a", "added", null), "staged").className).toBe("staged");
    expect(statusLetter(change("a", null, "added"), "unstaged").className).toBe(
      "unstaged",
    );
  });

  it("reports a conflict from either side, whichever side is asked", () => {
    const stagedConflict = change("a", "conflicted", "modified");
    expect(statusLetter(stagedConflict, "staged")).toEqual({
      letter: "!",
      className: "conflicted",
    });
    expect(statusLetter(stagedConflict, "unstaged")).toEqual({
      letter: "!",
      className: "conflicted",
    });

    const unstagedConflict = change("a", null, "conflicted");
    expect(statusLetter(unstagedConflict, "staged").letter).toBe("!");
  });
});

describe("buildSections", () => {
  const paths = (files: FileChange[] | undefined) => (files ?? []).map((f) => f.path);

  /** Paths in the section with this key; empty if there is no such section. */
  const inSection = (sections: FileSection[], key: string) =>
    paths(sections.find((s) => s.key === key)?.files);

  it("always emits Staged first and Unstaged last, groups in between", () => {
    const groups: Changelist[] = [
      { name: "one", paths: [] },
      { name: "two", paths: [] },
    ];
    expect(buildSections([], groups).map((s) => s.key)).toEqual([
      "staged",
      "group:one",
      "group:two",
      "unstaged",
    ]);
  });

  it("splits staged and unstaged work", () => {
    const files = [
      change("s.ts", "modified", null),
      change("u.ts", null, "modified"),
    ];
    const sections = buildSections(files, []);
    expect(inSection(sections, "staged")).toEqual(["s.ts"]);
    expect(inSection(sections, "unstaged")).toEqual(["u.ts"]);
  });

  it("lists a partially staged file in BOTH the staged and unstaged sections", () => {
    const files = [change("p.ts", "modified", "modified")];
    const sections = buildSections(files, []);
    expect(inSection(sections, "staged")).toEqual(["p.ts"]);
    expect(inSection(sections, "unstaged")).toEqual(["p.ts"]);
  });

  it("puts untracked files in the unstaged section", () => {
    const files = [change("new.ts", null, "untracked")];
    const sections = buildSections(files, []);
    expect(inSection(sections, "staged")).toEqual([]);
    expect(inSection(sections, "unstaged")).toEqual(["new.ts"]);
  });

  it("moves an unstaged file out of Unstaged and into its group", () => {
    const files = [change("a.ts", null, "modified"), change("b.ts", null, "modified")];
    const groups: Changelist[] = [{ name: "feature", paths: ["a.ts"] }];
    const sections = buildSections(files, groups);
    expect(inSection(sections, "group:feature")).toEqual(["a.ts"]);
    expect(inSection(sections, "unstaged")).toEqual(["b.ts"]);
  });

  it("never routes staged work into a group, even when the path is grouped", () => {
    const files = [change("a.ts", "modified", null)];
    const groups: Changelist[] = [{ name: "feature", paths: ["a.ts"] }];
    const sections = buildSections(files, groups);
    expect(inSection(sections, "staged")).toEqual(["a.ts"]);
    expect(inSection(sections, "group:feature")).toEqual([]);
    expect(inSection(sections, "unstaged")).toEqual([]);
  });

  it("still shows a partially staged grouped file under Staged and its group", () => {
    const files = [change("a.ts", "modified", "modified")];
    const groups: Changelist[] = [{ name: "feature", paths: ["a.ts"] }];
    const sections = buildSections(files, groups);
    expect(inSection(sections, "staged")).toEqual(["a.ts"]);
    expect(inSection(sections, "group:feature")).toEqual(["a.ts"]);
    expect(inSection(sections, "unstaged")).toEqual([]);
  });

  it("keeps custom groups when empty but not Staged/Unstaged", () => {
    const sections = buildSections([], [{ name: "feature", paths: [] }]);
    expect(sections.map((s) => s.keepWhenEmpty)).toEqual([false, true, false]);
  });

  it("labels each section and marks which side it draws", () => {
    const sections = buildSections([], [{ name: "feature", paths: [] }]);
    expect(sections.map((s) => s.label)).toEqual(["Staged", "feature", "Unstaged"]);
    expect(sections.map((s) => s.side)).toEqual(["staged", "unstaged", "unstaged"]);
    expect(sections.map((s) => s.group)).toEqual([null, "feature", null]);
  });

  it("assigns a file to the first group that claims it", () => {
    const files = [change("a.ts", null, "modified")];
    const groups: Changelist[] = [
      { name: "one", paths: ["a.ts"] },
      { name: "two", paths: ["a.ts"] },
    ];
    const sections = buildSections(files, groups);
    expect(inSection(sections, "group:one")).toEqual(["a.ts"]);
    expect(inSection(sections, "group:two")).toEqual([]);
    expect(inSection(sections, "unstaged")).toEqual([]);
  });
});

describe("sortFilesByRisk", () => {
  /** A risk lookup driven by a fixed score table; anything absent abstains. */
  const riskOf = (scores: Record<string, number>) => (path: string) => {
    const score = scores[path];
    return score === undefined ? null : { level: "elevated" as const, score };
  };

  it("orders files within a section by risk score, highest first", () => {
    const files = [
      change("low.ts", null, "modified"),
      change("high.ts", null, "modified"),
      change("mid.ts", null, "modified"),
    ];
    const sorted = sortFilesByRisk(
      buildSections(files, []),
      riskOf({ "low.ts": 1, "mid.ts": 3, "high.ts": 9 }),
    );
    const unstaged = sorted.find((s) => s.key === "unstaged");
    expect(unstaged?.files.map((f) => f.path)).toEqual(["high.ts", "mid.ts", "low.ts"]);
  });

  it("keeps git order for files that score equally (stable)", () => {
    const files = [
      change("a.ts", null, "modified"),
      change("b.ts", null, "modified"),
      change("c.ts", null, "modified"),
    ];
    // No file has any risk signal — every score is 0, so the order must not move.
    const sorted = sortFilesByRisk(buildSections(files, []), riskOf({}));
    const unstaged = sorted.find((s) => s.key === "unstaged");
    expect(unstaged?.files.map((f) => f.path)).toEqual(["a.ts", "b.ts", "c.ts"]);
  });

  it("sorts within each partition, never across them", () => {
    const files = [
      change("s.ts", "modified", null),
      change("u1.ts", null, "modified"),
      change("u2.ts", null, "modified"),
    ];
    // The staged file scores highest, but it must stay in Staged, not jump into
    // the unstaged list.
    const sorted = sortFilesByRisk(
      buildSections(files, []),
      riskOf({ "s.ts": 100, "u2.ts": 5 }),
    );
    expect(sorted.find((s) => s.key === "staged")?.files.map((f) => f.path)).toEqual(["s.ts"]);
    expect(sorted.find((s) => s.key === "unstaged")?.files.map((f) => f.path)).toEqual([
      "u2.ts",
      "u1.ts",
    ]);
  });

  it("preserves section labels, sides and keep-when-empty", () => {
    const sections = buildSections([], [{ name: "feature", paths: [] }]);
    const sorted = sortFilesByRisk(sections, riskOf({}));
    expect(sorted.map((s) => s.key)).toEqual(["staged", "group:feature", "unstaged"]);
    expect(sorted.map((s) => s.keepWhenEmpty)).toEqual([false, true, false]);
  });
});
