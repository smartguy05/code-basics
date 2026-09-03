import { describe, it, expect } from "vitest";
import { diffFile, secretsFile, workspaceFile } from "./editorSourceLogic";
import {
  changesOnScreen,
  type ChangesVisibility,
  DIFF_SOURCE_KIND,
  paneForAction,
  shouldPollChanges,
  shouldRefreshChanges,
  toolbarFor,
  type ToolbarTab,
} from "./projectViewLogic";

/** A tab backed by the given source kind — the only field the toolbar reads. */
function tab(kind: string): ToolbarTab {
  return { source: { kind } };
}

describe("toolbarFor", () => {
  it("shows the Changes toolbar for a diff tab", () => {
    expect(toolbarFor(tab(DIFF_SOURCE_KIND))).toBe("changes");
  });

  it("shows the Run toolbar for an ordinary workspace file", () => {
    expect(toolbarFor(tab("workspace"))).toBe("run");
  });

  it("shows the Run toolbar for a secrets tab", () => {
    // Secrets are an ordinary file being edited, not a change being reviewed.
    expect(toolbarFor(tab("secrets"))).toBe("run");
  });

  it("shows the Run toolbar with no tab open", () => {
    // The Run controls are workspace-level and stay meaningful with an empty
    // editor area; a Changes toolbar there would be all-disabled buttons.
    expect(toolbarFor(null)).toBe("run");
    expect(toolbarFor(undefined)).toBe("run");
  });

  it("degrades an unrecognised source kind to the Run toolbar", () => {
    // The asymmetry that makes this the safe default: unknown + Run is a set of
    // workspace-level buttons, unknown + Changes would arm Stage/Revert against
    // a file that is not the one on screen.
    expect(toolbarFor(tab("imagePreview"))).toBe("run");
    expect(toolbarFor(tab(""))).toBe("run");
  });

  it("reads only the source kind, never the tab's identity", () => {
    // Pinned, dirty, differently-labelled — none of it changes the answer.
    const decorated = { id: "a.ts", name: "a.ts", pinned: true, source: { kind: "workspace" } };
    expect(toolbarFor(decorated)).toBe("run");
  });
});

describe("paneForAction", () => {
  it("follows a rail click unconditionally", () => {
    expect(paneForAction({ kind: "railSelect", pane: "files" })).toBe("files");
    expect(paneForAction({ kind: "railSelect", pane: "changes" })).toBe("changes");
  });

  it("requires the Files pane to reveal a file in the tree", () => {
    expect(paneForAction({ kind: "revealInTree" })).toBe("files");
  });

  it("requires the Changes pane to highlight a change", () => {
    expect(paneForAction({ kind: "highlightChange" })).toBe("changes");
  });

  it("has no opinion when a tab is opened into the editor area", () => {
    // The result of opening a tab is in the main area, which both panes share,
    // so moving the rail would spend the user's navigation state for nothing.
    expect(paneForAction({ kind: "openEditorTab" })).toBeNull();
  });

  it("distinguishes 'no opinion' from a pane, so a caller can skip the update", () => {
    const implied = paneForAction({ kind: "openEditorTab" });
    expect(implied === null).toBe(true);
    // Never the current pane echoed back — that would be an invented answer.
    expect(implied).not.toBe("files");
    expect(implied).not.toBe("changes");
  });
});

describe("changes-panel visibility", () => {
  /** On screen unless something says otherwise. */
  const shown = (over: Partial<ChangesVisibility> = {}): ChangesVisibility => ({
    pane: "changes",
    tabForeground: true,
    codebaseActive: true,
    ...over,
  });

  describe("changesOnScreen", () => {
    it("needs all three gates, not just the rail", () => {
      // The merge destroyed two mount gates (`active && tab === "changes"`) and
      // added a third. Restoring only the rail is the regression this pins.
      expect(changesOnScreen(shown())).toBe(true);
      expect(changesOnScreen(shown({ pane: "files" }))).toBe(false);
      expect(changesOnScreen(shown({ tabForeground: false }))).toBe(false);
      expect(changesOnScreen(shown({ codebaseActive: false }))).toBe(false);
    });
  });

  describe("shouldRefreshChanges", () => {
    it("refreshes when the rail switches to changes", () => {
      expect(shouldRefreshChanges(shown({ pane: "files" }), shown())).toBe(true);
    });

    it("refreshes when the user leaves the Project tab and comes back", () => {
      // The old conditional mount covered this and a rail-only predicate does
      // not: an agent working in a terminal changes the tree while the user is
      // on Tests, and the list must not come back stale.
      expect(shouldRefreshChanges(shown({ tabForeground: false }), shown())).toBe(true);
    });

    it("refreshes when this codebase becomes the foreground one again", () => {
      expect(shouldRefreshChanges(shown({ codebaseActive: false }), shown())).toBe(true);
    });

    it("refreshes on a first mount that is already on screen", () => {
      expect(shouldRefreshChanges(null, shown())).toBe(true);
    });

    it("does not refresh on a first mount that is not on screen", () => {
      expect(shouldRefreshChanges(null, shown({ pane: "files" }))).toBe(false);
    });

    it("does not refresh when nothing left the screen", () => {
      // Clicking the already-selected rail icon expressed no intent to refresh,
      // and a predicate that said yes here would refetch git on every render.
      expect(shouldRefreshChanges(shown(), shown())).toBe(false);
    });

    it("does not refresh when the panel is going away", () => {
      expect(shouldRefreshChanges(shown(), shown({ pane: "files" }))).toBe(false);
      expect(shouldRefreshChanges(shown(), shown({ tabForeground: false }))).toBe(false);
    });
  });

  describe("shouldPollChanges", () => {
    it("polls only while the panel is genuinely on screen", () => {
      expect(shouldPollChanges(shown(), false)).toBe(true);
      expect(shouldPollChanges(shown({ pane: "files" }), false)).toBe(false);
      // The two the merge destroyed: without these every open codebase scans
      // git on a timer behind the Tests tab.
      expect(shouldPollChanges(shown({ tabForeground: false }), false)).toBe(false);
      expect(shouldPollChanges(shown({ codebaseActive: false }), false)).toBe(false);
    });

    it("stays stopped while the panel says it is paused", () => {
      expect(shouldPollChanges(shown(), true)).toBe(false);
    });
  });
});

describe("the join with editorSourceLogic", () => {
  it("DIFF_SOURCE_KIND is the kind a real diff source actually carries", () => {
    // Without this the constant and the variant can drift apart silently: every
    // toolbar test feeds the constant back into the function under test, so
    // renaming one side leaves the suite green while every diff tab quietly
    // gets the Run toolbar. TypeScript cannot help — `ToolbarTab.source.kind`
    // is a bare string on purpose, so the module composes with new variants.
    expect(diffFile("a.ts", "workingToHead").source.kind).toBe(DIFF_SOURCE_KIND);
    expect(toolbarFor(diffFile("a.ts", "workingToHead"))).toBe("changes");
  });

  it("gives the Run toolbar to the source kinds that are not diffs", () => {
    expect(toolbarFor(workspaceFile("a.ts"))).toBe("run");
    expect(toolbarFor(secretsFile("Api"))).toBe("run");
  });
});
