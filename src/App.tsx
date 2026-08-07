import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { BranchMenu } from "./components/BranchMenu";
import { ChangesView } from "./views/ChangesView";
import { HistoryView } from "./views/HistoryView";
import { InspectView } from "./views/InspectView";
import { RunView } from "./views/RunView";
import { TestsView } from "./views/TestsView";
import * as api from "./ipc/api";
import { loadRecents, rememberRecent } from "./recentsLogic";
import type { InspectTarget, RootSpec, Workspace } from "./ipc/types";

type Tab = "tests" | "run" | "changes" | "history" | "inspect";

/**
 * A jump into the Objects tab raised from somewhere else in the app.
 *
 * This is the UI's request, not the wire request the sidecar reads (that is
 * `InspectRequest` in `ipc/types.ts`): caps and suspension are the backend's
 * business, and all a crashed run or a red test knows is what to look at and
 * why.
 */
export interface InspectRequest {
  target: InspectTarget;
  root: RootSpec;
  /** Shown above the capture so the user knows what they clicked. */
  reason: string;
}

const TABS: { id: Tab; label: string }[] = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
  { id: "changes", label: "Changes" },
  { id: "history", label: "History" },
  { id: "inspect", label: "Objects" },
];

/** True when running inside the Tauri webview (false in a plain browser tab). */
const inTauri = "__TAURI_INTERNALS__" in window;

export function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [tab, setTab] = useState<Tab>("run");
  const [error, setError] = useState<string | null>(null);
  const [recents, setRecents] = useState<string[]>(() => loadRecents(localStorage));
  const [loading, setLoading] = useState(true);
  /**
   * A contextual Inspect click, held only until the Objects tab has consumed
   * it. It lives here because the views that raise one and the view that
   * serves it are siblings.
   */
  const [inspectRequest, setInspectRequest] = useState<InspectRequest | null>(null);

  /** Send the user to the Objects tab with something already chosen to read. */
  function requestInspect(request: InspectRequest) {
    setInspectRequest(request);
    setTab("inspect");
  }

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
      rememberRecent(localStorage, opened.root);
      setRecents(loadRecents(localStorage));
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

      {/* Run, Tests and Objects stay mounted while hidden: they own running
          processes and their consoles, which must survive a tab switch.
          Changes and History re-mount so they re-read git state on every
          visit. */}
      <div className="body" hidden={tab !== "run"}>
        <RunView
          workspace={workspace}
          onWorkspaceChange={setWorkspace}
          onInspect={requestInspect}
        />
      </div>
      <div className="body" hidden={tab !== "tests"}>
        <TestsView workspace={workspace} key={workspace.root} onInspect={requestInspect} />
      </div>
      <div className="body" hidden={tab !== "inspect"}>
        <InspectView
          workspace={workspace}
          key={workspace.root}
          pendingRequest={inspectRequest}
          onRequestConsumed={() => setInspectRequest(null)}
        />
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
