import { useEffect, useMemo, useRef, useState } from "react";
import { EditorState, StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap, lineNumbers, type DecorationSet } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { MergeView, unifiedMergeView } from "@codemirror/merge";
import { editorColors, languageFor } from "./language";
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

/** How to render the comparison. */
export type DiffLayout = "inline" | "sideBySide";

export interface DiffViewProps {
  path: string;
  /** The state being compared against. `null` for a new file. */
  baseline: string | null;
  /** The file as it exists on disk. */
  working: string;
  /** Structured diff, used to map editor lines back to diff line indices. */
  diff: FileDiff;
  layout: DiffLayout;
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
  layout,
  editable,
  onSave,
  onSelectionChange,
}: DiffViewProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editorError, setEditorError] = useState<string | null>(null);

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

    // Fill-the-pane sizing is only correct for a single editor. The
    // side-by-side MergeView positions its revert buttons and alignment
    // spacers in *document* coordinates, so its editors must auto-size and
    // scroll via the outer .diff-host instead.
    const heightTheme = EditorView.theme({
      "&": { height: "100%" },
      ".cm-scroller": { overflow: "auto" },
    });

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
      ...languageFor(path),
      ...editorColors,
    ];

    // A CodeMirror failure must degrade to a message for this one file, not
    // take down the whole UI (an effect error unmounts the React tree).
    try {
      // A file with no committed baseline has nothing to diff against, so it
      // is shown as a plain editor rather than an all-green diff.
      if (baseline != null && layout === "sideBySide") {
        const merge = new MergeView({
          a: {
            doc: baseline,
            extensions: [
              lineNumbers(),
              EditorView.editable.of(false),
              ...languageFor(path),
              ...editorColors,
            ],
          },
          b: { doc: working, extensions },
          parent: hostRef.current,
          // The buttons copy a chunk from the baseline onto the working copy —
          // the side-by-side equivalent of the unified view's revert control.
          // (There is no "accept": keeping the working copy is a no-op.)
          revertControls: editable ? "a-to-b" : undefined,
          // The library's default control is a bare "⇜" glyph, invisible on a
          // dark theme. The library positions the element and handles clicks.
          renderRevertControl: () => {
            const button = document.createElement("button");
            button.textContent = "↶";
            button.title = "Revert this chunk to the baseline";
            button.setAttribute("aria-label", "Revert this chunk");
            return button;
          },
          highlightChanges: true,
          gutter: true,
        });
        setEditorError(null);
        viewRef.current = merge.b;
        return () => {
          merge.destroy();
          viewRef.current = null;
        };
      }

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
      extensions.push(heightTheme);

      const view = new EditorView({
        state: EditorState.create({ doc: working, extensions }),
        parent: hostRef.current,
      });
      setEditorError(null);
      viewRef.current = view;
      return () => {
        view.destroy();
        viewRef.current = null;
      };
    } catch (e) {
      setEditorError(e instanceof Error ? `${e.message}\n${e.stack ?? ""}` : String(e));
      return;
    }
  }, [path, baseline, working, layout, editable]);

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

  if (editorError) {
    return (
      <div className="error" style={{ whiteSpace: "pre-wrap" }}>
        The diff editor failed to open {path}:{"\n"}
        {editorError}
      </div>
    );
  }

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
