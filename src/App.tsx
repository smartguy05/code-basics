import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { BranchMenu } from "./components/BranchMenu";
import { MenuBar } from "./components/MenuBar";
import { NotesPanel } from "./components/NotesPanel";
import { RunningPanel } from "./components/RunningPanel";
import { liveCount } from "./components/runningLogic";
import { WorkspaceTab, type WorkspaceTabHandle } from "./components/WorkspaceTab";
import {
  addOpenWorkspace,
  closeOpenWorkspace,
  shouldFlashWorkspaceTab,
  tabLabels,
} from "./components/workspaceTabsLogic";
import * as api from "./ipc/api";
import { applyEditorFontSize, loadEditorFontSize } from "./editorFontSize";
import { DEFAULT_EDITOR_FONT_SIZE, recogniseFontSizeShortcut, stepFontSize } from "./editorFontSizeLogic";
import { loadRecents, rememberRecent } from "./recentsLogic";
import type { InspectTarget, RootSpec, RunningReport, Workspace } from "./ipc/types";

/**
 * A jump into the Objects tab raised from somewhere else in a workspace tab.
 *
 * This is the UI's request, not the wire request the sidecar reads (that is
 * `InspectRequest` in `ipc/types.ts`): caps and suspension are the backend's
 * business, and all a crashed run or a red test knows is what to look at and why.
 */
export interface InspectRequest {
  target: InspectTarget;
  root: RootSpec;
  /** Shown above the capture so the user knows what they clicked. */
  reason: string;
}

/**
 * A file the search palette asked to be opened, held until the Run tab has it.
 *
 * `token` is what makes the request re-fire: choosing a symbol in a file that is
 * *already* open changes no field a consumer could compare, and a number that
 * only ever goes up cannot collide with itself.
 */
export interface OpenFileRequest {
  /** Workspace-relative, as `SearchHit.path` gives it. */
  path: string;
  /** The file name for the editor tab. */
  name: string;
  /** 1-based line to reveal, when the hit named one. */
  line?: number;
  token: number;
}

/**
 * A configuration the palette asked to be selected — selected, not started.
 * Starting a process off a fuzzy-matched keystroke is the kind of guess this app
 * refuses; selecting puts the configuration under the Run button instead.
 */
export interface SelectConfigRequest {
  configId: string;
  token: number;
}

/** True when running inside the Tauri webview (false in a plain browser tab). */
const inTauri = "__TAURI_INTERNALS__" in window;

export function App() {
  // Every open codebase, and which one is in the foreground. Identity is `root`.
  const [openWorkspaces, setOpenWorkspaces] = useState<Workspace[]>([]);
  const [activeRoot, setActiveRoot] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [recents, setRecents] = useState<string[]>(() => loadRecents(localStorage));
  const [loading, setLoading] = useState(true);
  const [notesOpen, setNotesOpen] = useState(false);
  // The Running panel and the report it renders. The report is polled here (not
  // in the panel) so the titlebar badge stays live even while the panel is
  // closed; `list_running` is a cheap in-memory read.
  const [runningOpen, setRunningOpen] = useState(false);
  const [runningReport, setRunningReport] = useState<RunningReport | null>(null);
  // Per-codebase terminal-attention flag, so a background tab can flash to show
  // which project a minimized terminal's bell is coming from.
  const [attentionByRoot, setAttentionByRoot] = useState<Record<string, boolean>>({});

  const activeWorkspace = openWorkspaces.find((w) => w.root === activeRoot) ?? null;

  /**
   * Each open tab registers an action handle here; the titlebar and the global
   * Notes panel invoke the *foreground* tab's handle. A ref (not state) because
   * these are imperative one-shot calls, not something the render reads.
   */
  const tabHandles = useRef(new Map<string, WorkspaceTabHandle>());
  const registerTab = useCallback((root: string, handle: WorkspaceTabHandle | null) => {
    if (handle) tabHandles.current.set(root, handle);
    else tabHandles.current.delete(root);
  }, []);
  const activeHandle = () => (activeRoot ? tabHandles.current.get(activeRoot) : undefined);

  /** A rescan or config-save handed back a fresh workspace; replace it in place. */
  const onWorkspaceChange = useCallback((ws: Workspace) => {
    setOpenWorkspaces((list) => list.map((w) => (w.root === ws.root ? ws : w)));
  }, []);

  /** Re-read the running set (for the badge and the panel). */
  const refreshRunning = useCallback(() => {
    api.listRunning().then(setRunningReport).catch(() => {});
  }, []);

  /** Kill one process from the panel, then refresh. A refusal (a reused pid) is
   *  surfaced as the app error banner. */
  const killRunningEntry = useCallback(
    (req: Parameters<typeof api.killRunning>[0]) => {
      api
        .killRunning(req)
        .catch((e) => setError(api.errorMessage(e)))
        .finally(refreshRunning);
    },
    [refreshRunning],
  );

  // Poll the running set on a steady cadence so the titlebar badge stays live
  // even while the panel is closed; `list_running` is a cheap in-memory read.
  useEffect(() => {
    refreshRunning();
    const timer = setInterval(refreshRunning, 2000);
    return () => clearInterval(timer);
  }, [refreshRunning]);

  /**
   * The editor font size: restored on start and driven by Ctrl+= / Ctrl+- /
   * Ctrl+0 from anywhere. App-wide, so it lives here rather than in a tab.
   */
  useEffect(() => {
    applyEditorFontSize(loadEditorFontSize());
    const onKeyDown = (event: KeyboardEvent) => {
      const action = recogniseFontSizeShortcut(event);
      if (action === null) return;
      event.preventDefault();
      event.stopPropagation();
      const current = loadEditorFontSize();
      applyEditorFontSize(
        action === "reset" ? DEFAULT_EDITOR_FONT_SIZE : stepFontSize(current, action === "increase" ? 1 : -1),
      );
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

  // The backend keeps every open workspace across a window reload, so the tab
  // strip is rebuilt from it (there is no event channel; identity is `root`).
  useEffect(() => {
    Promise.all([api.listOpenWorkspaces(), api.currentWorkspace()])
      .then(([list, current]) => {
        setOpenWorkspaces(list);
        setActiveRoot(current?.root ?? list[0]?.root ?? null);
      })
      .catch(() => {
        /* nothing open */
      })
      .finally(() => setLoading(false));
  }, []);

  /** Open a folder: add it as a tab (never evicts) and make it active. */
  async function openPath(path: string) {
    try {
      const opened = await api.openWorkspace(path);
      const next = addOpenWorkspace(openWorkspaces, opened);
      setOpenWorkspaces(next.list);
      setActiveRoot(next.activeRoot);
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

  /** Switch tabs: flip the backend's active pointer *before* revealing the tab,
   *  so the newly-foregrounded views never query the previous workspace. */
  async function activateWorkspace(root: string) {
    if (root === activeRoot) return;
    try {
      await api.setActiveWorkspace(root);
      setActiveRoot(root);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  /** Close a tab: tears its backend workspace down, then repoints to a neighbour
   *  (the frontend's tab-order choice, which the backend is realigned to). */
  async function closeWorkspace(root: string) {
    try {
      await api.closeWorkspace(root);
    } catch (e) {
      setError(api.errorMessage(e));
    }
    const next = closeOpenWorkspace(openWorkspaces, root, activeRoot);
    if (next.activeRoot && next.activeRoot !== activeRoot) {
      await api.setActiveWorkspace(next.activeRoot).catch(() => {});
    }
    setOpenWorkspaces(next.list);
    setActiveRoot(next.activeRoot);
    setAttentionByRoot(({ [root]: _closed, ...rest }) => rest);
  }

  /** Rescan the active workspace (re-detect projects/configs), keeping it live. */
  async function rescan() {
    try {
      onWorkspaceChange(await api.rescanWorkspace());
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

  if (openWorkspaces.length === 0) {
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

  const labels = tabLabels(openWorkspaces);

  return (
    <div className="app">
      <div className="titlebar">
        {/* File (Open / Rescan) and Enhancements — the agent actions target the
            foreground tab through its registered handle. */}
        <MenuBar
          onOpen={pickFolder}
          onRescan={rescan}
          onRunAgent={(promptId) => activeHandle()?.openRunAgent(promptId)}
          onOpenReview={() => activeHandle()?.openReview()}
        />

        {activeWorkspace && (
          /* Keyed by the active root, so switching codebases re-reads branches. */
          <BranchMenu key={activeRoot ?? ""} />
        )}

        <div className="spacer" />

        {activeWorkspace && (
          <span className="muted" style={{ fontSize: 11 }}>
            {activeWorkspace.projects.length} project
            {activeWorkspace.projects.length === 1 ? "" : "s"}
          </span>
        )}
        <button onClick={() => setNotesOpen(true)} title="Open the notes / scratchpad panel">
          Notes
        </button>
        <button
          onClick={() => setRunningOpen(true)}
          title="Show everything the app is running (and possible orphans)"
        >
          Running
          {liveCount(runningReport) > 0 && (
            <span className="running-badge">{liveCount(runningReport)}</span>
          )}
        </button>
        <button
          onClick={() => activeHandle()?.openTerminal()}
          title="Open a floating terminal in the active codebase"
        >
          + Terminal
        </button>
        <button onClick={rescan} title="Re-detect projects and configurations">
          Rescan
        </button>
        <button onClick={pickFolder}>Open…</button>
      </div>

      {/* The open-codebases tab strip, above each workspace's own inner tabs. */}
      <div className="tabs ws-tabs">
        {openWorkspaces.map((w, i) => (
          <div
            key={w.root}
            className={`ws-tab ${w.root === activeRoot ? "active" : ""}${
              shouldFlashWorkspaceTab(w.root, activeRoot, attentionByRoot[w.root] ?? false)
                ? " attention"
                : ""
            }`}
          >
            <button
              className="ws-tab-label"
              onClick={() => void activateWorkspace(w.root)}
              title={w.root}
            >
              {labels[i]}
            </button>
            <button
              className="ws-tab-close"
              onClick={() => void closeWorkspace(w.root)}
              title="Close this codebase"
            >
              ×
            </button>
          </div>
        ))}
        <button className="ws-tab-add" onClick={pickFolder} title="Open another codebase">
          +
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      {/* One tab per open codebase, kept mounted; only the active one is visible,
          so a background codebase's processes, terminals and language server keep
          running. */}
      {openWorkspaces.map((w) => (
        <WorkspaceTab
          key={w.root}
          workspace={w}
          active={w.root === activeRoot}
          onWorkspaceChange={onWorkspaceChange}
          onRegister={registerTab}
          onAttentionChange={(root, has) =>
            setAttentionByRoot((prev) => ({ ...prev, [root]: has }))
          }
        />
      ))}

      {/* The global Notes / scratchpad panel — one instance, not per-workspace.
          Its "send to agent" runs in the foreground tab. */}
      {notesOpen && (
        <NotesPanel
          onClose={() => setNotesOpen(false)}
          onSendToAgent={(note) => activeHandle()?.openNoteInAgent(note)}
        />
      )}

      {/* The global Running panel — everything the app is running across all open
          codebases, plus crash-orphan candidates. One instance, open/close only. */}
      {runningOpen && (
        <RunningPanel
          report={runningReport}
          onKill={killRunningEntry}
          onRefresh={refreshRunning}
          onClose={() => setRunningOpen(false)}
        />
      )}

      {/* Bottom status bar: the active codebase's folder name and full path,
          moved here from the titlebar. */}
      <div className="statusbar">
        {activeWorkspace && (
          <>
            <span className="workspace-name">{activeWorkspace.name}</span>
            <span className="faint mono statusbar-path" title={activeWorkspace.root}>
              {activeWorkspace.root}
            </span>
          </>
        )}
      </div>
    </div>
  );
}
