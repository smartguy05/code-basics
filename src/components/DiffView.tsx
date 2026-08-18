import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { EditorState, StateEffect, StateField, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  hoverTooltip,
  keymap,
  lineNumbers,
  type DecorationSet,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Change, MergeView, presentableDiff, unifiedMergeView } from "@codemirror/merge";
import { editorColors, languageFor } from "./language";
import {
  changeMarks,
  diffSplitFraction,
  loadDiffSplit,
  mapOffset,
  nextChangeLine,
  normaliseEndings,
  normaliseWhitespace,
  saveDiffSplit,
  scrollLeftForThumb,
  scrollThumb,
  type ScrollMetrics,
} from "./diffLogic";
import { onEditorFontSizeChange } from "../editorFontSize";
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

/**
 * Diff both sides with whitespace ignored, reporting the result in the *real*
 * documents' coordinates.
 *
 * `@codemirror/merge` has no ignore-whitespace option, but it will take a diff
 * function (`DiffConfig.override`). So the comparison runs over normalised
 * copies and every offset is mapped back, which is what lets the highlight land
 * on the actual text while the decision was made about the stripped version.
 *
 * This is a **display** filter and nothing more. Staging and reverting act on
 * the exact `FileDiff` from Rust, because a whitespace-only hunk this hides is
 * still a real change on disk.
 */
function whitespaceInsensitiveDiff(a: string, b: string): readonly Change[] {
  const left = normaliseWhitespace(a);
  const right = normaliseWhitespace(b);

  return presentableDiff(left.text, right.text).map(
    (change) =>
      new Change(
        mapOffset(left.map, change.fromA),
        mapOffset(left.map, change.toA),
        mapOffset(right.map, change.fromB),
        mapOffset(right.map, change.toB),
      ),
  );
}

/** What a parent view can ask the diff to do. */
export interface DiffViewHandle {
  /** Jump to the next (`1`) or previous (`-1`) change, wrapping at the ends. */
  goToChange: (direction: 1 | -1) => void;
}

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
  /**
   * Fold away long stretches of unchanged code, leaving a few lines of context
   * around each change. Off by default: a review of a small change reads
   * faster folded, but a review that needs the surrounding code does not.
   */
  collapseUnchanged?: boolean;
  /**
   * Compare with whitespace ignored. A **display** filter: staging and
   * reverting still act on the exact diff, so a hunk hidden here is still a
   * real change on disk.
   */
  ignoreWhitespace?: boolean;
  /** Called when the user saves an edit made in place. */
  onSave: (content: string) => void;
  /** Called with the diff line indices the user selected. */
  onSelectionChange: (indices: number[]) => void;
  /**
   * Diff line indices to select on open, so choosing an intent card lands on
   * its lines already highlighted. The user can still change the selection
   * afterwards; this only seeds it.
   */
  highlight?: number[];
  /** Imperative handle for the toolbar's and F7's jump-to-change. */
  handleRef?: RefObject<DiffViewHandle | null>;
  /**
   * The recorded "why" for a 1-based line of the working document, shown as a
   * hover tooltip. `null` for a line with no recorded reason (no tooltip). The
   * History tab uses this to surface durable intent git-blame style.
   */
  lineWhy?: (line: number) => string | null;
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
  collapseUnchanged = false,
  ignoreWhitespace = false,
  onSave,
  onSelectionChange,
  highlight,
  handleRef,
  lineWhy,
}: DiffViewProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  /**
   * Every live editor: one in the inline layout, both panes side by side.
   *
   * `viewRef` above stays the *working copy* — the one selection and saving act
   * on — but scrolling, re-measuring and jumping to a change have to reach both
   * sides, and the merge view's baseline pane is otherwise unreachable once the
   * effect has returned.
   */
  const panesRef = useRef<EditorView[]>([]);
  /**
   * The baseline pane's width fraction in the side-by-side layout.
   *
   * Held in a ref and applied straight to the library's flex wrappers so a drag
   * resizes the panes without rebuilding the editors — a rebuild would lose the
   * scroll position and the cursor, exactly as the callback comment above notes.
   */
  const splitRef = useRef(loadDiffSplit(localStorage));
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editorError, setEditorError] = useState<string | null>(null);
  /** The scrollbar's track, measured for the thumb arithmetic. */
  const trackRef = useRef<HTMLDivElement>(null);
  /** Where the thumb is dragged from, or `null` when nothing is being dragged. */
  const dragRef = useRef<{ pointerX: number; thumbLeft: number } | null>(null);
  const [metrics, setMetrics] = useState<ScrollMetrics>({
    contentWidth: 0,
    viewportWidth: 0,
    scrollLeft: 0,
    trackWidth: 0,
  });
  /** The working document's line count, the scale the marker strip is drawn on. */
  const [docLines, setDocLines] = useState(1);

  // Callbacks are read through a ref so changing them does not tear down and
  // rebuild the editor, which would lose scroll position and the cursor.
  const handlers = useRef({ onSave, onSelectionChange, lineWhy });
  handlers.current = { onSave, onSelectionChange, lineWhy };

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
      // `@codemirror/merge` ships both a light and a dark palette and picks
      // between them with CodeMirror's `&dark` selector, which only matches when
      // a theme declares itself dark. Nothing in this app ever did, so the
      // light-theme diff colours were being painted onto a dark background.
      EditorView.darkTheme.of(true),
      history(),
      // Ctrl+F searches within the diff (and, on the read-only baseline pane
      // below, within that side too).
      search({ top: true }),
      highlightSelectionMatches(),
      keymap.of([
        {
          key: "Mod-s",
          run: (view) => {
            handlers.current.onSave(view.state.doc.toString());
            return true;
          },
        },
        ...searchKeymap,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      selectedLineField,
      EditorView.editable.of(editable),
      // Hover a line to see its recorded intent, when a resolver is supplied
      // (the History tab). Reads through the handlers ref so it always sees the
      // latest data without rebuilding the editor.
      hoverTooltip((view, pos) => {
        const resolve = handlers.current.lineWhy;
        if (!resolve) return null;
        const line = view.state.doc.lineAt(pos).number;
        const text = resolve(line);
        if (!text) return null;
        return {
          pos,
          above: true,
          create() {
            const dom = document.createElement("div");
            dom.className = "cm-why-tooltip";
            dom.textContent = text;
            return { dom };
          },
        };
      }),
      ...languageFor(path),
      ...editorColors,
    ];

    // `margin` is the lines left visible either side of a change; `minSize` is
    // how long an unchanged run has to be before folding it earns its keep.
    const collapse = collapseUnchanged ? { margin: 3, minSize: 8 } : undefined;
    const diffConfig = ignoreWhitespace ? { override: whitespaceInsensitiveDiff } : undefined;

    // The baseline comes from git (always `\n`); the working copy comes from
    // disk (`\r\n` on Windows). `@codemirror/merge` diffs the two raw strings,
    // so an ending mismatch alone would mark every line changed. Both sides are
    // brought to `\n` for the comparison — git filters the same difference.
    const baselineDoc = baseline == null ? null : normaliseEndings(baseline);
    const workingDoc = normaliseEndings(working);

    // A CodeMirror failure must degrade to a message for this one file, not
    // take down the whole UI (an effect error unmounts the React tree).
    try {
      // A file with no committed baseline has nothing to diff against, so it
      // is shown as a plain editor rather than an all-green diff.
      if (baselineDoc != null && layout === "sideBySide") {
        const merge = new MergeView({
          a: {
            doc: baselineDoc,
            extensions: [
              lineNumbers(),
              EditorView.darkTheme.of(true),
              EditorView.editable.of(false),
              search({ top: true }),
              highlightSelectionMatches(),
              keymap.of(searchKeymap),
              ...languageFor(path),
              ...editorColors,
            ],
          },
          b: { doc: workingDoc, extensions },
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
          collapseUnchanged: collapse,
          diffConfig,
        });
        setEditorError(null);
        viewRef.current = merge.b;
        panesRef.current = [merge.a, merge.b];

        // Restore the remembered divider position on the fresh panes, and drop a
        // draggable handle at their seam. The handle lives inside the library's
        // own flex row so it stays at the boundary as the ratio changes;
        // `merge.destroy()` takes the whole subtree — handle included — with it.
        applyPaneSplit(splitRef.current);
        const editors = merge.a.dom.closest<HTMLElement>(".cm-mergeViewEditors");
        const wrapB = merge.b.dom.closest<HTMLElement>(".cm-mergeViewEditor");
        if (editors && wrapB) {
          const handle = document.createElement("div");
          handle.className = "diff-pane-resizer";
          handle.title = "Drag to resize the panes";
          handle.addEventListener("mousedown", startPaneDrag);
          editors.insertBefore(handle, wrapB);
        }

        return () => {
          merge.destroy();
          viewRef.current = null;
          panesRef.current = [];
        };
      }

      if (baselineDoc != null) {
        extensions.push(
          unifiedMergeView({
            original: baselineDoc,
            mergeControls: true,
            highlightChanges: true,
            gutter: true,
            collapseUnchanged: collapse,
            diffConfig,
          }),
        );
      }
      extensions.push(heightTheme);

      const view = new EditorView({
        state: EditorState.create({ doc: workingDoc, extensions }),
        parent: hostRef.current,
      });
      setEditorError(null);
      viewRef.current = view;
      panesRef.current = [view];
      return () => {
        view.destroy();
        viewRef.current = null;
        panesRef.current = [];
      };
    } catch (e) {
      setEditorError(e instanceof Error ? `${e.message}\n${e.stack ?? ""}` : String(e));
      return;
    }
  }, [path, baseline, working, layout, editable, collapseUnchanged, ignoreWhitespace]);

  /**
   * Re-read what the scrollbar and the marker strip describe.
   *
   * Both are drawn from the editors' current geometry rather than from React
   * state the editors do not have, so anything that can change that geometry —
   * a rebuild, a resize, a font-size change, scrolling — calls this.
   */
  const measure = useCallback(() => {
    const panes = panesRef.current;
    if (panes.length === 0) return;

    const scrollers = panes.map((view) => view.scrollDOM);
    const first = scrollers.at(0);
    if (!first) return;

    setMetrics({
      // Widest content across the panes against narrowest viewport: the bar is
      // shared, so it has to be able to reach the far end of either side.
      contentWidth: Math.max(...scrollers.map((el) => el.scrollWidth)),
      viewportWidth: Math.min(...scrollers.map((el) => el.clientWidth)),
      scrollLeft: first.scrollLeft,
      trackWidth: trackRef.current?.clientWidth ?? 0,
    });
    setDocLines(viewRef.current?.state.doc.lines ?? 1);
  }, []);

  /** Scroll every pane to the same horizontal offset. */
  const applyScrollLeft = useCallback((next: number) => {
    const clamped = Math.max(0, Math.round(next));
    for (const view of panesRef.current) {
      // Guarded: assigning an unchanged value still fires `scroll` in some
      // engines, and the scroll handler calls back into here.
      if (view.scrollDOM.scrollLeft !== clamped) view.scrollDOM.scrollLeft = clamped;
    }
    setMetrics((previous) => ({ ...previous, scrollLeft: clamped }));
  }, []);

  /**
   * The two side-by-side pane wrappers the merge view lays out, or null in the
   * inline layout (one editor, nothing to divide).
   */
  function paneWrappers(): [HTMLElement, HTMLElement] | null {
    const [a, b] = panesRef.current;
    const wrapA = a?.dom.closest<HTMLElement>(".cm-mergeViewEditor");
    const wrapB = b?.dom.closest<HTMLElement>(".cm-mergeViewEditor");
    return wrapA && wrapB && wrapA !== wrapB ? [wrapA, wrapB] : null;
  }

  /**
   * Size the panes to `frac : 1 - frac`.
   *
   * The library gives both wrappers `flex-grow: 1; flex-basis: 0`, so overriding
   * only `flex-grow` splits the flexible width in that ratio and leaves the
   * revert column between them untouched.
   */
  function applyPaneSplit(frac: number) {
    const wrappers = paneWrappers();
    if (!wrappers) return;
    wrappers[0].style.flexGrow = String(frac);
    wrappers[1].style.flexGrow = String(1 - frac);
  }

  /** Drag the divider between the two panes; the fraction persists across files. */
  function startPaneDrag(event: MouseEvent) {
    event.preventDefault();
    const editors = panesRef.current[0]?.dom.closest<HTMLElement>(".cm-mergeViewEditors");
    if (!editors) return;
    const { left, width } = editors.getBoundingClientRect();

    const onMove = (move: MouseEvent) => {
      splitRef.current = diffSplitFraction(move.clientX, left, width);
      applyPaneSplit(splitRef.current);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      saveDiffSplit(localStorage, splitRef.current);
      // The panes changed width, so the shared scrollbar's arithmetic is stale.
      requestAnimationFrame(measure);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  // CodeMirror caches character metrics, so a font size applied through CSS
  // leaves every open editor laying out at the old one until it is told to
  // measure again. The widths the scrollbar reads change with it.
  useEffect(
    () =>
      onEditorFontSizeChange(() => {
        for (const view of panesRef.current) view.requestMeasure();
        requestAnimationFrame(measure);
      }),
    [measure],
  );

  /**
   * Keep the two panes' horizontal offsets equal however they were moved.
   *
   * CodeMirror scrolls a pane by itself when the cursor leaves the viewport, so
   * this cannot only listen to the app's own scrollbar — without it the panes
   * drift apart and the side-by-side comparison stops lining up, which was the
   * whole point of the request.
   */
  useEffect(() => {
    const panes = panesRef.current;
    if (panes.length === 0) return;

    const onScroll = (event: Event) => {
      const source = event.target as HTMLElement;
      applyScrollLeft(source.scrollLeft);
    };

    for (const view of panes) view.scrollDOM.addEventListener("scroll", onScroll);
    return () => {
      for (const view of panes) view.scrollDOM.removeEventListener("scroll", onScroll);
    };
    // Re-bound whenever the editors are rebuilt, which replaces every scroller.
  }, [applyScrollLeft, path, baseline, working, layout, editable]);

  // A freshly built editor has no layout yet, so the widths are only readable
  // on the frame after it was created.
  useEffect(() => {
    const frame = requestAnimationFrame(measure);
    return () => cancelAnimationFrame(frame);
  }, [measure, path, baseline, working, layout, editable, diff]);

  // The panes' widths follow the window and the sidebar splitter.
  useEffect(() => {
    const host = hostRef.current;
    if (!host || typeof ResizeObserver !== "function") return;

    const observer = new ResizeObserver(() => measure());
    observer.observe(host);
    return () => observer.disconnect();
  }, [measure]);

  /** Put a document line in view in every pane. */
  const revealLine = useCallback((lineNumber: number) => {
    for (const view of panesRef.current) {
      const clamped = Math.min(Math.max(lineNumber, 1), view.state.doc.lines);
      const line = view.state.doc.line(clamped);
      view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: "center" }) });
    }
    // Only the working copy takes the cursor: moving it in the read-only
    // baseline would put a caret in a pane that cannot be edited, and the next
    // jump reads its position from here.
    const working = viewRef.current;
    if (!working) return;
    const clamped = Math.min(Math.max(lineNumber, 1), working.state.doc.lines);
    working.dispatch({ selection: { anchor: working.state.doc.line(clamped).from } });
  }, []);

  useImperativeHandle(
    handleRef,
    () => ({
      goToChange: (direction: 1 | -1) => {
        const view = viewRef.current;
        if (!view) return;

        const from = view.state.doc.lineAt(view.state.selection.main.head).number;
        const target = nextChangeLine(diff, from, direction);
        if (target !== null) revealLine(target);
      },
    }),
    [diff, revealLine],
  );

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

  // A new file or a new diff invalidates any previous selection. When the
  // caller supplied lines to start from — opening an intent card — those seed
  // it instead of starting empty.
  //
  // Keyed on the joined indices rather than the array, so a caller that
  // rebuilds an equal array on every render does not reset the user's own
  // selection underneath them.
  const highlightKey = highlight?.join(",") ?? "";
  useEffect(() => {
    setSelected(new Set(highlightKey === "" ? [] : highlightKey.split(",").map(Number)));
  }, [path, diff, highlightKey]);

  if (editorError) {
    return (
      <div className="error" style={{ whiteSpace: "pre-wrap" }}>
        The diff editor failed to open {path}:{"\n"}
        {editorError}
      </div>
    );
  }

  const marks = changeMarks(diff, docLines);
  const thumb = scrollThumb(metrics);

  /** Drag the thumb, or click the track to jump to that offset. */
  const startDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const track = trackRef.current;
    if (!track || !thumb.scrollable) return;

    const trackLeft = track.getBoundingClientRect().left;
    const onThumb =
      event.clientX >= trackLeft + thumb.left &&
      event.clientX <= trackLeft + thumb.left + thumb.width;

    // Clicking the bare track centres the thumb where it was clicked; grabbing
    // the thumb keeps the offset under the pointer so it does not jump.
    const thumbLeft = onThumb ? thumb.left : event.clientX - trackLeft - thumb.width / 2;
    if (!onThumb) applyScrollLeft(scrollLeftForThumb(metrics, thumbLeft));

    dragRef.current = { pointerX: event.clientX, thumbLeft };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const continueDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    applyScrollLeft(
      scrollLeftForThumb(metrics, drag.thumbLeft + (event.clientX - drag.pointerX)),
    );
  };

  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    dragRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  return (
    <div className="diff-frame">
      <div
        className="diff-host"
        ref={hostRef}
        // Shift+wheel and horizontal trackpad gestures would otherwise reach a
        // native scrollbar that is thousands of pixels below the viewport.
        onWheel={(event) => {
          const delta = event.deltaX !== 0 ? event.deltaX : event.shiftKey ? event.deltaY : 0;
          if (delta === 0 || !thumb.scrollable) return;
          event.preventDefault();
          applyScrollLeft(metrics.scrollLeft + delta);
        }}
      />

      <div
        className="diff-markers"
        role="presentation"
        title={`${marks.length} change${marks.length === 1 ? "" : "s"} — click a mark to jump to it`}
      >
        {marks.map((mark, index) => (
          <button
            key={index}
            className={`mark ${mark.kind}`}
            style={{ top: `${mark.top * 100}%`, height: `${mark.height * 100}%` }}
            // The strip is drawn from the working document's line count, so a
            // mark's own position is the line to go to.
            onClick={() => revealLine(Math.round(mark.top * docLines) + 1)}
            title={`Jump to this ${mark.kind}`}
            aria-label={`Jump to ${mark.kind} at ${Math.round(mark.top * 100)}% of the file`}
          />
        ))}
      </div>

      <div
        className={`diff-hscroll ${thumb.scrollable ? "" : "idle"}`}
        ref={trackRef}
        onPointerDown={startDrag}
        onPointerMove={continueDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        {thumb.scrollable && (
          <div className="thumb" style={{ left: thumb.left, width: thumb.width }} />
        )}
      </div>
    </div>
  );
}
