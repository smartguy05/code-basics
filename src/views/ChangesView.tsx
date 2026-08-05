import { useCallback, useEffect, useState } from "react";
import {
  DiffView,
  allChangedIndices,
  type DiffLayout,
} from "../components/DiffView";
import { Sidebar } from "../components/Sidebar";
import * as api from "../ipc/api";
import type {
  ComparisonMode,
  FileChange,
  FileContents,
  FileDiff,
  WorkingStatus,
} from "../ipc/types";

const DIFF_LAYOUT_KEY = "code-basics.diffLayout";

function loadDiffLayout(): DiffLayout {
  return localStorage.getItem(DIFF_LAYOUT_KEY) === "inline" ? "inline" : "sideBySide";
}

const MODE_LABELS: Record<ComparisonMode, string> = {
  workingToHead: "Working tree vs HEAD",
  workingToIndex: "Unstaged (vs staged)",
  indexToHead: "Staged (vs HEAD)",
};

function statusLetter(change: FileChange): { letter: string; className: string } {
  if (change.staged === "conflicted" || change.unstaged === "conflicted") {
    return { letter: "!", className: "conflicted" };
  }
  const kind = change.unstaged ?? change.staged;
  const letter =
    kind === "added" || kind === "untracked"
      ? "A"
      : kind === "deleted"
        ? "D"
        : kind === "renamed"
          ? "R"
          : "M";

  return {
    letter,
    className: change.unstaged ? "unstaged" : "staged",
  };
}

export function ChangesView() {
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [mode, setMode] = useState<ComparisonMode>("workingToHead");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [contents, setContents] = useState<FileContents | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [selectedLines, setSelectedLines] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [amend, setAmend] = useState(false);
  const [busy, setBusy] = useState(false);
  const [diffLayout, setDiffLayout] = useState<DiffLayout>(loadDiffLayout);

  function changeDiffLayout(layout: DiffLayout) {
    setDiffLayout(layout);
    localStorage.setItem(DIFF_LAYOUT_KEY, layout);
  }

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.gitStatus());
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const loadFile = useCallback(
    async (path: string, comparison: ComparisonMode) => {
      try {
        const [fileContents, fileDiff] = await Promise.all([
          api.gitFileContents(path, comparison),
          api.gitFileDiff(path, comparison),
        ]);
        setContents(fileContents);
        setDiff(fileDiff);
        setSelectedLines([]);
        setError(null);
      } catch (e) {
        setError(api.errorMessage(e));
      }
    },
    [],
  );

  useEffect(() => {
    if (selectedPath) void loadFile(selectedPath, mode);
  }, [selectedPath, mode, loadFile]);

  /** Re-read both the file list and the open file after a mutation. */
  const refreshAll = useCallback(async () => {
    await refreshStatus();
    if (selectedPath) await loadFile(selectedPath, mode);
  }, [refreshStatus, loadFile, selectedPath, mode]);

  async function withBusy(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const revert = (lines: number[]) =>
    withBusy(async () => {
      if (!selectedPath || lines.length === 0) return;
      await api.gitRevertLines(selectedPath, mode, lines);
      await refreshAll();
    });

  const stage = (lines: number[]) =>
    withBusy(async () => {
      if (!selectedPath) return;
      if (lines.length === 0) await api.gitStageFile(selectedPath);
      else await api.gitStageLines(selectedPath, lines);
      await refreshAll();
    });

  const unstage = (lines: number[]) =>
    withBusy(async () => {
      if (!selectedPath) return;
      if (lines.length === 0) await api.gitUnstageFile(selectedPath);
      else await api.gitUnstageLines(selectedPath, lines);
      await refreshAll();
    });

  const commit = () =>
    withBusy(async () => {
      await api.gitCommit(message, amend);
      setMessage("");
      setAmend(false);
      await refreshAll();
    });

  const save = (content: string) =>
    withBusy(async () => {
      if (!selectedPath) return;
      await api.gitWriteFile(selectedPath, content);
      await refreshAll();
    });

  const files = status?.files ?? [];
  const hasSelection = selectedLines.length > 0;
  const canRevertAll = diff != null && allChangedIndices(diff).length > 0;

  return (
    <>
      <Sidebar className="file-list">
        <div className="group-label">
          {status?.branch ?? "no branch"}
          {status && (status.ahead > 0 || status.behind > 0) && (
            <span className="badge" style={{ marginLeft: 6 }}>
              ↑{status.ahead} ↓{status.behind}
            </span>
          )}
        </div>

        {status?.inProgressOperation && (
          <div className="warning">A {status.inProgressOperation} is in progress.</div>
        )}

        {files.length === 0 && (
          <div className="muted" style={{ padding: 8 }}>
            No changes.
          </div>
        )}

        {files.map((change) => {
          const { letter, className } = statusLetter(change);
          return (
            <button
              key={change.path}
              className={`row ${change.path === selectedPath ? "selected" : ""}`}
              onClick={() => setSelectedPath(change.path)}
              title={change.path}
            >
              <span className={`status ${className}`}>{letter}</span>
              <span className="path">{change.path}</span>
            </button>
          );
        })}

        <div className="commit-box">
          <textarea
            placeholder="Commit message"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
          />
          <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <input
              type="checkbox"
              checked={amend}
              onChange={(e) => setAmend(e.target.checked)}
            />
            Amend previous commit
          </label>
          <button
            className="primary"
            onClick={commit}
            disabled={busy || !message.trim()}
          >
            Commit
          </button>
        </div>
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <select
            value={mode}
            onChange={(e) => setMode(e.target.value as ComparisonMode)}
          >
            {(Object.keys(MODE_LABELS) as ComparisonMode[]).map((value) => (
              <option key={value} value={value}>
                {MODE_LABELS[value]}
              </option>
            ))}
          </select>

          <button
            onClick={() => revert(selectedLines)}
            disabled={busy || !hasSelection}
            title={
              hasSelection
                ? `Revert ${selectedLines.length} selected line(s)`
                : "Click line numbers to select lines to revert"
            }
          >
            Revert selected{hasSelection ? ` (${selectedLines.length})` : ""}
          </button>
          <button
            onClick={() => diff && revert(allChangedIndices(diff))}
            disabled={busy || !canRevertAll}
          >
            Revert file
          </button>

          <span style={{ width: 12 }} />

          <button onClick={() => stage(selectedLines)} disabled={busy || !selectedPath}>
            Stage{hasSelection ? " selected" : " file"}
          </button>
          <button onClick={() => unstage(selectedLines)} disabled={busy || !selectedPath}>
            Unstage{hasSelection ? " selected" : " file"}
          </button>

          <span style={{ flex: 1 }} />

          <select
            value={diffLayout}
            onChange={(e) => changeDiffLayout(e.target.value as DiffLayout)}
            title="How to lay the comparison out"
          >
            <option value="sideBySide">Side by side</option>
            <option value="inline">Inline</option>
          </select>

          <span className="faint" style={{ fontSize: 11 }}>
            Click a line number to select · ⌘S / Ctrl+S to save an edit
          </span>
        </div>

        {error && <div className="error">{error}</div>}

        <div className="content">
          {!selectedPath && (
            <div className="empty">Select a file to see its changes.</div>
          )}

          {selectedPath && diff?.isBinary && (
            <div className="empty">{selectedPath} is a binary file.</div>
          )}

          {selectedPath && contents && diff && !diff.isBinary && (
            contents.working == null ? (
              <div className="empty">
                {selectedPath} was deleted.
                {canRevertAll && (
                  <div style={{ marginTop: 12 }}>
                    <button onClick={() => revert(allChangedIndices(diff))}>
                      Restore it
                    </button>
                  </div>
                )}
              </div>
            ) : (
              <DiffView
                path={selectedPath}
                baseline={contents.baseline}
                working={contents.working}
                diff={diff}
                layout={diffLayout}
                editable
                onSave={save}
                onSelectionChange={setSelectedLines}
              />
            )
          )}
        </div>
      </div>
    </>
  );
}
