import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { BranchMenu } from "./components/BranchMenu";
import { ChangesView } from "./views/ChangesView";
import { HistoryView } from "./views/HistoryView";
import { RunView } from "./views/RunView";
import { TestsView } from "./views/TestsView";
import * as api from "./ipc/api";
import type { Workspace } from "./ipc/types";

type Tab = "tests" | "run" | "changes" | "history";

const TABS: { id: Tab; label: string }[] = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
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

/** True when running inside the Tauri webview (false in a plain browser tab). */
const inTauri = "__TAURI_INTERNALS__" in window;

export function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [tab, setTab] = useState<Tab>("run");
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
    try {
      const chosen = await open({ directory: true, multiple: false });
      if (typeof chosen === "string") await openPath(chosen);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function rescan() {
    try {
      setWorkspace(await api.rescanWorkspace());
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  if (!inTauri) {
    return (
      <div className="empty" style={{ paddingTop: 80 }}>
        <h2 style={{ marginBottom: 4 }}>code-basics</h2>
        <p className="muted">
          This page is running in a plain browser, so the desktop backend is not
          available. Launch the app with <code>pnpm tauri dev</code> and use the
          native window instead.
        </p>
      </div>
    );
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

        {/* Keyed by root: a different workspace is a different repository. */}
        <BranchMenu key={workspace.root} />

        {/* The Run view portals its configuration dropdown here (it owns the
            selection and process state; see RunConfigMenu). */}
        <div id="run-config-slot" />

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

      <div className="tabs tabs-row">
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

      {error && <div className="error">{error}</div>}

      {/* Run and Tests stay mounted while hidden: they own running processes
          and their consoles, which must survive a tab switch. Changes and
          History re-mount so they re-read git state on every visit. */}
      <div className="body" hidden={tab !== "run"}>
        <RunView workspace={workspace} onWorkspaceChange={setWorkspace} />
      </div>
      <div className="body" hidden={tab !== "tests"}>
        <TestsView workspace={workspace} key={workspace.root} />
      </div>
      {tab === "changes" && (
        <div className="body">
          <ChangesView key={workspace.root} />
        </div>
      )}
      {tab === "history" && (
        <div className="body">
          <HistoryView key={workspace.root} />
        </div>
      )}
    </div>
  );
}
