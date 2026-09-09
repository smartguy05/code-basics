import { describe, expect, it } from "vitest";
import type { FileChange } from "../ipc/types";
import {
  clickModifier,
  contextSelection,
  defaultStashMessage,
  stashMenuLabel,
  stashablePaths,
  toggleSelection,
} from "./changesSelectionLogic";

function file(over: Partial<FileChange> & { path: string }): FileChange {
  return {
    oldPath: null,
    staged: null,
    unstaged: "modified",
    isBinary: false,
    ...over,
  };
}

const ORDER = ["a.ts", "b.ts", "c.ts", "d.ts"];

describe("clickModifier", () => {
  it("prefers a range over a toggle when both keys are held", () => {
    expect(clickModifier({ ctrlKey: true, metaKey: false, shiftKey: true })).toBe("range");
    expect(clickModifier({ ctrlKey: true, metaKey: false, shiftKey: false })).toBe("toggle");
    expect(clickModifier({ ctrlKey: false, metaKey: true, shiftKey: false })).toBe("toggle");
    expect(clickModifier({ ctrlKey: false, metaKey: false, shiftKey: false })).toBe("none");
  });
});

describe("toggleSelection", () => {
  it("replaces the selection on a plain click", () => {
    const result = toggleSelection(new Set(["a.ts", "b.ts"]), "c.ts", "none", ORDER, "a.ts");
    expect([...result.selected]).toEqual(["c.ts"]);
    expect(result.anchor).toBe("c.ts");
  });

  it("adds then removes a path on a toggling click", () => {
    const added = toggleSelection(new Set(["a.ts"]), "c.ts", "toggle", ORDER, "a.ts");
    expect([...added.selected].sort()).toEqual(["a.ts", "c.ts"]);

    const removed = toggleSelection(added.selected, "c.ts", "toggle", ORDER, "a.ts");
    expect([...removed.selected]).toEqual(["a.ts"]);
  });

  it("selects an inclusive span in either direction, keeping the anchor", () => {
    const down = toggleSelection(new Set(), "c.ts", "range", ORDER, "b.ts");
    expect([...down.selected]).toEqual(["b.ts", "c.ts"]);
    expect(down.anchor).toBe("b.ts");

    const up = toggleSelection(new Set(), "a.ts", "range", ORDER, "c.ts");
    expect([...up.selected]).toEqual(["a.ts", "b.ts", "c.ts"]);
    // The anchor does not move, so widening and narrowing the range works.
    expect(up.anchor).toBe("c.ts");
  });

  it("falls back to a plain click when there is nothing to range from", () => {
    const noAnchor = toggleSelection(new Set(["a.ts"]), "c.ts", "range", ORDER, null);
    expect([...noAnchor.selected]).toEqual(["c.ts"]);

    // An anchor that is no longer in the list (the file was committed away).
    const gone = toggleSelection(new Set(), "c.ts", "range", ORDER, "z.ts");
    expect([...gone.selected]).toEqual(["c.ts"]);
  });

  it("never hands back the set it was given", () => {
    const before = new Set(["a.ts"]);
    expect(toggleSelection(before, "a.ts", "toggle", ORDER, null).selected).not.toBe(before);
  });
});

describe("contextSelection", () => {
  it("keeps a selection the right-click landed inside", () => {
    expect([...contextSelection(new Set(["a.ts", "b.ts"]), "b.ts")].sort()).toEqual([
      "a.ts",
      "b.ts",
    ]);
  });

  it("replaces a selection the right-click landed outside", () => {
    expect([...contextSelection(new Set(["a.ts", "b.ts"]), "c.ts")]).toEqual(["c.ts"]);
    expect([...contextSelection(new Set(), "c.ts")]).toEqual(["c.ts"]);
  });
});

describe("stashablePaths", () => {
  const files = [
    file({ path: "a.ts" }),
    file({ path: "b.ts", staged: "modified", unstaged: null }),
    file({ path: "conflict.ts", unstaged: "conflicted" }),
    file({ path: "quiet.ts", staged: null, unstaged: null }),
  ];

  it("keeps only files that have a change and no conflict", () => {
    const selected = new Set(["a.ts", "b.ts", "conflict.ts", "quiet.ts"]);
    expect(stashablePaths(selected, files)).toEqual(["a.ts", "b.ts"]);
  });

  it("drops a path the status no longer lists at all", () => {
    expect(stashablePaths(new Set(["ghost.ts"]), files)).toEqual([]);
  });

  it("sorts, so the prompt and the menu agree on the first name", () => {
    expect(stashablePaths(new Set(["b.ts", "a.ts"]), files)).toEqual(["a.ts", "b.ts"]);
  });
});

describe("stashMenuLabel", () => {
  it("counts the files, and says nothing when there are none", () => {
    expect(stashMenuLabel(1)).toBe("Stash file…");
    expect(stashMenuLabel(3)).toBe("Stash 3 files…");
    expect(stashMenuLabel(0)).toBe("");
  });
});

describe("defaultStashMessage", () => {
  it("names the file, so the stash list is readable later", () => {
    expect(defaultStashMessage(["src/views/ChangesView.tsx"])).toBe("ChangesView.tsx");
    expect(defaultStashMessage(["src/a.ts", "src/b.ts", "src/c.ts"])).toBe("a.ts +2 more");
  });

  it("falls back when there is nothing to name", () => {
    expect(defaultStashMessage([])).toBe("work in progress");
  });
});
