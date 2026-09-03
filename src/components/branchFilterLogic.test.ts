import { describe, expect, it } from "vitest";
import type { Branch } from "../ipc/types";
import {
  branchMatches,
  expansionForQuery,
  filterBranches,
  hasQuery,
} from "./branchFilterLogic";

function branch(name: string, extra: Partial<Branch> = {}): Branch {
  return {
    name,
    isHead: false,
    isRemote: name.startsWith("origin/"),
    upstream: null,
    ...extra,
  };
}

const branches: Branch[] = [
  branch("main", { isHead: true }),
  branch("users/anthony/spike"),
  branch("users/anthony/Feature-Flags"),
  branch("release/1.2"),
  branch("origin/feature/x"),
  branch("origin/main"),
];

describe("hasQuery", () => {
  it("treats an empty or whitespace-only box as no filter", () => {
    expect(hasQuery("")).toBe(false);
    expect(hasQuery("   ")).toBe(false);
    expect(hasQuery(" a ")).toBe(true);
  });
});

describe("branchMatches", () => {
  it("matches on the full name, not just the rendered last segment", () => {
    expect(branchMatches(branch("users/anthony/spike"), "anthony")).toBe(true);
    expect(branchMatches(branch("origin/feature/x"), "feat")).toBe(true);
  });

  it("ignores case on both sides", () => {
    expect(branchMatches(branch("users/anthony/Feature-Flags"), "FEATURE")).toBe(true);
    expect(branchMatches(branch("MAIN"), "main")).toBe(true);
  });

  it("ignores surrounding whitespace in the query", () => {
    expect(branchMatches(branch("release/1.2"), "  release  ")).toBe(true);
  });
});

describe("filterBranches", () => {
  it("returns the very same array for an empty query", () => {
    expect(filterBranches(branches, "")).toBe(branches);
    expect(filterBranches(branches, "   ")).toBe(branches);
  });

  it("keeps only the matches, in their original order", () => {
    expect(filterBranches(branches, "main").map((b) => b.name)).toEqual([
      "main",
      "origin/main",
    ]);
  });

  it("matches a folder segment the tree renders as a folder, not a row", () => {
    expect(filterBranches(branches, "anthony").map((b) => b.name)).toEqual([
      "users/anthony/spike",
      "users/anthony/Feature-Flags",
    ]);
  });

  it("returns nothing at all when nothing matches, so the caller can say so", () => {
    expect(filterBranches(branches, "nothing-like-this")).toEqual([]);
  });

  it("finds the current branch like any other", () => {
    const found = filterBranches(branches, "MAI");
    expect(found.some((b) => b.isHead && b.name === "main")).toBe(true);
  });
});

describe("expansionForQuery", () => {
  const stored = new Set(["section:local"]);

  it("returns the stored set itself when there is no query", () => {
    expect(expansionForQuery(branches, "", stored)).toBe(stored);
    expect(expansionForQuery(branches, "  ", stored)).toBe(stored);
  });

  it("opens every folder on the path to a nested match", () => {
    const open = expansionForQuery(branches, "spike", stored);
    expect(open.has("local:users")).toBe(true);
    expect(open.has("local:users/anthony")).toBe(true);
    expect(open.has("section:local")).toBe(true);
  });

  it("opens the Remote section and its folders for a remote-only match", () => {
    const open = expansionForQuery(branches, "feature/x", stored);
    expect(open.has("section:remote")).toBe(true);
    expect(open.has("remote:origin")).toBe(true);
    expect(open.has("remote:origin/feature")).toBe(true);
  });

  it("opens folders regardless of case", () => {
    const open = expansionForQuery(branches, "FEATURE-FLAGS", stored);
    expect(open.has("local:users/anthony")).toBe(true);
  });

  it("adds nothing for a match that is already at the top level", () => {
    const open = expansionForQuery([branch("main", { isHead: true })], "main", stored);
    expect([...open]).toEqual(["section:local"]);
  });

  it("leaves the stored set untouched and adds nothing when nothing matches", () => {
    const open = expansionForQuery(branches, "nothing-like-this", stored);
    expect([...open]).toEqual(["section:local"]);
    expect([...stored]).toEqual(["section:local"]);
  });

  it("keeps a folder the user opened by hand", () => {
    const byHand = new Set(["section:local", "local:release"]);
    const open = expansionForQuery(branches, "spike", byHand);
    expect(open.has("local:release")).toBe(true);
  });
});
