import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AppOutputPanel } from "./components/AppOutputPanel";
import { BranchMenu } from "./components/BranchMenu";
import { LauncherPicker } from "./components/LauncherPicker";
import { FeaturesPicker } from "./components/FeaturesPicker";
import { MenuBar } from "./components/MenuBar";
import { NotesPanel } from "./components/NotesPanel";
import { RunningPanel } from "./components/RunningPanel";
import { liveCount } from "./components/runningLogic";
import {
  addTab,
  applyEvent,
  closeTab,
  liveTabCount,
  makeTab,
  setTabSeverity,
  type AppTab,
} from "./components/appOutputLogic";
import type { Severity } from "./components/consoleLogic";
import type { ConsoleHandle } from "./components/OutputConsole";
import { WorkspaceTab, type WorkspaceTabHandle } from "./components/WorkspaceTab";
import { SettingsDialog } from "./components/SettingsDialog";
import {
  addOpenWorkspace,
  closeOpenWorkspace,
  mergeSignal,
  tabLabels,
  tabSignalClass,
} from "./components/workspaceTabsLogic";
import type { TabSignal } from "./components/workspaceTabsLogic";
import * as api from "./ipc/api";

/**
 * How long a `done` signal stays on a tab: two runs of the 0.9s `ws-tab-flash`
 * animation, plus enough slack that the class outlives the last frame rather
 * than cutting it. "A terminal finished" is worth a glance and nothing more, so
 * unlike the other three signals it expires without being acknowledged.
 */
const DONE_SIGNAL_MS = 1900;
import { applyAppearance, loadAppearance } from "./appearance";
import { dispatchShortcut, registerCommand } from "./shortcuts";
import { loadRecents, rememberRecent } from "./recentsLogic";
import type {
  InspectTarget,
  ProcessEvent,
  FeatureInfo,
  RootSpec,
  RunningReport,
  Workspace,
} from "./ipc/types";

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
  const activeRootRef = useRef<string | null>(null);
  activeRootRef.current = activeRoot;
  const [error, setError] = useState<string | null>(null);
  const [recents, setRecents] = useState<string[]>(() => loadRecents(localStorage));
  const [loading, setLoading] = useState(true);
  const [notesOpen, setNotesOpen] = useState(false);
  const [notesRestoreRequest, setNotesRestoreRequest] = useState(0);
  const showNotes = () => {
    setNotesOpen(true);
    setNotesRestoreRequest((request) => request + 1);
  };
  const [settingsOpen, setSettingsOpen] = useState(false);
  /**
   * Which optional features are on. Loaded once at startup — before any
   * workspace can be opened — so `featuresLogic` never has to render against a
   * half-known answer. `null` is "not loaded yet", which `featureEnabled` reads
   * as everything on; see the comment there for why that is the safe direction.
   */
  const [features, setFeatures] = useState<FeatureInfo[] | null>(null);
  const [featuresOpen, setFeaturesOpen] = useState(false);
  // The Running panel and the report it renders. The report is polled here (not
  // in the panel) so the titlebar badge stays live even while the panel is
  // closed; `list_running` is a cheap in-memory read.
  const [runningOpen, setRunningOpen] = useState(false);
  const [runningReport, setRunningReport] = useState<RunningReport | null>(null);
  // The app launcher: the picker overlay, the output panel, and its tabs. All
  // app-level (not per-codebase) because a launched app belongs to no
  // repository - closing the codebase it was started from must not take it down.
  const [launcherOpen, setLauncherOpen] = useState(false);
  const [appOutputOpen, setAppOutputOpen] = useState(false);
  const [appTabs, setAppTabs] = useState<AppTab[]>([]);
  const [activeAppKey, setActiveAppKey] = useState<string | null>(null);
  // Per-codebase terminal-attention flag, so a background tab can flash to show
  // which project a minimized terminal's bell is coming from. Live state, not an
  // event: a terminal is asking for you until it is restored, so this goes back
  // down on its own and is never latched.
  const [attentionByRoot, setAttentionByRoot] = useState<Record<string, boolean>>({});
  /**
   * Per-codebase latched signal — a build that succeeded or failed, or a
   * minimized terminal that finished.
   *
   * Latched, unlike the flag above, because these are events: nothing about the
   * codebase is still true a second later, so there is nothing to derive the
   * display from and the user clears it by clicking the tab. `mergeSignal`
   * decides what survives when two arrive.
   */
  const [signalByRoot, setSignalByRoot] = useState<Record<string, TabSignal>>({});

  const doneTimers = useRef(new Map<string, number>());

  /** Latch a signal onto a codebase's tab, keeping the strongest one showing. */
  const raiseSignal = useCallback((root: string, incoming: TabSignal) => {
    // Events that finish on screen have already told the user. Latching them
    // would make the tab begin flashing only after the user switched away.
    if (root === activeRootRef.current) return;
    setSignalByRoot((prev) => {
      const next = mergeSignal(prev[root] ?? null, incoming);
      return next === prev[root] ? prev : { ...prev, [root]: next };
    });

    const timers = doneTimers.current;
    const pending = timers.get(root);
    if (pending !== undefined) {
      window.clearTimeout(pending);
      timers.delete(root);
    }
    if (incoming !== "done" && incoming !== "success") return;
    timers.set(
      root,
      window.setTimeout(() => {
        timers.delete(root);
        // Only a signal that is *still* `done` expires: anything louder that
        // arrived meanwhile outranked it and is not this timer's to clear.
        setSignalByRoot((prev) => {
          if (prev[root] !== "done" && prev[root] !== "success") return prev;
          const { [root]: _expired, ...rest } = prev;
          return rest;
        });
      }, DONE_SIGNAL_MS),
    );
  }, []);

  /** Drop a codebase's latched signal — it has been seen, or it has gone away. */
  const clearSignal = useCallback((root: string) => {
    const pending = doneTimers.current.get(root);
    if (pending !== undefined) {
      window.clearTimeout(pending);
      doneTimers.current.delete(root);
    }
    setSignalByRoot((prev) => {
      if (!(root in prev)) return prev;
      const { [root]: _seen, ...rest } = prev;
      return rest;
    });
  }, []);

  /**
   * Load the optional-feature set once, at startup. This is also what adopts an
   * installer seed on a first run — `list_features` is the only caller that
   * needs the answer, so the seeding hangs off it rather than a separate step.
   *
   * A failure leaves `features` at `null`, which reads as everything enabled: a
   * preferences file that cannot be read must never make the app look broken.
   */
  useEffect(() => {
    let live = true;
    api
      .listFeatures()
      .then((list) => {
        if (live) setFeatures(list);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    const timers = doneTimers.current;
    return () => {
      for (const timer of timers.values()) window.clearTimeout(timer);
      timers.clear();
    };
  }, []);

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

  /**
   * Each launched app's console, and the output that arrived before it mounted.
   *
   * Refs, not state: this is an imperative write-through to xterm. The buffer
   * exists because the first bytes of output can arrive in the same tick the tab
   * is added, before React has mounted its console - dropping them would lose
   * exactly the lines that say why a mistyped command failed.
   */
  const appConsoles = useRef(new Map<string, ConsoleHandle>());
  const appPending = useRef(new Map<string, ProcessEvent[]>());
  const appWorkspaceRoots = useRef(new Map<string, string>());

  const registerAppConsole = useCallback((key: string, handle: ConsoleHandle | null) => {
    if (!handle) {
      appConsoles.current.delete(key);
      return;
    }
    appConsoles.current.set(key, handle);
    const queued = appPending.current.get(key);
    if (queued) {
      for (const event of queued) handle.handle(event);
      appPending.current.delete(key);
    }
  }, []);

  /** Route one process event to its tab's console and status. */
  const onAppEvent = useCallback((key: string, event: ProcessEvent) => {
    const handle = appConsoles.current.get(key);
    if (handle) {
      handle.handle(event);
    } else {
      const queued = appPending.current.get(key) ?? [];
      queued.push(event);
      appPending.current.set(key, queued);
    }
    const root = appWorkspaceRoots.current.get(key);
    if (root && event.type === "exited" && !event.cancelled) {
      raiseSignal(root, event.success ? "success" : "error");
    } else if (root && event.type === "failed") {
      raiseSignal(root, "error");
    }
    setAppTabs((tabs) => applyEvent(tabs, key, event));
  }, [raiseSignal]);

  /** Launch a command from the picker: open a tab for it, then start it. */
  const launchApp = useCallback(
    (spec: { command: string; cwd: string; shell: boolean; label?: string }) => {
      // The key is minted here, not by the backend: output starts arriving the
      // moment the process spawns, which is before `launchCommand` resolves.
      const key = `ext:${crypto.randomUUID()}`;
      const placeholder = makeTab(
        {
          key,
          id: key,
          label: spec.label?.trim() || spec.command,
          cwd: spec.cwd,
        },
        activeRootRef.current,
      );
      const added = addTab(appTabs, placeholder);
      if (placeholder.workspaceRoot) appWorkspaceRoots.current.set(key, placeholder.workspaceRoot);
      setAppTabs(added.tabs);
      setActiveAppKey(added.activeKey);
      setAppOutputOpen(true);

      api
        .launchCommand({ ...spec, key }, (event) => onAppEvent(key, event))
        .then((app) => {
          // Adopt the backend's label (it applies an earlier rename) and the id
          // of the recents entry, which the panel addresses pin/rename by.
          setAppTabs((tabs) =>
            tabs.map((t) => (t.key === key ? { ...t, label: app.label, entryId: app.id } : t)),
          );
          refreshRunning();
        })
        .catch((e) => {
          const message = api.errorMessage(e);
          setError(message);
          // A command line that could not even be resolved never became a
          // process, so its tab would otherwise sit "running" for ever.
          onAppEvent(key, { type: "failed", message });
        });
    },
    [appTabs, onAppEvent, refreshRunning],
  );

  /** Stop a launched app, leaving its tab and its output in place. */
  const stopApp = useCallback(
    (key: string) => {
      api
        .stopCommand(key)
        .catch((e) => setError(api.errorMessage(e)))
        .finally(refreshRunning);
    },
    [refreshRunning],
  );

  /** Close an output tab, stopping the process first when it is still running. */
  const closeAppTab = useCallback(
    (key: string) => {
      const tab = appTabs.find((t) => t.key === key);
      if (tab && tab.status.kind === "running") {
        if (!window.confirm(`"${tab.label}" is still running. Stop it and close this tab?`)) {
          return;
        }
        stopApp(key);
      }
      const result = closeTab(appTabs, key, activeAppKey);
      setAppTabs(result.tabs);
      setActiveAppKey(result.activeKey);
      appConsoles.current.delete(key);
      appPending.current.delete(key);
      appWorkspaceRoots.current.delete(key);
      if (result.tabs.length === 0) setAppOutputOpen(false);
    },
    [appTabs, activeAppKey, stopApp],
  );

  /** The Running panel's View action: focus a launched app's output tab. */
  const viewAppOutput = useCallback((key: string) => {
    setActiveAppKey(key);
    setAppOutputOpen(true);
  }, []);

  // Poll the running set on a steady cadence so the titlebar badge stays live
  // even while the panel is closed; `list_running` is a cheap in-memory read.
  useEffect(() => {
    refreshRunning();
    const timer = setInterval(refreshRunning, 2000);
    return () => clearInterval(timer);
  }, [refreshRunning]);

  /** Apply user-global appearance and route every configurable shortcut. */
  useEffect(() => {
    applyAppearance(loadAppearance(), false);
    const onKeyDown = (event: KeyboardEvent) => {
      if (!dispatchShortcut(event)) return;
      event.preventDefault();
      event.stopPropagation();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

  useEffect(() => {
    const registrations = [
      registerCommand("file.open", pickFolder),
      registerCommand("file.rescan", () => void rescan()),
      registerCommand("file.settings", () => setSettingsOpen(true)),
      registerCommand("panel.notes", showNotes),
      registerCommand("panel.launch", () => setLauncherOpen(true)),
      registerCommand("panel.apps", () => setAppOutputOpen(true)),
      registerCommand("panel.running", () => setRunningOpen(true)),
      registerCommand("terminal.new", () => activeHandle()?.openTerminal()),
      registerCommand("agent.review", () => activeHandle()?.openReview()),
      registerCommand("project.next", () => switchWorkspace(1)),
      registerCommand("project.previous", () => switchWorkspace(-1)),
      registerCommand("project.close", () => { if (activeRootRef.current) void closeWorkspace(activeRootRef.current); }),
      ...[[-1, "decrease"], [1, "increase"], [0, "reset"]].map(([delta, name]) => registerCommand(`font.code.${name}`, () => {
        const settings = loadAppearance();
        settings.codeFontSize = delta === 0 ? 12.5 : Math.min(32, Math.max(8, Math.round(settings.codeFontSize) + Number(delta)));
        applyAppearance(settings, true);
      })),
      ...[[-1, "decrease"], [1, "increase"], [0, "reset"]].map(([delta, name]) => registerCommand(`font.ui.${name}`, () => {
        const settings = loadAppearance();
        settings.uiFontSize = delta === 0 ? 13 : Math.min(24, Math.max(10, settings.uiFontSize + Number(delta)));
        applyAppearance(settings, true);
      })),
    ];
    return () => registrations.forEach((unregister) => unregister());
    // Handlers read current mutable refs or are intentionally rebound with state.
  });

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
    // Looking at the codebase is the acknowledgement: this is the "until
    // clicked" in the signal's promise.
    clearSignal(root);
    try {
      await api.setActiveWorkspace(root);
      setActiveRoot(root);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  function switchWorkspace(direction: -1 | 1) {
    if (openWorkspaces.length < 2) return;
    const current = openWorkspaces.findIndex((workspace) => workspace.root === activeRootRef.current);
    const next = openWorkspaces[(current + direction + openWorkspaces.length) % openWorkspaces.length];
    if (next) void activateWorkspace(next.root);
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
    clearSignal(root);
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
          onOpenFeatures={() => setFeaturesOpen(true)}
          onOpenSettings={() => setSettingsOpen(true)}
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
        <button onClick={showNotes} title="Open or restore the notes / scratchpad panel">
          Notes
        </button>
        <button
          onClick={() => setLauncherOpen(true)}
          title="Run another app or command, and see what you have run before"
        >
          Launch
        </button>
        {appTabs.length > 0 && (
          <button
            onClick={() => setAppOutputOpen(true)}
            title="Show the output of the apps you launched"
          >
            Apps
            {liveTabCount(appTabs) > 0 && (
              <span className="running-badge">{liveTabCount(appTabs)}</span>
            )}
          </button>
        )}
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
            className={`ws-tab ${w.root === activeRoot ? "active" : ""}${tabSignalClass(
              w.root,
              activeRoot,
              // A ringing bell is live state and outranks nothing it is folded
              // into; a latched signal keeps showing once it stops ringing.
              attentionByRoot[w.root]
                ? mergeSignal(signalByRoot[w.root], "attention")
                : (signalByRoot[w.root] ?? null),
            )}`}
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
          onSignal={raiseSignal}
          features={features}
        />
      ))}

      {/* The global Notes / scratchpad panel — one instance, not per-workspace.
          Its "send to agent" runs in the foreground tab. */}
      {featuresOpen && (
        <FeaturesPicker
          features={features}
          onChange={setFeatures}
          onClose={() => setFeaturesOpen(false)}
        />
      )}

      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}

      {notesOpen && (
        <NotesPanel
          restoreRequest={notesRestoreRequest}
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
          onViewOutput={viewAppOutput}
          onClose={() => setRunningOpen(false)}
        />
      )}

      {/* The app launcher's picker: an overlay, closed as soon as it launches. */}
      {launcherOpen && (
        <LauncherPicker
          root={activeRoot}
          onLaunch={launchApp}
          onClose={() => setLauncherOpen(false)}
        />
      )}

      {/* The launched apps' output. Mounted while any tab exists - hidden, never
          unmounted, when the panel is closed - because unmounting would discard
          the scrollback of a process that is still running. */}
      {appTabs.length > 0 && (
        <AppOutputPanel
          tabs={appTabs}
          activeKey={activeAppKey}
          hidden={!appOutputOpen}
          onSelect={setActiveAppKey}
          onCloseTab={closeAppTab}
          onStop={stopApp}
          onClose={() => setAppOutputOpen(false)}
          onSeverityChange={(key: string, severity: Severity) =>
            setAppTabs((tabs) => setTabSeverity(tabs, key, severity))
          }
          registerConsole={registerAppConsole}
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
