import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import { ConfigEditor } from "../components/ConfigEditor";
import { FileEditor } from "../components/FileEditor";
import { FileTree } from "../components/FileTree";
import { RiderImportDialog } from "../components/RiderImportDialog";
import { RunConfigMenu } from "../components/RunConfigMenu";
import { SecretsEditor } from "../components/SecretsEditor";
import { Sidebar } from "../components/Sidebar";
import {
  EnvironmentPicker,
  type EnvironmentState,
} from "../components/EnvironmentPicker";
import * as api from "../ipc/api";
import type {
  BuildAction,
  ProcessEvent,
  RunConfig,
  Workspace,
} from "../ipc/types";

/** One console tab: a run or build whose output has its own terminal. */
interface ConsoleSession {
  id: string;
  label: string;
}

/** What a tab's status icon shows. */
type SessionStatus = "running" | "ok" | "fail" | "stopped";

/**
 * Output lines that mean a long-running app is *up*, even though its process
 * will not exit until stopped: ASP.NET's startup lines, plus the ready lines
 * of common dev servers.
 */
const APP_UP = /Now listening on|Application started|ready in|Local:/i;

/** The config behind a session id (build sessions are `<config>:build`). */
const sessionConfigId = (sessionId: string) => sessionId.replace(/:build$/, "");

/**
 * The .NET environment picker's saved state, per workspace. Kept in
 * localStorage rather than `.code-basics/config.json`: which environment a
 * developer runs against is personal, and the config file is checked in.
 */
const DEFAULT_ENVIRONMENTS: EnvironmentState = {
  options: ["Development"],
  selected: "Development",
};

const environmentsKey = (root: string) => `code-basics.environments:${root}`;

/** A file open in the editor pane. */
interface OpenFile {
  /** Workspace-relative path — the file's identity. */
  path: string;
  name: string;
}

/** Editor pane height as a fraction of the main area, shared across workspaces. */
const SPLIT_KEY = "code-basics.editorSplit";

function loadSplit(): number {
  const stored = Number(localStorage.getItem(SPLIT_KEY));
  return Number.isFinite(stored) && stored > 0.1 && stored < 0.9 ? stored : 0.55;
}

function loadEnvironments(root: string): EnvironmentState {
  try {
    const raw = localStorage.getItem(environmentsKey(root));
    if (!raw) return DEFAULT_ENVIRONMENTS;
    const parsed = JSON.parse(raw) as EnvironmentState;
    return Array.isArray(parsed.options) ? parsed : DEFAULT_ENVIRONMENTS;
  } catch {
    return DEFAULT_ENVIRONMENTS;
  }
}

export function RunView({
  workspace,
  onWorkspaceChange,
}: {
  workspace: Workspace;
  onWorkspaceChange: (workspace: Workspace) => void;
}) {
  const appConfigs = workspace.configs.filter((c) => c.kind === "app");

  /**
   * Which solution a configuration's project belongs to.
   *
   * Only meaningful once a workspace holds more than one solution — with a
   * single one the label would be on every row and say nothing.
   */
  const solutionOf = (config: RunConfig): string | null => {
    if (workspace.solutions.length < 2 || !config.project) return null;
    // Solution members are stored with forward slashes; a config's project
    // path can carry whatever separator wrote it.
    const target = config.project.replace(/\\/g, "/");
    return (
      workspace.solutions.find((s) =>
        s.projects.some((p) => p.path.replace(/\\/g, "/") === target),
      )?.name ?? null
    );
  };

  const [selectedId, setSelectedId] = useState<string | null>(
    appConfigs[0]?.id ?? null,
  );
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<RunConfig | null>(null);
  const [importing, setImporting] = useState(false);
  const [secretsFor, setSecretsFor] = useState<RunConfig | null>(null);
  const [environments, setEnvironments] = useState<EnvironmentState>(() =>
    loadEnvironments(workspace.root),
  );
  const [building, setBuilding] = useState(false);
  const [sessions, setSessions] = useState<ConsoleSession[]>([]);
  const [activeSession, setActiveSession] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<Record<string, SessionStatus>>({});

  // The editor pane: files opened from the directory tree.
  const [openFiles, setOpenFiles] = useState<OpenFile[]>([]);
  const [activeFile, setActiveFile] = useState<string | null>(null);
  const [dirtyFiles, setDirtyFiles] = useState<Set<string>>(new Set());
  const [split, setSplit] = useState(loadSplit);
  const splitRef = useRef<HTMLDivElement>(null);

  function openFile(path: string, name: string) {
    setOpenFiles((previous) =>
      previous.some((f) => f.path === path) ? previous : [...previous, { path, name }],
    );
    setActiveFile(path);
  }

  function closeFile(path: string) {
    const remaining = openFiles.filter((f) => f.path !== path);
    setOpenFiles(remaining);
    setDirtyFiles((previous) => {
      const next = new Set(previous);
      next.delete(path);
      return next;
    });
    if (activeFile === path) {
      setActiveFile(remaining[remaining.length - 1]?.path ?? null);
    }
  }

  function setFileDirty(path: string, dirty: boolean) {
    setDirtyFiles((previous) => {
      if (previous.has(path) === dirty) return previous;
      const next = new Set(previous);
      if (dirty) next.add(path);
      else next.delete(path);
      return next;
    });
  }

  /** Drag the editor/console divider; the fraction persists across sessions. */
  function startSplitDrag(event: React.MouseEvent) {
    event.preventDefault();
    const host = splitRef.current;
    if (!host) return;
    const { top, height } = host.getBoundingClientRect();

    const fractionAt = (y: number) =>
      Math.min(0.9, Math.max(0.1, (y - top) / height));

    const onMove = (move: MouseEvent) => setSplit(fractionAt(move.clientY));
    const onUp = (up: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      localStorage.setItem(SPLIT_KEY, String(fractionAt(up.clientY)));
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  function setStatus(id: string, status: SessionStatus) {
    setStatuses((previous) => ({ ...previous, [id]: status }));
  }

  /**
   * One terminal per session, all kept mounted (hidden when inactive) so
   * switching tabs never loses scrollback. Events that arrive before a
   * freshly opened tab's terminal has mounted are queued and replayed.
   */
  const consoleRefs = useRef(new Map<string, ConsoleHandle>());
  const pendingEvents = useRef(new Map<string, ProcessEvent[]>());

  function registerConsole(id: string, handle: ConsoleHandle | null) {
    if (!handle) {
      consoleRefs.current.delete(id);
      return;
    }
    consoleRefs.current.set(id, handle);
    for (const event of pendingEvents.current.get(id) ?? []) {
      handle.handle(event);
    }
    pendingEvents.current.delete(id);
  }

  function handleEvent(id: string, event: ProcessEvent) {
    const console = consoleRefs.current.get(id);
    if (console) {
      console.handle(event);
    } else {
      pendingEvents.current.set(id, [
        ...(pendingEvents.current.get(id) ?? []),
        event,
      ]);
    }

    // Keep the tab's status icon in step with the process.
    switch (event.type) {
      case "exited":
        setStatus(id, event.cancelled ? "stopped" : event.success ? "ok" : "fail");
        break;
      case "failed":
        setStatus(id, "fail");
        break;
      case "output":
        // A server is "up" long before its process exits.
        if (APP_UP.test(event.text)) {
          setStatuses((previous) =>
            previous[id] === "running" ? { ...previous, [id]: "ok" } : previous,
          );
        }
        break;
    }
  }

  /** Open (or refocus) the tab for a session, updating its label. */
  function openSession(id: string, label: string) {
    setSessions((previous) =>
      previous.some((s) => s.id === id)
        ? previous.map((s) => (s.id === id ? { ...s, label } : s))
        : [...previous, { id, label }],
    );
    setActiveSession(id);
    setStatus(id, "running");
    consoleRefs.current.get(id)?.clear();
    pendingEvents.current.delete(id);
  }

  function closeSession(id: string) {
    const remaining = sessions.filter((s) => s.id !== id);
    setSessions(remaining);
    consoleRefs.current.delete(id);
    pendingEvents.current.delete(id);
    setStatuses((previous) => {
      const { [id]: _, ...rest } = previous;
      return rest;
    });
    if (activeSession === id) {
      setActiveSession(remaining[remaining.length - 1]?.id ?? null);
    }
  }
  const selected = appConfigs.find((c) => c.id === selectedId) ?? null;
  const favorites = new Set(workspace.favorites);

  /** Secrets only exist for .NET, and only when a project file is targeted. */
  const canEditSecrets = (config: RunConfig | null) =>
    config?.ecosystem === "dotnet" && !!config.project;

  // Processes outlive a view switch, so reconcile on mount rather than
  // assuming nothing is running.
  useEffect(() => {
    api
      .runningIds()
      .then((ids) => setRunning(new Set(ids)))
      .catch(() => {
        /* nothing running */
      });
  }, []);

  // A different workspace has its own environment list.
  useEffect(() => {
    setEnvironments(loadEnvironments(workspace.root));
  }, [workspace.root]);

  function saveEnvironments(next: EnvironmentState) {
    setEnvironments(next);
    localStorage.setItem(environmentsKey(workspace.root), JSON.stringify(next));
  }

  async function runBuild(config: RunConfig, action: BuildAction) {
    const session = `${config.id}:build`;
    setError(null);
    setBuilding(true);
    openSession(session, `${config.name} · ${action}`);
    setRunning((previous) => new Set(previous).add(session));

    try {
      await api.buildProject(config.id, action, (event) =>
        handleEvent(session, event),
      );
    } catch (e) {
      setError(api.errorMessage(e));
      setStatus(session, "fail");
    } finally {
      setBuilding(false);
      setRunning((previous) => {
        const next = new Set(previous);
        next.delete(session);
        return next;
      });
    }
  }

  async function start(config: RunConfig) {
    setError(null);
    setSelectedId(config.id);
    openSession(config.id, config.name);
    setRunning((previous) => new Set(previous).add(config.id));

    // The picker only applies to .NET, and "" means run the config as-is.
    const env =
      config.ecosystem === "dotnet" && environments.selected
        ? { ASPNETCORE_ENVIRONMENT: environments.selected }
        : undefined;

    try {
      await api.startRun(config.id, (event) => handleEvent(config.id, event), env);
    } catch (e) {
      setError(api.errorMessage(e));
      setStatus(config.id, "fail");
    } finally {
      setRunning((previous) => {
        const next = new Set(previous);
        next.delete(config.id);
        return next;
      });
    }
  }

  async function stop(config: RunConfig) {
    await api.cancelRun(config.id);
  }

  async function save(config: RunConfig) {
    try {
      onWorkspaceChange(await api.saveConfig(config));
      setEditing(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function remove(config: RunConfig) {
    try {
      onWorkspaceChange(await api.deleteConfig(config.id));
      setEditing(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function toggleFavorite(config: RunConfig) {
    try {
      onWorkspaceChange(await api.setFavorite(config.id, !favorites.has(config.id)));
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  /**
   * The sidebar dot for a configuration: grey when idle, yellow while
   * building or starting up, green once the app is up, red when the build or
   * start-up failed. Reuses the test-outcome dot colours.
   */
  function dotClass(config: RunConfig): string {
    const runStatus = statuses[config.id];
    const buildStatus = statuses[`${config.id}:build`];

    if (running.has(`${config.id}:build`)) return "skipped"; // yellow: building
    if (running.has(config.id)) {
      return runStatus === "ok" ? "passed" : "skipped"; // green: up, yellow: starting
    }
    if (runStatus === "fail" || buildStatus === "fail") return "failed"; // red
    return "other"; // grey: nothing running
  }

  /**
   * The id of the config `delta` places away in this list, staying within the
   * same group — favourites are pinned above the rest, so moving across the
   * boundary could never change what is displayed.
   */
  function neighborId(config: RunConfig, delta: -1 | 1): string | null {
    const group = appConfigs
      .filter((c) => favorites.has(c.id) === favorites.has(config.id))
      .map((c) => c.id);
    return group[group.indexOf(config.id) + delta] ?? null;
  }

  /**
   * Reposition `config` just before/after its visible neighbour, persisting
   * the *full* config id order so test configurations keep their places too.
   */
  async function move(config: RunConfig, delta: -1 | 1) {
    const neighbor = neighborId(config, delta);
    if (!neighbor) return;

    const order = workspace.configs.map((c) => c.id).filter((id) => id !== config.id);
    const at = order.indexOf(neighbor) + (delta === 1 ? 1 : 0);
    order.splice(at, 0, config.id);

    try {
      onWorkspaceChange(await api.setConfigOrder(order));
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  return (
    <>
      {/* Lives in the titlebar (portal), next to the branch widget. */}
      <RunConfigMenu
        configs={appConfigs}
        selectedId={selectedId}
        favorites={favorites}
        dotClass={dotClass}
        canMove={(config, delta) => neighborId(config, delta) !== null}
        groupLabel={solutionOf}
        onSelect={(config) => {
          setSelectedId(config.id);
          // Selecting something that has a console tab focuses that tab.
          if (sessions.some((s) => s.id === config.id)) {
            setActiveSession(config.id);
          }
        }}
        onToggleFavorite={(config) => void toggleFavorite(config)}
        onMove={(config, delta) => void move(config, delta)}
        onNew={() =>
          setEditing({
            id: `custom:${Date.now()}`,
            name: "New configuration",
            kind: "app",
            ecosystem: "dotnet",
            source: "userFile",
          })
        }
        onImport={() => setImporting(true)}
      />

      <Sidebar>
        <div className="group-label">Files</div>
        <FileTree
          refreshToken={workspace}
          activePath={activeFile}
          onOpenFile={openFile}
        />
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <button
            className="primary"
            onClick={() => selected && start(selected)}
            disabled={!selected || running.has(selected.id)}
          >
            Run
          </button>
          <button
            onClick={() => selected && stop(selected)}
            disabled={!selected || !running.has(selected.id)}
          >
            Stop
          </button>
          <button
            onClick={() => selected && start(selected)}
            disabled={!selected}
            title="Stop and start again"
          >
            Restart
          </button>
          <button
            onClick={() =>
              activeSession && consoleRefs.current.get(activeSession)?.clear()
            }
            disabled={!activeSession}
            title="Clear this tab's console output"
          >
            Clear
          </button>

          <button
            onClick={() => selected && runBuild(selected, "build")}
            disabled={selected?.ecosystem !== "dotnet" || building}
            title="Build the project"
          >
            🔨
          </button>
          <button
            onClick={() => selected && runBuild(selected, "rebuild")}
            disabled={selected?.ecosystem !== "dotnet" || building}
            title="Rebuild the project (full, non-incremental)"
          >
            ⟳
          </button>
          <button
            onClick={() => selected && runBuild(selected, "clean")}
            disabled={selected?.ecosystem !== "dotnet" || building}
            title="Clean the project's build output"
          >
            🧹
          </button>

          <button onClick={() => selected && setEditing(selected)} disabled={!selected}>
            Edit
          </button>
          <button
            onClick={() => selected && setSecretsFor(selected)}
            disabled={!canEditSecrets(selected)}
            title={
              canEditSecrets(selected)
                ? "Edit this project's .NET user secrets"
                : "User secrets are available for .NET configurations with a project"
            }
          >
            Secrets…
          </button>
          {selected?.ecosystem === "dotnet" && (
            <EnvironmentPicker state={environments} onChange={saveEnvironments} />
          )}

          <span style={{ flex: 1 }} />
          {selected && (
            <span className="muted mono" style={{ fontSize: 11 }}>
              {selected.ecosystem}
              {selected.buildConfiguration ? ` · ${selected.buildConfiguration}` : ""}
              {selected.launchProfile ? ` · ${selected.launchProfile}` : ""}
              {selected.script ? ` · ${selected.script}` : ""}
            </span>
          )}
        </div>

        {error && <div className="error">{error}</div>}
        {selected?.warnings?.map((warning) => (
          <div className="warning" key={warning}>
            {warning}
          </div>
        ))}

        <div className="content console-area">
          <div className="editor-console-split" ref={splitRef}>
            {openFiles.length > 0 && (
              <>
                <div
                  className="editor-pane"
                  style={{ flex: `0 1 ${Math.round(split * 100)}%` }}
                >
                  <div className="console-tabs">
                    {openFiles.map((file) => (
                      <button
                        key={file.path}
                        className={file.path === activeFile ? "active" : ""}
                        onClick={() => setActiveFile(file.path)}
                        // Middle-click closes, like browser tabs. The
                        // mousedown guard stops the autoscroll cursor.
                        onMouseDown={(e) => e.button === 1 && e.preventDefault()}
                        onAuxClick={(e) => {
                          if (e.button === 1) closeFile(file.path);
                        }}
                        title={file.path}
                      >
                        {dirtyFiles.has(file.path) && (
                          <span className="dirty-dot" title="Unsaved changes — Ctrl+S to save">
                            ●
                          </span>
                        )}
                        {file.name}
                        <span
                          className="row-action"
                          role="button"
                          title={
                            dirtyFiles.has(file.path)
                              ? "Close this file (unsaved changes are discarded)"
                              : "Close this file"
                          }
                          onClick={(e) => {
                            e.stopPropagation();
                            closeFile(file.path);
                          }}
                        >
                          ×
                        </span>
                      </button>
                    ))}
                  </div>
                  <div className="editor-area">
                    {openFiles.map((file) => (
                      <div
                        key={file.path}
                        style={{
                          display: file.path === activeFile ? "block" : "none",
                          height: "100%",
                        }}
                      >
                        <FileEditor
                          path={file.path}
                          onDirtyChange={(dirty) => setFileDirty(file.path, dirty)}
                        />
                      </div>
                    ))}
                  </div>
                </div>
                <div
                  className="split-resizer"
                  onMouseDown={startSplitDrag}
                  title="Drag to resize"
                />
              </>
            )}

            <div className="console-pane">
              {sessions.length > 0 && (
                <div className="console-tabs">
                  {sessions.map((session) => (
                    <button
                      key={session.id}
                      className={session.id === activeSession ? "active" : ""}
                      // Middle-click closes, like browser tabs (the process,
                      // as with the × control, keeps running).
                      onMouseDown={(e) => e.button === 1 && e.preventDefault()}
                      onAuxClick={(e) => {
                        if (e.button === 1) closeSession(session.id);
                      }}
                      onClick={() => {
                        setActiveSession(session.id);
                        // The tab's project becomes the active one, so the toolbar
                        // buttons act on what the user is looking at.
                        const configId = sessionConfigId(session.id);
                        if (appConfigs.some((c) => c.id === configId)) {
                          setSelectedId(configId);
                        }
                      }}
                    >
                      {statuses[session.id] === "running" && <span className="spinner" />}
                      {statuses[session.id] === "ok" && (
                        <span className="tab-status ok">✓</span>
                      )}
                      {statuses[session.id] === "fail" && (
                        <span className="tab-status fail">✕</span>
                      )}
                      {statuses[session.id] === "stopped" && (
                        <span className="tab-status stopped">■</span>
                      )}
                      {session.label}
                      <span
                        className="row-action"
                        role="button"
                        title="Close this tab (a running process keeps running)"
                        onClick={(e) => {
                          e.stopPropagation();
                          closeSession(session.id);
                        }}
                      >
                        ×
                      </span>
                    </button>
                  ))}
                </div>
              )}

              <div className="console-sessions">
                {sessions.length === 0 && (
                  <div className="empty">Run a configuration to see its output here.</div>
                )}
                {sessions.map((session) => (
                  <div
                    key={session.id}
                    style={{
                      display: session.id === activeSession ? "block" : "none",
                      height: "100%",
                    }}
                  >
                    <OutputConsole ref={(handle) => registerConsole(session.id, handle)} />
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      {editing && (
        <ConfigEditor
          config={editing}
          workspace={workspace}
          onCancel={() => setEditing(null)}
          onSave={save}
          // Detected configurations cannot be deleted (they reappear on the
          // next scan), and a brand-new draft has nothing to delete yet.
          onDelete={
            editing.source !== "detected" &&
            workspace.configs.some((c) => c.id === editing.id)
              ? () => void remove(editing)
              : undefined
          }
        />
      )}

      {secretsFor?.project && (
        <SecretsEditor
          project={secretsFor.project}
          projectName={secretsFor.project.split(/[\\/]/).pop() ?? secretsFor.name}
          onClose={() => setSecretsFor(null)}
        />
      )}

      {importing && (
        <RiderImportDialog
          onClose={() => setImporting(false)}
          onImported={(updated) => {
            onWorkspaceChange(updated);
            setImporting(false);
          }}
        />
      )}
    </>
  );
}
