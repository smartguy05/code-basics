import { describe, expect, it } from "vitest";
import { ancestorsOf, revealPlan } from "./fileTreeRevealLogic";

describe("ancestorsOf", () => {
  it("lists every directory prefix, outermost first", () => {
    expect(ancestorsOf("src/views/architecture/DiagramCanvas.tsx")).toEqual([
      "src",
      "src/views",
      "src/views/architecture",
    ]);
  });

  it("yields nothing for a top-level file", () => {
    expect(ancestorsOf("README.md")).toEqual([]);
  });

  it("folds backslashes, which is how a Windows-spelled path arrives", () => {
    expect(ancestorsOf("src\\components\\FileTree.tsx")).toEqual(["src", "src/components"]);
  });

  it("accepts a path that mixes both separators", () => {
    expect(ancestorsOf("crates\\core/src/git\\patch.rs")).toEqual([
      "crates",
      "crates/core",
      "crates/core/src",
      "crates/core/src/git",
    ]);
  });

  it("collapses repeated separators rather than inventing an empty directory", () => {
    expect(ancestorsOf("src//views///RunView.tsx")).toEqual(["src", "src/views"]);
  });

  it("has no answer for the root, the empty path, or a path of only separators", () => {
    expect(ancestorsOf("")).toEqual([]);
    expect(ancestorsOf("/")).toEqual([]);
    expect(ancestorsOf("\\")).toEqual([]);
    expect(ancestorsOf("///")).toEqual([]);
  });

  it("ignores a leading separator instead of treating the root as a directory", () => {
    expect(ancestorsOf("/src/main.rs")).toEqual(["src"]);
  });
});

describe("revealPlan", () => {
  it("expands every ancestor and loads the ones with no listing yet", () => {
    expect(revealPlan("src/views/architecture/DiagramCanvas.tsx", [])).toEqual({
      expand: ["src", "src/views", "src/views/architecture"],
      load: ["src", "src/views", "src/views/architecture"],
    });
  });

  it("still expands a cached directory, because a listing does not mean an open row", () => {
    expect(revealPlan("src/views/architecture/DiagramCanvas.tsx", ["src", "src/views"])).toEqual({
      expand: ["src", "src/views", "src/views/architecture"],
      load: ["src/views/architecture"],
    });
  });

  it("keeps the load order outermost first so each fetch has its parent on screen", () => {
    // The cached entry is the middle one, which is the case that would expose a
    // plan built by walking up from the file instead of down from the root.
    expect(revealPlan("a/b/c/d.ts", ["a/b"]).load).toEqual(["a", "a/b/c"]);
  });

  it("asks for nothing when every ancestor is already loaded", () => {
    expect(revealPlan("src/main.tsx", ["src", "src/views"])).toEqual({
      expand: ["src"],
      load: [],
    });
  });

  it("abstains when there is no open file", () => {
    expect(revealPlan(null, ["src"])).toEqual({ expand: [], load: [] });
    expect(revealPlan("", ["src"])).toEqual({ expand: [], load: [] });
    expect(revealPlan("   ", ["src"])).toEqual({ expand: [], load: [] });
  });

  it("plans nothing to expand for a file at the workspace root", () => {
    expect(revealPlan("README.md", [])).toEqual({ expand: [], load: [] });
  });

  it("reads the loaded set from any iterable, which is what a Map's keys are", () => {
    const listings = new Map([
      ["", []],
      ["src", []],
    ]);
    expect(revealPlan("src/views/RunView.tsx", listings.keys()).load).toEqual(["src/views"]);
  });
});
