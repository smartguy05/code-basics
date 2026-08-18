import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { DiffView, type DiffLayout, type DiffViewHandle } from "./DiffView";
import { Sidebar } from "./Sidebar";
import * as api from "../ipc/api";
import type { FileContents, FileDiff, StashEntry } from "../ipc/types";
import { formatTime, stashSummary } from "./stashLogic";

/**
 * The Stashes panel of the Changes tab: a list of stashes with a read-only diff
 * preview and the core actions (create, apply, pop, drop, clear).
 *
 * A stash is stored as a commit, so the preview reuses the History tab's exact
 * cascade — `gitCommitDiff` then `gitCommitFileContents` against the stash's
 * commit oid — rather than any stash-specific diff plumbing.
 */
export function StashPanel({
  header,
  onChanged,
}: {
  /** The segmented Files/Intent/Stashes toggle, rendered at the top of the list. */
  header: ReactNode;
  /** Called after a mutation so the Files view reflects an apply/pop when shown. */
  onChanged?: () => void;
}) {
  const [stashes, setStashes] = useState<StashEntry[]>([]);
  const [selected, setSelected] = useState<StashEntry | null>(null);
  const [diffs, setDiffs] = useState<FileDiff[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [contents, setContents] = useState<FileContents | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const diffHandle = useRef<DiffViewHandle | null>(null);

  const diffLayout: DiffLayout =
    localStorage.getItem("code-basics.diffLayout") === "inline" ? "inline" : "sideBySide";

  const shownDiff = diffs.find((diff) => diff.path === selectedFile) ?? null;

  const refresh = useCallback(async () => {
    try {
      const list = await api.gitStashList();
      setStashes(list);
      // Keep the current selection only if a stash with that index still holds
      // the same commit; git renumbers on drop/pop, so match on identity.
      setSelected((previous) =>
        previous == null
          ? null
          : (list.find((s) => s.id === previous.id) ?? null),
      );
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // The files a stash touched, as its commit left them.
  useEffect(() => {
    if (!selected) {
      setDiffs([]);
      setSelectedFile(null);
      return;
    }
    let cancelled = false;
    api
      .gitCommitDiff(selected.id)
      .then((files) => {
        if (cancelled) return;
        setDiffs(files);
        setSelectedFile(files[0]?.path ?? null);
      })
      .catch((e) => !cancelled && setError(api.errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [selected]);

  useEffect(() => {
    if (!selected || !selectedFile) {
      setContents(null);
      return;
    }
    let cancelled = false;
    api
      .gitCommitFileContents(selected.id, selectedFile)
      .then((next) => !cancelled && setContents(next))
      .catch((e) => !cancelled && setError(api.errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [selected, selectedFile]);

  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      await refresh();
      onChanged?.();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function create() {
    const note = window.prompt("Stash message", "work in progress");
    if (note != null) void act(() => api.gitStashSave(note));
  }

  function clearAll() {
    if (stashes.length === 0) return;
    const plural = stashes.length === 1 ? "stash" : "stashes";
    if (!window.confirm(`Drop all ${stashes.length} ${plural}? This cannot be undone.`)) {
      return;
    }
    void act(() => api.gitStashClear());
  }

  return (
    <>
      <Sidebar className="file-list">
        {header}

        <div style={{ display: "flex", gap: 4, padding: "4px 8px" }}>
          <button className="primary" disabled={busy} onClick={create} title="Stash the current working-tree changes">
            + Stash changes
          </button>
          <button
            disabled={busy || stashes.length === 0}
            onClick={clearAll}
            title="Drop every stash"
          >
            Clear all
          </button>
          {busy && <span className="spinner" style={{ alignSelf: "center" }} />}
        </div>

        <div className="group-label">Stashes</div>

        {stashes.length === 0 && (
          <div className="muted" style={{ padding: 8 }}>
            No stashes. Use “Stash changes” to set one aside.
          </div>
        )}

        {stashes.map((entry) => (
          <button
            key={entry.id}
            className={`row ${selected?.id === entry.id ? "selected" : ""}`}
            onClick={() => setSelected(entry)}
            title={entry.message}
            style={{ flexDirection: "column", alignItems: "stretch", gap: 2 }}
          >
            <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
              {entry.branch && <span className="badge">{entry.branch}</span>}
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
                {stashSummary(entry)}
              </span>
            </span>
            <span className="meta faint" style={{ fontSize: "0.85em" }}>
              stash@{"{"}
              {entry.index}
              {"}"} · {formatTime(entry.time)}
            </span>
          </button>
        ))}
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <button
            disabled={busy || !selected}
            onClick={() => selected && act(() => api.gitStashApply(selected.index))}
            title="Apply this stash and keep it in the list"
          >
            Apply
          </button>
          <button
            disabled={busy || !selected}
            onClick={() => selected && act(() => api.gitStashPop(selected.index))}
            title="Apply this stash and remove it from the list"
          >
            Pop
          </button>
          <button
            disabled={busy || !selected}
            onClick={() => selected && act(() => api.gitStashDrop(selected.index))}
            title="Remove this stash without applying it"
            style={{ color: selected ? "var(--fail)" : undefined }}
          >
            Drop
          </button>

          {busy && <span className="spinner" />}

          <span style={{ width: 12 }} />

          <button
            onClick={() => diffHandle.current?.goToChange(-1)}
            disabled={!shownDiff}
            title="Previous change (Shift+F7)"
            aria-label="Previous change"
          >
            ↑
          </button>
          <button
            onClick={() => diffHandle.current?.goToChange(1)}
            disabled={!shownDiff}
            title="Next change (F7)"
            aria-label="Next change"
          >
            ↓
          </button>
        </div>

        {error && <div className="error">{error}</div>}

        <div className="content split">
          <div className="bottom" style={{ flex: 1 }}>
            {selected ? (
              <div className="commit-detail">
                <div className="commit-message">
                  <strong>{stashSummary(selected)}</strong>
                  <div className="meta faint">
                    {selected.branch ? `on ${selected.branch} · ` : ""}
                    stash@{"{"}
                    {selected.index}
                    {"}"}
                  </div>
                </div>

                {diffs.length === 0 ? (
                  <div className="muted" style={{ padding: 8 }}>
                    This stash changed no files.
                  </div>
                ) : (
                  <>
                    <div className="commit-files">
                      {diffs.map((diff) => (
                        <button
                          key={diff.path}
                          className={`row ${diff.path === selectedFile ? "selected" : ""}`}
                          onClick={() => setSelectedFile(diff.path)}
                          title={diff.path}
                        >
                          {diff.path}
                        </button>
                      ))}
                    </div>

                    {shownDiff && shownDiff.isBinary && (
                      <div className="empty">{shownDiff.path} is a binary file.</div>
                    )}

                    {shownDiff && !shownDiff.isBinary && contents && (
                      <div className="commit-diff">
                        <DiffView
                          key={`${selected.id}:${shownDiff.path}`}
                          path={shownDiff.path}
                          baseline={contents.baseline}
                          working={contents.working ?? contents.baseline ?? ""}
                          diff={shownDiff}
                          layout={diffLayout}
                          editable={false}
                          onSave={() => {}}
                          onSelectionChange={() => {}}
                          handleRef={diffHandle}
                        />
                      </div>
                    )}
                  </>
                )}
              </div>
            ) : (
              <div className="empty">Select a stash to preview what it holds.</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
