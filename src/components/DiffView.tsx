import { useEffect, useMemo, useRef, useState } from "react";
import { EditorState, StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap, lineNumbers, type DecorationSet } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { unifiedMergeView } from "@codemirror/merge";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { xml } from "@codemirror/lang-xml";
import { cpp } from "@codemirror/lang-cpp";
import type { FileDiff } from "../ipc/types";

/** Highlight for lines the user has picked for revert or staging. */
const setSelectedLines = StateEffect.define<Set<number>>();

const selectedLineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const effect of tr.effects) {
      if (!effect.is(setSelectedLines)) continue;

      const marks = [];
      for (const lineNumber of [...effect.value].sort((a, b) => a - b)) {
        if (lineNumber < 1 || lineNumber > tr.state.doc.lines) continue;
        const line = tr.state.doc.line(lineNumber);
        marks.push(
          Decoration.line({ class: "cb-line-selected" }).range(line.from),
        );
      }
      return Decoration.set(marks);
    }
    return tr.docChanged ? value.map(tr.changes) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/**
 * Pick a language mode from the file extension.
 *
 * Syntax highlighting is cosmetic here, so an unknown extension falls back to
 * no mode rather than guessing.
 */
function languageFor(path: string): Extension[] {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";

  switch (extension) {
    case "ts":
    case "tsx":
      return [javascript({ typescript: true, jsx: extension === "tsx" })];
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
      return [javascript({ jsx: extension === "jsx" })];
    case "json":
      return [json()];
    case "css":
    case "scss":
      return [css()];
    case "html":
      return [html()];
    case "py":
      return [python()];
    case "rs":
      return [rust()];
    case "xml":
    case "csproj":
    case "fsproj":
    case "props":
    case "targets":
      return [xml()];
    case "cs":
    case "c":
    case "h":
    case "cpp":
    case "hpp":
      // No dedicated C# mode ships with CodeMirror; the C-family one is a
      // close enough approximation for reviewing a diff.
      return [cpp()];
    default:
      return [];
  }
}

export interface DiffViewProps {
  path: string;
  /** The state being compared against. `null` for a new file. */
  baseline: string | null;
  /** The file as it exists on disk. */
  working: string;
  /** Structured diff, used to map editor lines back to diff line indices. */
  diff: FileDiff;
  editable: boolean;
  /** Called when the user saves an edit made in place. */
  onSave: (content: string) => void;
  /** Called with the diff line indices the user selected. */
  onSelectionChange: (indices: number[]) => void;
}

/**
 * A unified diff the user can edit and revert within.
 *
 * `@codemirror/merge`'s `unifiedMergeView` supplies the diff rendering and
 * per-chunk accept/reject controls. Line selection is layered on top: the
 * editor works in document line numbers, while every revert and staging
 * operation is expressed in the diff's own line indices, so the two have to be
 * mapped onto each other.
 */
export function DiffView({
  path,
  baseline,
  working,
  diff,
  editable,
  onSave,
  onSelectionChange,
}: DiffViewProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  // Callbacks are read through a ref so changing them does not tear down and
  // rebuild the editor, which would lose scroll position and the cursor.
  const handlers = useRef({ onSave, onSelectionChange });
  handlers.current = { onSave, onSelectionChange };

  /**
   * Map each working-copy line number to the diff line index that produced it.
   *
   * Only added and context lines exist in the working copy; deletions do not,
   * which is why reverting a deleted line has to be done from the chunk
   * controls rather than by clicking a line.
   */
  const lineToDiffIndex = useMemo(() => {
    const map = new Map<number, number>();
    for (const hunk of diff.hunks) {
      for (const line of hunk.lines) {
        if (line.newLineno != null) {
          map.set(line.newLineno, line.index);
        }
      }
    }
    return map;
  }, [diff]);

  /**
   * Deletions attached to each working-copy line, so selecting a modified line
   * also selects the original it replaced. Without this, reverting a changed
   * line would delete it instead of restoring its previous content.
   */
  const adjacentDeletions = useMemo(() => {
    const map = new Map<number, number[]>();

    for (const hunk of diff.hunks) {
      let pendingDeletions: number[] = [];
      let lastKnownLine = hunk.newStart;

      for (const line of hunk.lines) {
        if (line.origin === "deletion") {
          pendingDeletions.push(line.index);
          continue;
        }
        if (line.newLineno != null) {
          lastKnownLine = line.newLineno;
          if (pendingDeletions.length > 0 && line.origin === "addition") {
            map.set(line.newLineno, pendingDeletions);
            pendingDeletions = [];
          } else if (pendingDeletions.length > 0) {
            // Deletions with no replacement attach to the preceding line.
            map.set(lastKnownLine, pendingDeletions);
            pendingDeletions = [];
          }
        }
      }
      if (pendingDeletions.length > 0) {
        map.set(lastKnownLine, pendingDeletions);
      }
    }
    return map;
  }, [diff]);

  // Build the editor. Rebuilt when the file or its baseline changes.
  useEffect(() => {
    if (!hostRef.current) return;

    const extensions: Extension[] = [
      lineNumbers(),
      history(),
      keymap.of([
        {
          key: "Mod-s",
          run: (view) => {
            handlers.current.onSave(view.state.doc.toString());
            return true;
          },
        },
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      selectedLineField,
      EditorView.editable.of(editable),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
      }),
      ...languageFor(path),
    ];

    // A file with no committed baseline has nothing to diff against, so it is
    // shown as a plain editor rather than an all-green diff.
    if (baseline != null) {
      extensions.push(
        unifiedMergeView({
          original: baseline,
          mergeControls: true,
          highlightChanges: true,
          gutter: true,
        }),
      );
    }

    const view = new EditorView({
      state: EditorState.create({ doc: working, extensions }),
      parent: hostRef.current,
    });
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, [path, baseline, working, editable]);

  // Push the highlight into the editor whenever the selection changes.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;

    const lines = new Set<number>();
    for (const [lineNumber, index] of lineToDiffIndex) {
      if (selected.has(index)) lines.add(lineNumber);
    }
    view.dispatch({ effects: setSelectedLines.of(lines) });
  }, [selected, lineToDiffIndex]);

  useEffect(() => {
    handlers.current.onSelectionChange([...selected]);
  }, [selected]);

  // Clicking the gutter toggles a line's selection.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const onClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      const gutterElement = target.closest(".cm-gutterElement");
      if (!gutterElement) return;

      const view = viewRef.current;
      if (!view) return;

      const position = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (position == null) return;

      const lineNumber = view.state.doc.lineAt(position).number;
      const index = lineToDiffIndex.get(lineNumber);
      if (index == null) return;

      setSelected((previous) => {
        const next = new Set(previous);
        const related = [index, ...(adjacentDeletions.get(lineNumber) ?? [])];

        // Toggle the line and whatever it replaced as one unit.
        if (next.has(index)) {
          for (const value of related) next.delete(value);
        } else {
          for (const value of related) next.add(value);
        }
        return next;
      });
    };

    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  }, [lineToDiffIndex, adjacentDeletions]);

  // A new file or a new diff invalidates any previous selection.
  useEffect(() => setSelected(new Set()), [path, diff]);

  return <div className="diff-host" ref={hostRef} />;
}

/** Every changed line index in a diff, for "select all". */
export function allChangedIndices(diff: FileDiff): number[] {
  return diff.hunks
    .flatMap((hunk) => hunk.lines)
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}

/** Changed line indices belonging to one hunk. */
export function hunkIndices(diff: FileDiff, hunk: number): number[] {
  return (diff.hunks[hunk]?.lines ?? [])
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}
