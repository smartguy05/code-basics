import { describe, expect, it } from "vitest";
import {
  RECENT_FILES_CAP,
  SELECTION_TEXT_CAP,
  buildEditorContext,
  capSelectionText,
  editorContextEqual,
  pushRecent,
  type EditorContextInput,
  type LiveEditorPosition,
} from "./editorContextLogic";
import { diffFile, secretsFile, workspaceFile } from "./editorSourceLogic";
import type { EditorContext, EditorSelection } from "../ipc/types";

const live = (over: Partial<LiveEditorPosition> = {}): LiveEditorPosition => ({
  cursor: { line: 3, character: 5 },
  viewport: { firstVisibleLine: 1, lastVisibleLine: 40 },
  selection: null,
  ...over,
});

const selection = (over: Partial<EditorSelection> = {}): EditorSelection => ({
  startLine: 2,
  startCharacter: 0,
  endLine: 2,
  endCharacter: 4,
  text: "code",
  ...over,
});

const input = (over: Partial<EditorContextInput> = {}): EditorContextInput => ({
  openFiles: [],
  activeFileId: null,
  dirtyIds: new Set(),
  pinnedIds: new Set(),
  recent: [],
  live: null,
  ...over,
});

describe("capSelectionText", () => {
  it("returns short text unchanged", () => {
    expect(capSelectionText("hello")).toBe("hello");
  });

  it("truncates to the cap", () => {
    const text = "a".repeat(SELECTION_TEXT_CAP + 100);
    expect(capSelectionText(text)).toHaveLength(SELECTION_TEXT_CAP);
  });

  it("respects an explicit cap", () => {
    expect(capSelectionText("abcdef", 3)).toBe("abc");
  });

  it("never splits a surrogate pair", () => {
    // Each emoji is two UTF-16 code units but one code point, so a naive
    // `slice(0, cap)` at an odd boundary would cut one in half and render a
    // replacement glyph. Capping by code points cannot.
    const text = "😀".repeat(10);
    const capped = capSelectionText(text, 3);
    expect(Array.from(capped)).toHaveLength(3);
    expect(capped).toBe("😀😀😀");
  });

  it("keeps text whose code-unit length exceeds the cap but code-point count does not", () => {
    // 3 emoji = 6 code units, over a cap of 4, but only 3 code points — so it is
    // under the cap and must be returned whole rather than clipped to 2.
    const text = "😀😀😀";
    expect(capSelectionText(text, 4)).toBe(text);
  });
});

describe("pushRecent", () => {
  it("prepends a new path", () => {
    expect(pushRecent(["b", "c"], "a")).toEqual(["a", "b", "c"]);
  });

  it("dedupes by moving an existing path to the front", () => {
    expect(pushRecent(["b", "a", "c"], "a")).toEqual(["a", "b", "c"]);
  });

  it("returns the same reference when the path is already first", () => {
    const recent = ["a", "b"];
    expect(pushRecent(recent, "a")).toBe(recent);
  });

  it("caps the list, dropping the oldest", () => {
    const recent = Array.from({ length: RECENT_FILES_CAP }, (_, i) => `f${i}`);
    const next = pushRecent(recent, "new");
    expect(next).toHaveLength(RECENT_FILES_CAP);
    expect(next[0]).toBe("new");
    expect(next).not.toContain(`f${RECENT_FILES_CAP - 1}`);
  });

  it("respects an explicit cap", () => {
    expect(pushRecent(["b", "c"], "a", 2)).toEqual(["a", "b"]);
  });
});

describe("buildEditorContext", () => {
  it("includes only workspace files, with their per-tab flags", () => {
    const ws = workspaceFile("src/a.ts");
    const ctx = buildEditorContext(
      input({
        openFiles: [ws, secretsFile("Proj"), diffFile("src/b.ts", "workingToHead")],
        activeFileId: ws.id,
        dirtyIds: new Set([ws.id]),
        pinnedIds: new Set([ws.id]),
      }),
    );
    expect(ctx.openFiles).toEqual([{ path: "src/a.ts", active: true, dirty: true, pinned: true }]);
  });

  it("reports the active workspace file's path", () => {
    const ws = workspaceFile("src/a.ts");
    const ctx = buildEditorContext(input({ openFiles: [ws], activeFileId: ws.id }));
    expect(ctx.activeFile).toBe("src/a.ts");
  });

  it("reports no active file when the active tab is a diff", () => {
    const ws = workspaceFile("src/a.ts");
    const diff = diffFile("src/a.ts", "workingToHead");
    const ctx = buildEditorContext(
      input({ openFiles: [ws, diff], activeFileId: diff.id, live: live() }),
    );
    expect(ctx.activeFile).toBeNull();
    // and the live position is dropped, so a diff never carries a cursor
    expect(ctx.cursor).toBeNull();
    expect(ctx.viewport).toBeNull();
    expect(ctx.selection).toBeNull();
  });

  it("reports no active file when the active tab is secrets", () => {
    const secrets = secretsFile("Proj");
    const ctx = buildEditorContext(
      input({ openFiles: [secrets], activeFileId: secrets.id, live: live() }),
    );
    expect(ctx.activeFile).toBeNull();
    expect(ctx.cursor).toBeNull();
  });

  it("attaches the live position when the active tab is a workspace file", () => {
    const ws = workspaceFile("src/a.ts");
    const sel = selection();
    const ctx = buildEditorContext(
      input({ openFiles: [ws], activeFileId: ws.id, live: live({ selection: sel }) }),
    );
    expect(ctx.cursor).toEqual({ line: 3, character: 5 });
    expect(ctx.viewport).toEqual({ firstVisibleLine: 1, lastVisibleLine: 40 });
    expect(ctx.selection).toEqual(sel);
  });

  it("normalises backslash paths to forward slashes", () => {
    const ws = workspaceFile("src\\nested\\a.ts");
    const ctx = buildEditorContext(
      input({ openFiles: [ws], activeFileId: ws.id, recent: ["src\\nested\\a.ts"] }),
    );
    expect(ctx.activeFile).toBe("src/nested/a.ts");
    expect(ctx.openFiles[0]?.path).toBe("src/nested/a.ts");
    expect(ctx.recentFiles[0]?.path).toBe("src/nested/a.ts");
  });

  it("maps the recency list to recentFiles in order", () => {
    const ctx = buildEditorContext(input({ recent: ["a.ts", "b.ts"] }));
    expect(ctx.recentFiles).toEqual([{ path: "a.ts" }, { path: "b.ts" }]);
  });

  it("produces an all-empty context when nothing workspace-shaped is open", () => {
    const ctx = buildEditorContext(input({ openFiles: [secretsFile("Proj")], live: live() }));
    expect(ctx).toEqual({
      activeFile: null,
      cursor: null,
      viewport: null,
      selection: null,
      openFiles: [],
      recentFiles: [],
    });
  });
});

describe("editorContextEqual", () => {
  const base = (): EditorContext => ({
    activeFile: "src/a.ts",
    cursor: { line: 1, character: 0 },
    viewport: { firstVisibleLine: 1, lastVisibleLine: 10 },
    selection: selection(),
    openFiles: [{ path: "src/a.ts", active: true, dirty: false, pinned: false }],
    recentFiles: [{ path: "src/a.ts" }],
  });

  it("is true for structurally identical, distinct objects", () => {
    expect(editorContextEqual(base(), base())).toBe(true);
  });

  it("notices a cursor move", () => {
    const b = base();
    b.cursor = { line: 2, character: 0 };
    expect(editorContextEqual(base(), b)).toBe(false);
  });

  it("notices a selection text change", () => {
    const b = base();
    b.selection = selection({ text: "other" });
    expect(editorContextEqual(base(), b)).toBe(false);
  });

  it("notices a null vs present cursor", () => {
    const b = base();
    b.cursor = null;
    expect(editorContextEqual(base(), b)).toBe(false);
  });

  it("notices a dirty flag flip on an open file", () => {
    const b = base();
    b.openFiles = [{ path: "src/a.ts", active: true, dirty: true, pinned: false }];
    expect(editorContextEqual(base(), b)).toBe(false);
  });

  it("notices a change in the open-file count", () => {
    const b = base();
    b.openFiles = [...b.openFiles, { path: "src/b.ts", active: false, dirty: false, pinned: false }];
    expect(editorContextEqual(base(), b)).toBe(false);
  });

  it("notices a recency reorder", () => {
    const a = base();
    a.recentFiles = [{ path: "a.ts" }, { path: "b.ts" }];
    const b = base();
    b.recentFiles = [{ path: "b.ts" }, { path: "a.ts" }];
    expect(editorContextEqual(a, b)).toBe(false);
  });
});
