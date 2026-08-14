import { useCallback, useEffect, useRef, useState } from "react";
import { DiffView, type DiffLayout, type DiffViewHandle } from "../components/DiffView";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import { Sidebar } from "../components/Sidebar";
import * as api from "../ipc/api";
import type {
  Branch,
  Commit,
  FileContents,
  FileDiff,
  NetworkKind,
  WorkingStatus,
} from "../ipc/types";
import { formatTime } from "./historyLogic";

export function HistoryView() {
  const [commits, setCommits] = useState<Commit[]>([]);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [selected, setSelected] = useState<Commit | null>(null);
  const [diffs, setDiffs] = useState<FileDiff[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showConsole, setShowConsole] = useState(false);
  /** Which of the commit's files the diff pane is showing. */
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [contents, setContents] = useState<FileContents | null>(null);

  const consoleRef = useRef<ConsoleHandle>(null);
  const diffHandle = useRef<DiffViewHandle | null>(null);

  /**
   * The layout preference the Changes tab owns, read rather than duplicated —
   * one setting for "how do I like diffs laid out" is enough.
   */
  const diffLayout: DiffLayout =
    localStorage.getItem("code-basics.diffLayout") === "inline" ? "inline" : "sideBySide";

  const shownDiff = diffs.find((diff) => diff.path === selectedFile) ?? null;

  const refresh = useCallback(async () => {
    try {
      const [history, branchList, currentStatus] = await Promise.all([
        api.gitHistory(200),
        api.gitBranches(),
        api.gitStatus(),
      ]);
      setCommits(history);
      setBranches(branchList);
      setStatus(currentStatus);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!selected) {
      setDiffs([]);
      setSelectedFile(null);
      return;
    }
    api
      .gitCommitDiff(selected.id)
      .then((files) => {
        setDiffs(files);
        // Open the first file straight away: a commit detail that shows only a
        // file list is one more click than it needs to be.
        setSelectedFile(files[0]?.path ?? null);
      })
      .catch((e) => setError(api.errorMessage(e)));
  }, [selected]);

  // Both sides of the open file, as this commit left them.
  useEffect(() => {
    if (!selected || !selectedFile) {
      setContents(null);
      return;
    }
    let cancelled = false;
    api
      .gitCommitFileContents(selected.id, selectedFile)
      .then((next) => {
        if (!cancelled) setContents(next);
      })
      .catch((e) => setError(api.errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [selected, selectedFile]);

  /**
   * F7 / Shift+F7, as in the Changes tab. This view is only mounted while its
   * tab is showing, so the binding is scoped without having to check.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "F7" || event.ctrlKey || event.altKey || event.metaKey) return;
      event.preventDefault();
      event.stopPropagation();
      diffHandle.current?.goToChange(event.shiftKey ? -1 : 1);
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

  async function network(kind: NetworkKind) {
    setBusy(true);
    setShowConsole(true);
    consoleRef.current?.clear();
    try {
      await api.gitNetwork(kind, (event) => consoleRef.current?.handle(event));
      await refresh();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      await refresh();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const hasUpstream = status?.upstream != null;

  return (
    <>
      <Sidebar>
        <div className="group-label">Branches</div>
        {branches
          .filter((branch) => !branch.isRemote)
          .map((branch) => (
            <div key={branch.name} style={{ display: "flex", gap: 4 }}>
              <button
                className={`row ${branch.isHead ? "selected" : ""}`}
                onClick={() => act(() => api.gitCheckoutBranch(branch.name))}
                disabled={busy}
              >
                <span style={{ flex: 1 }}>{branch.name}</span>
                {branch.upstream && <span className="badge">tracked</span>}
              </button>
              {!branch.isHead && (
                <button
                  onClick={() => act(() => api.gitDeleteBranch(branch.name))}
                  disabled={busy}
                  title={`Delete ${branch.name}`}
                >
                  ×
                </button>
              )}
            </div>
          ))}

        <button
          className="row"
          disabled={busy}
          onClick={() => {
            const name = window.prompt("New branch name");
            if (name?.trim()) act(() => api.gitCreateBranch(name.trim(), true));
          }}
        >
          + New branch
        </button>

        <div className="group-label">Stash</div>
        <button
          className="row"
          disabled={busy}
          onClick={() => {
            const note = window.prompt("Stash message", "work in progress");
            if (note != null) act(() => api.gitStashSave(note));
          }}
        >
          Stash changes
        </button>
        <button className="row" disabled={busy} onClick={() => act(api.gitStashPop)}>
          Pop most recent stash
        </button>
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <button onClick={() => network("fetch")} disabled={busy}>
            Fetch
          </button>
          <button onClick={() => network("pull")} disabled={busy}>
            Pull
          </button>
          <button
            onClick={() => network(hasUpstream ? "push" : "pushSetUpstream")}
            disabled={busy}
            title={
              hasUpstream
                ? "Push to the tracked upstream"
                : "This branch has no upstream; pushing will create one"
            }
          >
            Push{hasUpstream ? "" : " (set upstream)"}
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

          <span style={{ flex: 1 }} />

          <button onClick={() => setShowConsole((value) => !value)}>
            {showConsole ? "Hide output" : "Show output"}
          </button>
          <span className="muted">
            {status?.branch ?? "detached"}
            {status?.upstream ? ` → ${status.upstream}` : " (no upstream)"}
          </span>
        </div>

        {error && <div className="error">{error}</div>}

        <div className="content split">
          <div className="top">
            {commits.length === 0 && <div className="empty">No commits yet.</div>}
            {commits.map((commit) => (
              <div
                key={commit.id}
                className={`commit-row ${selected?.id === commit.id ? "selected" : ""}`}
                onClick={() => setSelected(commit)}
              >
                <div className="summary">{commit.summary}</div>
                <div className="meta">
                  <span className="mono">{commit.shortId}</span>
                  <span>{commit.authorName}</span>
                  <span>{formatTime(commit.time)}</span>
                </div>
              </div>
            ))}
          </div>

          <div className="bottom">
            {showConsole ? (
              <OutputConsole ref={consoleRef} />
            ) : selected ? (
              <div className="commit-detail">
                <div className="commit-message">
                  <strong>{selected.summary}</strong>
                  {selected.body && <pre>{selected.body}</pre>}
                </div>

                {diffs.length === 0 ? (
                  <div className="muted" style={{ padding: 8 }}>
                    This commit changed no files.
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
                          // Rebuilt per commit *and* per file: the editor is
                          // constructed from the document it opens on.
                          key={`${selected.id}:${shownDiff.path}`}
                          path={shownDiff.path}
                          baseline={contents.baseline}
                          // A file the commit deleted has no "after" side; show
                          // the baseline so there is something to read.
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
              <div className="empty">Select a commit to see what it changed.</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
