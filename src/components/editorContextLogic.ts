// Decision logic for the **editor context** pushed to the backend, where the
// per-workspace Editor Context MCP shim reads it back for an agent.
//
// Pure so every rule below is testable in vitest's node environment (no DOM):
// `FileEditor` and `RunView` are rendering shells that gather the raw inputs and
// call these functions. Nothing here touches CodeMirror, an `EditorView` or the
// backend.
//
// This is the frontend half of the privacy gate. The feature-off gate is TWO
// things — `RunView` stops calling `api.setEditorContext` while the feature is
// off, AND the pipe host re-checks the feature every call (see
// `.memories/features/editor-context-mcp/notes.md`) — but the *shaping* of what
// is exposed lives here: only workspace-source files contribute a path, the
// selection text is capped, and identical context is not re-pushed.
//
// ## Position convention
//
// 1-based line, 0-based UTF-16 character — the app-wide rule (see
// `usagesExtension.ts` and `ipc/types.ts`). The caller hands us numbers already
// in those units (`Line.number`, `pos - line.from`); this module converts
// nothing.

import type {
  EditorContext,
  EditorCursor,
  EditorOpenFile,
  EditorSelection,
  EditorViewport,
} from "../ipc/types";
import type { OpenEditorFile } from "./editorSourceLogic";

/**
 * The most code points of a selection's text that cross to the agent (~4 KiB).
 *
 * Capped at the source so a whole-file selection does not push megabytes through
 * the frontend, the IPC boundary and an agent's transcript on every keystroke.
 * The cap is a decision, so it lives here where a test pins it; `FileEditor`
 * calls {@link capSelectionText} rather than slicing the string itself.
 */
export const SELECTION_TEXT_CAP = 4096;

/**
 * How many recently edited files to remember (most-recent-first).
 *
 * A bound, not a promise of exactly this many: a short session simply has fewer.
 */
export const RECENT_FILES_CAP = 20;

/**
 * The active editor's live position, as `FileEditor` reads it off CodeMirror.
 *
 * Each field is nullable because each can genuinely be absent: an editor with no
 * measured viewport yet, or a caret with no selection. `null` throughout is the
 * honest "no active editor is reporting", which {@link buildEditorContext} maps
 * to the context's own null fields.
 */
export interface LiveEditorPosition {
  cursor: EditorCursor | null;
  viewport: EditorViewport | null;
  /** A non-empty selection only; an empty selection is `null`, never a zero-width range. */
  selection: EditorSelection | null;
}

/** Everything {@link buildEditorContext} needs, gathered by `RunView`. */
export interface EditorContextInput {
  /** Every open tab, including diff and secrets tabs — filtered here. */
  openFiles: readonly OpenEditorFile[];
  /** The active tab's id (a path, `secrets:…` or `diff:…`), or null for none. */
  activeFileId: string | null;
  /** Ids (not paths) of dirty tabs. */
  dirtyIds: ReadonlySet<string>;
  /** Ids (not paths) of pinned tabs. */
  pinnedIds: ReadonlySet<string>;
  /** Recently edited workspace paths, most-recent-first (see {@link pushRecent}). */
  recent: readonly string[];
  /**
   * The active editor's live position, or null.
   *
   * Only meaningful when the active tab is a workspace file: `RunView` passes it
   * as null otherwise, and {@link buildEditorContext} drops it anyway when the
   * active tab contributes no path, so a diff or secrets tab never carries a
   * cursor.
   */
  live: LiveEditorPosition | null;
}

/**
 * Truncate selection text to the cap, without splitting a surrogate pair.
 *
 * `Array.from` iterates code points, so slicing it can never cut a surrogate
 * pair in half (which would render as a replacement glyph). A string at or under
 * the cap is returned unchanged.
 */
export function capSelectionText(text: string, cap: number = SELECTION_TEXT_CAP): string {
  // The fast path avoids the code-point walk for the overwhelmingly common short
  // selection: `String.length` (UTF-16 code units) is >= the code-point count,
  // so if it is within the cap the text certainly is.
  if (text.length <= cap) return text;
  const points = Array.from(text);
  if (points.length <= cap) return text;
  return points.slice(0, cap).join("");
}

/**
 * Add `path` to the front of the recency list, deduped and capped.
 *
 * Identity is the path: a file edited again moves to the front rather than
 * appearing twice. Returns the **same array reference** when `path` is already
 * at the front, so a caller storing this in state does not re-render on an edit
 * to the file that is already most-recent.
 */
export function pushRecent(
  recent: readonly string[],
  path: string,
  cap: number = RECENT_FILES_CAP,
): string[] {
  if (recent[0] === path) return recent as string[];
  const next = [path, ...recent.filter((p) => p !== path)];
  return next.length > cap ? next.slice(0, cap) : next;
}

/** Normalise a path to forward slashes, the wire convention for every path here. */
function normalizePath(path: string): string {
  return path.replace(/\\/g, "/");
}

/**
 * Assemble the `EditorContext` to push from the gathered inputs.
 *
 * Only **workspace-source** tabs contribute a path — a diff tab's "path" is a
 * revision comparison, not a file the agent should be told the user is editing,
 * and a secrets tab's id is no path at all. So `openFiles`, `activeFile` and the
 * live position are all derived from workspace tabs only.
 *
 * The live cursor/selection/viewport are attached **only when the active tab is
 * a workspace file** (`activeFile !== null`). This is the belt to `RunView`'s
 * braces: it already passes `live` as null for a non-workspace active tab, and
 * this drops a stale position even if it did not.
 */
export function buildEditorContext(input: EditorContextInput): EditorContext {
  const openFiles: EditorOpenFile[] = [];
  let activeFile: string | null = null;
  for (const file of input.openFiles) {
    if (file.source.kind !== "workspace") continue;
    const path = normalizePath(file.source.path);
    const active = file.id === input.activeFileId;
    openFiles.push({
      path,
      active,
      dirty: input.dirtyIds.has(file.id),
      pinned: input.pinnedIds.has(file.id),
    });
    if (active) activeFile = path;
  }

  const live = activeFile !== null ? input.live : null;

  return {
    activeFile,
    cursor: live?.cursor ?? null,
    viewport: live?.viewport ?? null,
    selection: live?.selection ?? null,
    openFiles,
    recentFiles: input.recent.map((path) => ({ path: normalizePath(path) })),
  };
}

// ---------------------------------------------------------------------------
// Equality — so identical context is not re-pushed.
// ---------------------------------------------------------------------------

function cursorEqual(a: EditorCursor | null, b: EditorCursor | null): boolean {
  if (a === null || b === null) return a === b;
  return a.line === b.line && a.character === b.character;
}

function viewportEqual(a: EditorViewport | null, b: EditorViewport | null): boolean {
  if (a === null || b === null) return a === b;
  return a.firstVisibleLine === b.firstVisibleLine && a.lastVisibleLine === b.lastVisibleLine;
}

function selectionEqual(a: EditorSelection | null, b: EditorSelection | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.startLine === b.startLine &&
    a.startCharacter === b.startCharacter &&
    a.endLine === b.endLine &&
    a.endCharacter === b.endCharacter &&
    a.text === b.text
  );
}

/**
 * Whether two contexts are the same in every field the agent would read.
 *
 * A structural compare rather than reference identity, because
 * {@link buildEditorContext} mints a fresh object every render: without this the
 * debounced push would fire on every unrelated re-render. The comparison walks
 * `openFiles` and `recentFiles` in order — order is meaningful (the recency list
 * is most-recent-first, and `active`/`dirty`/`pinned` ride each open row), so a
 * reorder is a real change and is reported as one.
 */
export function editorContextEqual(a: EditorContext, b: EditorContext): boolean {
  if (a.activeFile !== b.activeFile) return false;
  if (!cursorEqual(a.cursor, b.cursor)) return false;
  if (!viewportEqual(a.viewport, b.viewport)) return false;
  if (!selectionEqual(a.selection, b.selection)) return false;
  if (a.openFiles.length !== b.openFiles.length) return false;
  for (let i = 0; i < a.openFiles.length; i += 1) {
    const x = a.openFiles[i];
    const y = b.openFiles[i];
    if (!x || !y) return false;
    if (
      x.path !== y.path ||
      x.active !== y.active ||
      x.dirty !== y.dirty ||
      x.pinned !== y.pinned
    ) {
      return false;
    }
  }
  if (a.recentFiles.length !== b.recentFiles.length) return false;
  for (let i = 0; i < a.recentFiles.length; i += 1) {
    if (a.recentFiles[i]?.path !== b.recentFiles[i]?.path) return false;
  }
  return true;
}
