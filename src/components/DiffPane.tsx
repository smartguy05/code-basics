import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { DiffView, type DiffLayout, type DiffViewHandle } from "./DiffView";
import { allChangedIndices } from "./diffLogic";
import { confidenceForFile } from "./confidenceLogic";
import { uncoveredIndicesForPath } from "./coverageOfChangeLogic";
import {
  COLLAPSE_KEY,
  DIFF_LAYOUT_KEY,
  WHITESPACE_KEY,
  diffToolbarState,
  loadCollapse,
  loadDiffLayout,
  loadIgnoreWhitespace,
  riskIndices,
  shownDiff as shownDiffOf,
  viewBaseline as viewBaselineOf,
} from "./diffPaneLogic";
import * as api from "../ipc/api";
import { applyEditorFontSize, loadEditorFontSize, onEditorFontSizeChange } from "../editorFontSize";
import { MAX_EDITOR_FONT_SIZE, MIN_EDITOR_FONT_SIZE, stepFontSize } from "../editorFontSizeLogic";
import type {
  ChangeCoverage,
  ComparisonMode,
  ErosionReport,
  FileContents,
  FileDiff,
  IntentGroup,
} from "../ipc/types";

const MODE_LABELS: Record<ComparisonMode, string> = {
  workingToHead: "Working tree vs HEAD",
  workingToIndex: "Unstaged (vs staged)",
  indexToHead: "Staged (vs HEAD)",
};

export interface DiffPaneProps {
  /**
   * The file being compared, or `null` for the "select a file" empty state.
   * The pane re-reads it whenever this or {@link mode} changes.
   */
  path: string | null;

  /**
   * The comparison this whole view is showing. Lifted, never pane-local: the
   * erosion, coverage and intent indices are numbered per-mode, so a pane
   * disagreeing about it would paint the washes against a differently-numbered
   * diff. Its only editor is this toolbar, which is why `onModeChange` exists.
   */
  mode: ComparisonMode;
  onModeChange: (mode: ComparisonMode) => void;

  /**
   * The open card's hunks in this file, or `null` for the whole diff. This is
   * the pane's only knowledge of the intent grouping — leaving that grouping
   * already clears the scope, so the two were never independent.
   */
  scopedHunks: number[] | null;
  /** Diff line indices to preselect, so opening a card lands on its lines. */
  highlight: number[];

  /**
   * Whole-working-tree scans, handed down unscoped. Each is narrowed to `path`
   * here (`uncoveredIndicesForPath` / `hunkRisk` / `confidenceForFile` all
   * filter by path themselves), so the pane needs no fetch of its own.
   */
  intentGroups: IntentGroup[];
  erosion: ErosionReport | null;
  coverage: ChangeCoverage | null;

  /** An action is in flight somewhere in the view; disables this pane's actions. */
  busy: boolean;
  /**
   * Run `action` under the container's busy flag and error banner — the
   * container's `withBusy` handed down rather than reimplemented, so the pane
   * cannot own a second busy flag that leaves the commit box enabled mid-stage.
   */
  runAction: (action: () => Promise<void>) => Promise<void>;
  /**
   * Report (or clear) a read failure; the banner belongs to the container.
   *
   * The pane holds this in a ref rather than depending on it, so a container
   * may pass an inline arrow. That is not a style preference: `loadFile`
   * depends on it and the load effect depends on `loadFile`, so a fresh
   * identity each render would refire the effect, which sets state, which
   * renders again — an unbounded pair of git IPC calls per frame. It happens to
   * be safe today only because the one caller passes a `useState` setter, and
   * the second container arrives in the next stage.
   */
  onError: (message: string | null) => void;
  /** This pane mutated the tree: re-read the file list and the scans. */
  onMutated: () => Promise<void>;

  /**
   * Where the pane publishes its own "re-read my file" callback, so the
   * container's post-mutation refresh can reload the diff *in the position it
   * always did* — after the status read and before the three scans. Ordering is
   * load-bearing: a reload that ran last would clear a scan's error banner,
   * because a successful read reports no error.
   */
  reloadRef: { current: (() => Promise<void>) | null };

  /**
   * Rendered between the toolbar and the content. The container's error banner
   * goes here so a side-panel scan failure is not hidden behind the diff, while
   * the pane still emits the `.main` children in the order the layout expects.
   */
  banner?: ReactNode;
}

/**
 * The diff half of the Changes view: the comparison toolbar and the diff (or
 * editor) under it.
 *
 * Renders a fragment of exactly the shape a `.main` element's children have —
 * toolbar, then optional banner, then content — so a container drops it
 * straight in without wrapping.
 *
 * **Exactly one of these is ever mounted: the one being looked at.** Two are
 * not merely wasteful, they are wrong — `executeCommand` (`shortcuts.ts`)
 * falls back to `document.querySelector`'s *first* match for `[data-command]`,
 * and a hidden toolbar is still a match, so a background pane would steal
 * `changes.stage` and stage the wrong file. The accepted cost is that switching
 * away from a diff and back re-reads that file (one IPC call) and does not
 * preserve the scroll position; a toolbar holds no state worth keeping mounted,
 * unlike the consoles this codebase deliberately keeps alive while hidden.
 */
export function DiffPane({
  path,
  mode,
  onModeChange,
  scopedHunks,
  highlight,
  intentGroups,
  erosion,
  coverage,
  busy,
  runAction,
  onError,
  onMutated,
  reloadRef,
  banner,
}: DiffPaneProps) {
  const [contents, setContents] = useState<FileContents | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [selectedLines, setSelectedLines] = useState<number[]>([]);
  const [diffLayout, setDiffLayout] = useState<DiffLayout>(() => loadDiffLayout(localStorage));
  const [collapseUnchanged, setCollapseUnchanged] = useState(() => loadCollapse(localStorage));
  const [ignoreWhitespace, setIgnoreWhitespace] = useState(() =>
    loadIgnoreWhitespace(localStorage),
  );

  /**
   * The editor font size, mirrored here only so the buttons can grey out at the
   * ends of the range.
   *
   * `editorFontSize.ts` is the single source of truth — the buttons apply
   * through it and this state is refreshed from the change event, so pressing
   * Ctrl+= keeps the buttons honest without this pane owning the value.
   */
  const [fontSize, setFontSize] = useState(loadEditorFontSize);
  useEffect(() => onEditorFontSizeChange(() => setFontSize(loadEditorFontSize())), []);

  /** The open diff, for the toolbar arrows and F7. */
  const diffHandle = useRef<DiffViewHandle | null>(null);

  // `onError` is read through a ref kept current on every render, so nothing
  // below depends on its *identity*. See the prop's comment for why that is a
  // correctness matter and not a style one. `onMutated` needs no such treatment
  // — it is only ever called from an event handler, never from a dependency
  // array — so it is deliberately not folded in here.
  const errorSink = useRef(onError);
  errorSink.current = onError;

  const loadFile = useCallback(async (file: string, comparison: ComparisonMode) => {
    try {
      const [fileContents, fileDiff] = await Promise.all([
        api.gitFileContents(file, comparison),
        api.gitFileDiff(file, comparison),
      ]);
      setContents(fileContents);
      setDiff(fileDiff);
      setSelectedLines([]);
      errorSink.current(null);
    } catch (e) {
      errorSink.current(api.errorMessage(e));
    }
  }, []);

  // Keyed on `mode` as well as `path`, deliberately: a selection made under one
  // comparison's line numbering must not survive into another, and re-reading
  // is what drops it.
  useEffect(() => {
    if (path) void loadFile(path, mode);
  }, [path, mode, loadFile]);


  const reload = useCallback(async () => {
    if (path) await loadFile(path, mode);
  }, [path, mode, loadFile]);

  useEffect(() => {
    reloadRef.current = reload;
    return () => {
      if (reloadRef.current === reload) reloadRef.current = null;
    };
  }, [reload, reloadRef]);

  const revert = (lines: number[]) =>
    runAction(async () => {
      if (!path || lines.length === 0) return;
      await api.gitRevertLines(path, mode, lines);
      await onMutated();
    });

  const stage = (lines: number[]) =>
    runAction(async () => {
      if (!path) return;
      if (lines.length === 0) await api.gitStageFile(path);
      else await api.gitStageLines(path, lines);
      await onMutated();
    });

  const unstage = (lines: number[]) =>
    runAction(async () => {
      if (!path) return;
      if (lines.length === 0) await api.gitUnstageFile(path);
      else await api.gitUnstageLines(path, lines);
      await onMutated();
    });

  const save = (content: string) =>
    runAction(async () => {
      if (!path) return;
      await api.gitWriteFile(path, content);
      await onMutated();
    });

  function changeDiffLayout(layout: DiffLayout) {
    setDiffLayout(layout);
    localStorage.setItem(DIFF_LAYOUT_KEY, layout);
  }

  function changeIgnoreWhitespace(next: boolean) {
    setIgnoreWhitespace(next);
    localStorage.setItem(WHITESPACE_KEY, String(next));
  }

  function changeCollapse(next: boolean) {
    setCollapseUnchanged(next);
    localStorage.setItem(COLLAPSE_KEY, String(next));
  }

  const shownDiff = shownDiffOf(diff, scopedHunks);
  // Memoised so the diff editor is not torn down and rebuilt on unrelated
  // renders — the baseline is in its deps.
  const viewBaseline = useMemo(
    () => viewBaselineOf(contents, diff, scopedHunks),
    [contents, diff, scopedHunks],
  );
  const toolbar = diffToolbarState({ shownDiff, scopedHunks, selectedLines, path, busy });

  // The uncovered changed-line indices for the open file only. Scoped by path
  // (not the whole report) because DiffLine.index is per-file, so an index from
  // another file could collide with a real line number in this one.
  const uncoveredForFile = uncoveredIndicesForPath(coverage, path);
  const riskForFile = useMemo(
    () => riskIndices(path, diff, erosion?.flags ?? [], intentGroups),
    [path, diff, erosion, intentGroups],
  );
  /**
   * Per-line agent self-confidence for the open file — the confidence heatmap.
   * Keyed by `DiffLine.index`, which survives the `onlyHunks` scoping, so the
   * tints still land when the pane shows only a card's hunks.
   */
  const confidenceForOpenFile = useMemo(() => {
    if (!diff || !path) return [];
    return confidenceForFile(path, diff, intentGroups);
  }, [diff, path, intentGroups]);

  return (
    <>
      <div className="toolbar">
        <select
          value={mode}
          onChange={(e) => onModeChange(e.target.value as ComparisonMode)}
        >
          {(Object.keys(MODE_LABELS) as ComparisonMode[]).map((value) => (
            <option key={value} value={value}>
              {MODE_LABELS[value]}
            </option>
          ))}
        </select>

        <button
          onClick={() => revert(selectedLines)}
          disabled={!toolbar.canRevertSelected}
          title={
            toolbar.hasSelection
              ? `Revert ${selectedLines.length} selected line(s)`
              : "Click line numbers to select lines to revert"
          }
        >
          Revert selected{toolbar.hasSelection ? ` (${selectedLines.length})` : ""}
        </button>
        <button
          onClick={() => shownDiff && revert(allChangedIndices(shownDiff))}
          disabled={!toolbar.canRevertAll}
        >
          Revert {toolbar.revertAllLabel}
        </button>

        <span style={{ width: 12 }} />

        <button data-command="changes.stage" onClick={() => stage(selectedLines)} disabled={!toolbar.canStage}>
          Stage{toolbar.hasSelection ? " selected" : " file"}
        </button>
        <button data-command="changes.unstage" onClick={() => unstage(selectedLines)} disabled={!toolbar.canStage}>
          Unstage{toolbar.hasSelection ? " selected" : " file"}
        </button>

        <span style={{ width: 12 }} />

        <button
          data-command="change.previous"
          onClick={() => diffHandle.current?.goToChange(-1)}
          disabled={!toolbar.canStep}
          title="Previous change (Shift+F7)"
          aria-label="Previous change"
        >
          ↑
        </button>
        <button
          data-command="change.next"
          onClick={() => diffHandle.current?.goToChange(1)}
          disabled={!toolbar.canStep}
          title="Next change (F7)"
          aria-label="Next change"
        >
          ↓
        </button>
        <span className="faint" style={{ fontSize: 11, whiteSpace: "nowrap" }}>
          {toolbar.differences} difference{toolbar.differences === 1 ? "" : "s"}
        </span>

        <span style={{ flex: 1 }} />

        <select
          value={diffLayout}
          onChange={(e) => changeDiffLayout(e.target.value as DiffLayout)}
          title="How to lay the comparison out"
        >
          <option value="sideBySide">Side by side</option>
          <option value="inline">Inline</option>
        </select>

        <select
          value={ignoreWhitespace ? "ignore" : "exact"}
          onChange={(e) => changeIgnoreWhitespace(e.target.value === "ignore")}
          title="Whitespace-only differences change how the diff is drawn, never what Stage or Revert act on"
        >
          <option value="exact">Do not ignore</option>
          <option value="ignore">Ignore whitespace</option>
        </select>

        <label
          style={{ display: "flex", gap: 4, alignItems: "center", fontSize: 11 }}
          title="Fold away long runs of unchanged code, keeping a few lines either side of each change"
        >
          <input
            type="checkbox"
            checked={collapseUnchanged}
            onChange={(e) => changeCollapse(e.target.checked)}
          />
          Collapse unchanged
        </label>

        <span className="font-size-controls">
          <button
            onClick={() => applyEditorFontSize(stepFontSize(fontSize, -1))}
            disabled={fontSize <= MIN_EDITOR_FONT_SIZE}
            title="Smaller text (Ctrl+-)"
            aria-label="Smaller text"
          >
            A−
          </button>
          <button
            onClick={() => applyEditorFontSize(stepFontSize(fontSize, 1))}
            disabled={fontSize >= MAX_EDITOR_FONT_SIZE}
            title="Larger text (Ctrl+=)"
            aria-label="Larger text"
          >
            A+
          </button>
        </span>

        <span className="faint" style={{ fontSize: 11 }}>
          Click a line number to select · ⌘S / Ctrl+S to save an edit
        </span>
      </div>

      {banner}

      <div className="content">
        {!path && <div className="empty">Select a file to see its changes.</div>}

        {path && diff?.isBinary && <div className="empty">{path} is a binary file.</div>}

        {path && contents && shownDiff && !shownDiff.isBinary && (
          contents.working == null ? (
            <div className="empty">
              {path} was deleted.
              {toolbar.hasRevertableChanges && (
                <div style={{ marginTop: 12 }}>
                  <button onClick={() => revert(allChangedIndices(shownDiff))}>Restore it</button>
                </div>
              )}
            </div>
          ) : (
            <DiffView
              path={path}
              baseline={viewBaseline}
              working={contents.working}
              diff={shownDiff}
              layout={diffLayout}
              collapseUnchanged={collapseUnchanged}
              ignoreWhitespace={ignoreWhitespace}
              editable
              onSave={save}
              onSelectionChange={setSelectedLines}
              highlight={highlight}
              uncovered={uncoveredForFile}
              risk={riskForFile}
              confidence={confidenceForOpenFile}
              handleRef={diffHandle}
            />
          )
        )}
      </div>
    </>
  );
}
