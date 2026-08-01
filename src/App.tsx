import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ChangesView } from "./views/ChangesView";
import { HistoryView } from "./views/HistoryView";
import { RunView } from "./views/RunView";
import { TestsView } from "./views/TestsView";
import * as api from "./ipc/api";
import type { Workspace } from "./ipc/types";

type Tab = "tests" | "run" | "changes" | "history";

const TABS: { id: Tab; label: string }[] = [
  { id: "tests", label: "Tests" },
  { id: "run", label: "Run" },
  { id: "changes", label: "Changes" },
  { id: "history", label: "History" },
];

/** Workspaces the user has opened before, so reopening is one click. */
const RECENTS_KEY = "code-basics.recentWorkspaces";

function loadRecents(): string[] {
  try {
    const raw = localStorage.getItem(RECENTS_KEY);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

function rememberRecent(path: string) {
  const recents = [path, ...loadRecents().filter((p) => p !== path)].slice(0, 8);
  localStorage.setItem(RECENTS_KEY, JSON.stringify(recents));
}

export function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [tab, setTab] = useState<Tab>("tests");
  const [error, setError] = useState<string | null>(null);
  const [recents, setRecents] = useState<string[]>(loadRecents);
  const [loading, setLoading] = useState(true);

  // The backend keeps the open workspace across a window reload.
  useEffect(() => {
    api
      .currentWorkspace()
      .then(setWorkspace)
      .catch(() => {
        /* nothing open */
      })
      .finally(() => setLoading(false));
  }, []);

  async function openPath(path: string) {
    try {
      const opened = await api.openWorkspace(path);
      setWorkspace(opened);
      rememberRecent(opened.root);
      setRecents(loadRecents());
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function pickFolder() {
    const chosen = await open({ directory: true, multiple: false });
    if (typeof chosen === "string") await openPath(chosen);
  }

  async function rescan() {
    try {
      setWorkspace(await api.rescanWorkspace());
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  if (loading) {
    return <div className="empty">Loading…</div>;
  }

  if (!workspace) {
    return (
      <div className="app">
        <div className="empty" style={{ paddingTop: 80 }}>
          <h2 style={{ marginBottom: 4 }}>code-basics</h2>
          <p className="muted">Open a repository to get started.</p>

          <div style={{ marginTop: 16 }}>
            <button className="primary" onClick={pickFolder}>
              Open folder…
            </button>
          </div>

          {error && <div className="error">{error}</div>}

          {recents.length > 0 && (
            <div style={{ marginTop: 28, textAlign: "left", display: "inline-block" }}>
              <div className="group-label">Recent</div>
              {recents.map((path) => (
                <button key={path} className="row mono" onClick={() => openPath(path)}>
                  {path}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <div className="titlebar">
        <span className="workspace-name">{workspace.name}</span>
        <span className="faint mono" style={{ fontSize: 11 }}>
          {workspace.root}
        </span>

        <div className="spacer" />

        <div className="tabs">
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              className={tab === id ? "active" : ""}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="spacer" />

        <span className="muted" style={{ fontSize: 11 }}>
          {workspace.projects.length} project
          {workspace.projects.length === 1 ? "" : "s"}
        </span>
        <button onClick={rescan} title="Re-detect projects and configurations">
          Rescan
        </button>
        <button onClick={pickFolder}>Open…</button>
      </div>

      {error && <div className="error">{error}</div>}

      <div className="body">
        {tab === "tests" && <TestsView workspace={workspace} key={workspace.root} />}
        {tab === "run" && (
          <RunView workspace={workspace} onWorkspaceChange={setWorkspace} />
        )}
        {tab === "changes" && <ChangesView key={workspace.root} />}
        {tab === "history" && <HistoryView key={workspace.root} />}
      </div>
    </div>
  );
}
