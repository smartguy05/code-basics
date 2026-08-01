import { useCallback, useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import * as api from "../ipc/api";
import type { Branch, Commit, FileDiff, NetworkKind, WorkingStatus } from "../ipc/types";

function formatTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

/** Render a file diff as plain unified text for the read-only commit view. */
function unifiedText(diff: FileDiff): string {
  const lines: string[] = [];
  for (const hunk of diff.hunks) {
    lines.push(
      `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@ ${hunk.header}`,
    );
    for (const line of hunk.lines) {
      const marker =
        line.origin === "addition" ? "+" : line.origin === "deletion" ? "-" : " ";
      lines.push(marker + line.content);
    }
  }
  return lines.join("\n");
}

export function HistoryView() {
  const [commits, setCommits] = useState<Commit[]>([]);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [selected, setSelected] = useState<Commit | null>(null);
  const [diffs, setDiffs] = useState<FileDiff[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showConsole, setShowConsole] = useState(false);

  const consoleRef = useRef<ConsoleHandle>(null);

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
      return;
    }
    api
      .gitCommitDiff(selected.id)
      .then(setDiffs)
      .catch((e) => setError(api.errorMessage(e)));
  }, [selected]);

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
      <div className="sidebar">
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
      </div>

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
              <div className="failure-detail">
                <h3>{selected.summary}</h3>
                {selected.body && <pre>{selected.body}</pre>}
                {diffs.length === 0 && (
                  <div className="muted">This commit changed no files.</div>
                )}
                {diffs.map((diff) => (
                  <div key={diff.path}>
                    <div className="muted mono">{diff.path}</div>
                    <pre>{unifiedText(diff)}</pre>
                  </div>
                ))}
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
