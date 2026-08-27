import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import { ConfigEditor } from "../components/ConfigEditor";
import { projectTarget } from "../components/configLogic";
import { FileEditor } from "../components/FileEditor";
import { FileTree } from "../components/FileTree";
import { LspStatusIndicator } from "../components/LspStatus";
import { RiderImportDialog } from "../components/RiderImportDialog";
import { RunConfigMenu } from "../components/RunConfigMenu";
import { secretsFile, secretsProjects, type OpenEditorFile } from "../components/editorSourceLogic";
import { Sidebar } from "../components/Sidebar";
import {
  EnvironmentPicker,
  type EnvironmentState,
} from "../components/EnvironmentPicker";
import * as api from "../ipc/api";
import {
  loadCollapsed,
  loadSplit,
  saveCollapsed,
  saveSplit,
} from "./consolePanelLogic";
import {
  navBack,
  navForward,
  navMouseAction,
  partitionTabs,
  pushNav,
  togglePin,
  type NavEntry,
  type NavHistory,
} from "./editorNavLogic";
import { preferApplicationProcess } from "./inspectLogic";
import type { InspectRequest, OpenFileRequest, SelectConfigRequest } from "../App";
import type {
  AttachableProcess,
  BuildAction,
  InspectStatus,
  ProcessEvent,
  RunConfig,
  RunDump,
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
 * What a session knows that the Inspect affordances need.
 *
 * The status icon is not enough: it collapses a crash and a failed spawn into
 * the same red tick, and neither it nor `running` remembers the pid. All of
 * this is per session, because two configurations can be up at once.
 */
interface SessionInspect {
  /** Unix seconds. Nothing captured before this can belong to the session. */
  startedAt: number;
  /** The pid the process reported, when it reported one. */
  pid?: number;
  /** How it ended. Absent while it is still running. */
  exit?: { code: number | null; success: boolean; cancelled: boolean };
  /**
   * A dump that turned up for this session, with whether it is certainly this
   * session's. Attribution is the backend's call — see `inspect_run_dump`.
   */
  runDump?: RunDump;
}

/** How many instances a live type root asks for; the Objects tab's default. */
const LIVE_TYPE_LIMIT = 50;

const nowSeconds = () => Math.floor(Date.now() / 1000);

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
/**
 * An open editor tab. The identity is `file.id` — a workspace-relative path for
 * an ordinary file, or `secrets:<project>` for a secrets tab — and every per-file
 * map (active, dirty, pinned, reveal) keys on it. See `editorSourceLogic`.
 */
type OpenFile = OpenEditorFile;

/** The tab label for a path, from either separator it may carry. */
function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
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
  onInspect,
  pendingOpen,
  onOpenConsumed,
  pendingSelect,
  onSelectConsumed,
  onNavigate,
  active,
}: {
  workspace: Workspace;
  onWorkspaceChange: (workspace: Workspace) => void;
  onInspect: (request: InspectRequest) => void;
  /**
   * Whether the Run tab is the one on screen. The back/forward mouse buttons
   * only act while it is: this view stays mounted when hidden, and moving the
   * active file behind another tab would change what the user sees the next
   * time they return here, with no visible cause.
   */
  active: boolean;
  /** A file the search palette chose; see `OpenFileRequest` in `App.tsx`. */
  pendingOpen?: OpenFileRequest | null;
  onOpenConsumed?: () => void;
  /**
   * An editor asking to open another file at a line — Go to definition, or a
   * usage row.
   *
   * Deliberately routed *up* to `App.requestOpenFile` and back down as
   * `pendingOpen`, rather than calling this view's own `openFile` directly. The
   * reveal is keyed on `OpenFileRequest.token`, and that token is `App`'s
   * monotonic counter: a jump into the file the user is already looking at
   * changes no other field, so anything short of a fresh token would open the
   * tab that is already open and leave the cursor where it was. One counter,
   * one path in — see the comment on `OpenFileRequest`.
   */
  onNavigate?: (path: string, name: string, line?: number) => void;
  /** A configuration the search palette chose. Selected, never started. */
  pendingSelect?: SelectConfigRequest | null;
  onSelectConsumed?: () => void;
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

  /**
   * Projects the scan listed but could not read.
   *
   * `unreadable` is **optional, not nullable** — a healthy project has no such
   * key — so the test is a plain truthiness check and a missing key means
   * healthy. These used to be dropped from the scan entirely, which is why
   * they are shown rather than filtered again here: a shorter list looks
   * exactly like a correct one, and the developer whose manifest will not
   * parse is the one person who needs to be told.
   */
  const unreadableProjects = workspace.projects.filter((p) => p.unreadable);

  /**
   * Why the project a configuration targets could not be read, if that is the
   * situation.
   *
   * Only a saved configuration can be in this state: a detected one is derived
   * from a project the scan read, and a project it could not read produces
   * none. The comparison is `projectTarget`'s — the same workspace-relative
   * string `ConfigEditor` writes into `config.project` — with separators
   * normalised, because the saved file can carry whichever separator wrote it.
   * An exact match on that path is the project, so this cannot grey out a
   * configuration that points somewhere else.
   */
  const unreadableTarget = (config: RunConfig | null): string | null => {
    if (!config?.project) return null;
    const target = config.project.replace(/\\/g, "/");
    const project = workspace.projects.find(
      (p) => projectTarget(p, workspace.root).replace(/\\/g, "/") === target,
    );
    return project?.unreadable ?? null;
  };

  const [selectedId, setSelectedId] = useState<string | null>(
    appConfigs[0]?.id ?? null,
  );
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<RunConfig | null>(null);
  const [importing, setImporting] = useState(false);
  const [secretsOpen, setSecretsOpen] = useState(false);
  const [environments, setEnvironments] = useState<EnvironmentState>(() =>
    loadEnvironments(workspace.root),
  );
  const [building, setBuilding] = useState(false);
  const [sessions, setSessions] = useState<ConsoleSession[]>([]);
  const [activeSession, setActiveSession] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<Record<string, SessionStatus>>({});

  // What the Inspect affordances need, per session, plus whether the inspector
  // can do anything at all in this workspace.
  const [inspectInfo, setInspectInfo] = useState<Record<string, SessionInspect>>({});
  const [inspectStatus, setInspectStatus] = useState<InspectStatus | null>(null);
  const [attachable, setAttachable] = useState<AttachableProcess[]>([]);
  const [liveType, setLiveType] = useState("");

  // The editor pane: files opened from the directory tree.
  const [openFiles, setOpenFiles] = useState<OpenFile[]>([]);
  const [activeFile, setActiveFile] = useState<string | null>(null);
  const [dirtyFiles, setDirtyFiles] = useState<Set<string>>(new Set());
  /**
   * Paths shown in the pinned tab row. In-memory only, to match `openFiles`:
   * neither survives a workspace change (this view is keyed by `workspace.root`)
   * and persisting the pins without the files they name would be half a feature.
   */
  const [pinnedFiles, setPinnedFiles] = useState<Set<string>>(new Set());
  const [split, setSplit] = useState(() => loadSplit(localStorage, workspace.root));
  const [consoleCollapsed, setConsoleCollapsed] = useState(() =>
    loadCollapsed(localStorage, workspace.root),
  );
  const splitRef = useRef<HTMLDivElement>(null);

  /**
   * Put the console away, or bring it back.
   *
   * Collapsing hides `.console-sessions` rather than unmounting it: every
   * session's terminal stays mounted while hidden, exactly as switching between
   * session tabs already leaves them, so scrollback and a running process
   * survive. `OutputConsole`'s `ResizeObserver` refits on the way back out.
   */
  function toggleConsole() {
    setConsoleCollapsed((collapsed) => {
      saveCollapsed(localStorage, workspace.root, !collapsed);
      return !collapsed;
    });
  }

  function openFile(path: string, name: string) {
    setOpenFiles((previous) =>
      previous.some((f) => f.id === path)
        ? previous
        : [...previous, { id: path, name, source: { kind: "workspace", path } }],
    );
    setActiveFile(path);
  }

  /**
   * Open a project's user secrets as an ordinary editor tab — the Rider way. The
   * external `secrets.json` and its `<UserSecretsId>` are handled by the write
   * command on first save; opening mutates nothing. Deliberately not recorded in
   * the back/forward history: go-to-definition never lands here, and the stack is
   * for workspace paths only.
   */
  function openSecrets(project: string) {
    const file = secretsFile(project);
    setOpenFiles((previous) =>
      previous.some((f) => f.id === file.id) ? previous : [...previous, file],
    );
    setActiveFile(file.id);
  }

  /**
   * Where an editor has been asked to jump, and under which request.
   *
   * Held as state rather than pushed into the editor directly because the
   * `FileEditor` for a freshly opened file does not exist yet at the moment the
   * request arrives — it is created by the render this same update causes. The
   * token travels with it so the editor can tell a new instruction from a
   * re-render of the old one; see `FileEditor`'s reveal effect.
   */
  const [reveal, setReveal] = useState<{
    path: string;
    line: number;
    token: number;
  } | null>(null);

  /**
   * One monotonic source for every reveal token in this view.
   *
   * A reveal only has to differ from the last one the editor applied to fire
   * again (see `FileEditor`'s reveal effect), so a plain counter owned here
   * serves both a palette-driven open and a back/forward jump without any chance
   * of colliding with `App`'s own `requestToken`.
   */
  const revealSeq = useRef(0);
  const nextRevealToken = () => (revealSeq.current += 1);

  /**
   * The back/forward stack over the files the user has looked at, plus a ref
   * mirror so the window mouse handler — captured once — always reads the
   * latest value. Same idiom as `inspectInfoRef`/`writeInspect` above.
   */
  const [, setNavHistory] = useState<NavHistory>({ entries: [], index: -1 });
  const navHistoryRef = useRef<NavHistory>({ entries: [], index: -1 });
  function writeNav(next: NavHistory) {
    navHistoryRef.current = next;
    setNavHistory(next);
  }

  /** Record a navigation into the history (open, tab switch, or goto jump). */
  function recordNav(path: string, line?: number) {
    writeNav(pushNav(navHistoryRef.current, { path, line }));
  }

  /**
   * Show a history entry without recording it — this *is* the recorded past.
   * A closed file is reopened, browser-like, so Back can still reach it.
   */
  function applyEntry(entry: NavEntry) {
    openFile(entry.path, baseName(entry.path));
    setActiveFile(entry.path);
    setReveal(
      entry.line == null
        ? null
        : { path: entry.path, line: entry.line, token: nextRevealToken() },
    );
  }

  function goBack() {
    const result = navBack(navHistoryRef.current);
    if (!result) return;
    writeNav(result.history);
    applyEntry(result.entry);
  }

  function goForward() {
    const result = navForward(navHistoryRef.current);
    if (!result) return;
    writeNav(result.history);
    applyEntry(result.entry);
  }

  /** Held in a ref so the window listener always calls the current closures. */
  const nav = useRef({ goBack, goForward });
  nav.current = { goBack, goForward };

  /**
   * The browser side mouse buttons drive the editor history while the Run tab
   * is on screen. `preventDefault` on mousedown both stops the Windows
   * autoscroll cursor and suppresses any WebView2 back/forward — the same guard
   * the middle-click handlers use. Capture phase, matching the app's other
   * window-level listeners.
   */
  useEffect(() => {
    if (!active) return;
    const onDown = (event: MouseEvent) => {
      if (navMouseAction(event.button)) event.preventDefault();
    };
    const onAux = (event: MouseEvent) => {
      const action = navMouseAction(event.button);
      if (!action) return;
      event.preventDefault();
      if (action === "back") nav.current.goBack();
      else nav.current.goForward();
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("auxclick", onAux, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("auxclick", onAux, true);
    };
  }, [active]);

  /**
   * Serve a file the palette chose, exactly once.
   *
   * Consumed by object identity in a ref, the way `InspectView` consumes its
   * request: this view stays mounted while hidden, so without the guard every
   * unrelated re-render — a process event, a tab switch, a status tick — would
   * re-open the file and yank the user's cursor back to the line they had since
   * scrolled away from.
   */
  const openConsumed = useRef<OpenFileRequest | null>(null);
  useEffect(() => {
    if (!pendingOpen || openConsumed.current === pendingOpen) return;
    openConsumed.current = pendingOpen;
    openFile(pendingOpen.path, pendingOpen.name);
    setReveal(
      pendingOpen.line == null
        ? null
        : { path: pendingOpen.path, line: pendingOpen.line, token: nextRevealToken() },
    );
    // The palette, the architecture diagram and middle-click go-to-definition
    // all arrive here, so recording the jump here is what feeds every one of
    // them into the back/forward history.
    recordNav(pendingOpen.path, pendingOpen.line);
    onOpenConsumed?.();
  }, [pendingOpen, onOpenConsumed]);

  /**
   * Select a configuration the palette chose. It is not started: see
   * `SelectConfigRequest`.
   *
   * The id is checked against this view's own list before it is used. That list
   * is app configurations only, and setting the selection to an id it does not
   * hold would empty `selected` and disable the whole toolbar, which looks like
   * the app breaking rather than like a row that does not belong on this tab.
   *
   * The palette no longer offers a row this cannot serve — `dropUnactionable`
   * in `searchLogic.ts` filters action hits down to application configurations
   * before drawing them, precisely because a row that closed the palette and
   * then did nothing was indistinguishable from a broken one. What can still
   * arrive is a configuration a rescan removed between the ranking and the
   * keystroke, so the check stays.
   */
  const selectConsumed = useRef<SelectConfigRequest | null>(null);
  useEffect(() => {
    if (!pendingSelect || selectConsumed.current === pendingSelect) return;
    selectConsumed.current = pendingSelect;
    onSelectConsumed?.();
    if (appConfigs.some((c) => c.id === pendingSelect.configId)) {
      setSelectedId(pendingSelect.configId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingSelect, onSelectConsumed]);

  function closeFile(id: string) {
    const remaining = openFiles.filter((f) => f.id !== id);
    setOpenFiles(remaining);
    setDirtyFiles((previous) => {
      const next = new Set(previous);
      next.delete(id);
      return next;
    });
    setPinnedFiles((previous) => {
      if (!previous.has(id)) return previous;
      const next = new Set(previous);
      next.delete(id);
      return next;
    });
    // Closing is not a navigation: the fallback to another tab below must not
    // push onto the history (a browser does not record a tab close as "forward").
    if (activeFile === id) {
      setActiveFile(remaining[remaining.length - 1]?.id ?? null);
    }
  }

  function setFileDirty(id: string, dirty: boolean) {
    setDirtyFiles((previous) => {
      if (previous.has(id) === dirty) return previous;
      const next = new Set(previous);
      if (dirty) next.add(id);
      else next.delete(id);
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
      saveSplit(localStorage, workspace.root, fractionAt(up.clientY));
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  function setStatus(id: string, status: SessionStatus) {
    setStatuses((previous) => ({ ...previous, [id]: status }));
  }

  /**
   * Mirrors `inspectInfo` so the process-event callbacks can read it.
   *
   * Those callbacks are captured once, when the run starts, so the state they
   * close over never sees the pid the `started` event itself recorded. The ref
   * is the current value in both directions.
   */
  const inspectInfoRef = useRef<Record<string, SessionInspect>>({});

  function writeInspect(next: Record<string, SessionInspect>) {
    inspectInfoRef.current = next;
    setInspectInfo(next);
  }

  function patchInspect(id: string, patch: Partial<SessionInspect>) {
    const current = inspectInfoRef.current;
    writeInspect({
      ...current,
      [id]: { startedAt: nowSeconds(), ...current[id], ...patch },
    });
  }

  /**
   * Look for a dump this session may have produced, or leave the affordance off.
   *
   * The runtime writes the dump as the process dies, so it can land a moment
   * after the exit event arrives; one retry covers that without leaving a
   * button that appears seconds late.
   *
   * Which dump — and crucially whether it is *this* session's — is decided by
   * the backend, not here. The dump environment is inherited by every child
   * process and applies to every other configuration running at the same time,
   * so "the newest dump since this run started" is a dump, not this run's dump.
   * Only a matching pid is evidence; anything else comes back with
   * `certain: false` and is described as a candidate.
   */
  async function findDump(id: string, startedAt: number, pid?: number) {
    for (const delay of [0, 1500]) {
      if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));

      let status: InspectStatus;
      try {
        status = await api.inspectStatus();
      } catch {
        return;
      }
      setInspectStatus(status);
      if (!status.available || !status.dumpCaptureEnabled) return;

      try {
        const found = await api.inspectRunDump(pid ?? null, startedAt);
        if (found) {
          patchInspect(id, { runDump: found });
          return;
        }
      } catch {
        return;
      }
    }
  }

  /**
   * Re-read which of our processes are attachable, from the backend.
   *
   * The pid alone is not enough to offer an attach: `dotnet run` starts the
   * application as a child, so the pid this view saw in the `started` event is
   * the .NET CLI. The backend enumerates the machine's .NET processes and
   * attributes each one to a configuration, which is the only place the child
   * can be named; taking the offer from it means the button and the Objects
   * tab agree about the pid, the process and the caveat on it.
   *
   * It costs a sidecar launch, so it is called when the set of running
   * processes is known to have changed — a start, an exit, and mount — and
   * never on a timer.
   */
  async function refreshAttachable() {
    try {
      setAttachable((await api.inspectAttachable()).processes);
    } catch {
      /* the Objects tab reports its own failures; nothing to offer here */
    }
  }

  /**
   * One terminal per session, all kept mounted (hidden when inactive) so
   * switching tabs never loses scrollback. Events that arrive before a
   * freshly opened tab's terminal has mounted are queued and replayed.
   */
  const consoleRefs = useRef(new Map<string, ConsoleHandle>());
  const pendingEvents = useRef(new Map<string, ProcessEvent[]>());

  /** Sessions whose "application is up" line has already prompted one re-read. */
  const appUpSeen = useRef(new Set<string>());

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
      case "started":
        if (event.pid != null) patchInspect(id, { pid: event.pid });
        // The supervisor has just registered it, so the attach offer can now be
        // taken from the backend rather than inferred from this event.
        void refreshAttachable();
        break;
      case "exited": {
        setStatus(id, event.cancelled ? "stopped" : event.success ? "ok" : "fail");
        const { code, success, cancelled } = event;
        patchInspect(id, { exit: { code, success, cancelled } });
        // Gone from the supervisor: an attach offer left standing would aim at
        // a pid the operating system is free to hand to something else.
        void refreshAttachable();
        // A cancelled process was force-killed (`taskkill /T /F`), which never
        // writes a dump, so there is nothing to go looking for. A build that
        // fails is a compiler saying no, not a crash.
        if (!success && !cancelled && !id.endsWith(":build")) {
          const info = inspectInfoRef.current[id];
          void findDump(id, info?.startedAt ?? nowSeconds(), info?.pid);
        }
        break;
      }
      case "failed":
        setStatus(id, "fail");
        break;
      case "output":
        // A server is "up" long before its process exits.
        if (APP_UP.test(event.text)) {
          setStatuses((previous) =>
            previous[id] === "running" ? { ...previous, [id]: "ok" } : previous,
          );
          // `dotnet run` builds first and only then launches the application,
          // so when the supervisor reported its pid the process holding the
          // user's objects did not exist yet and could not be listed. This
          // line is that application saying it is up — the one further moment
          // the list is known to have changed. Once per session: the read runs
          // the sidecar, and a server repeats these lines on every restart.
          if (!appUpSeen.current.has(id)) {
            appUpSeen.current.add(id);
            void refreshAttachable();
          }
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
    // A fresh run launches a fresh application: its "up" line must be allowed
    // to prompt a re-read again.
    appUpSeen.current.delete(id);
    // A new run of the same configuration: the previous run's pid, exit and
    // dump all describe a process that is gone.
    writeInspect({ ...inspectInfoRef.current, [id]: { startedAt: nowSeconds() } });
  }

  function closeSession(id: string) {
    const remaining = sessions.filter((s) => s.id !== id);
    setSessions(remaining);
    consoleRefs.current.delete(id);
    pendingEvents.current.delete(id);
    appUpSeen.current.delete(id);
    setStatuses((previous) => {
      const { [id]: _, ...rest } = previous;
      return rest;
    });
    const { [id]: _discarded, ...remainingInspect } = inspectInfoRef.current;
    writeInspect(remainingInspect);
    if (activeSession === id) {
      setActiveSession(remaining[remaining.length - 1]?.id ?? null);
    }
  }
  const selected = appConfigs.find((c) => c.id === selectedId) ?? null;
  const favorites = new Set(workspace.favorites);

  /**
   * Why the selected configuration cannot be started, when its project is one
   * the scan could not read.
   *
   * Starting it would spawn a runner over the same manifest and fail with the
   * runner's own wording somewhere in a console, several seconds later. The
   * reason is already known here, so it is said here and the buttons that
   * would produce that failure are disabled instead.
   */
  const selectedUnreadable = unreadableTarget(selected);

  // Every .NET project in the workspace can have user secrets, not just the one
  // behind the selected run config — a folder with several projects must be able
  // to reach any of their secrets (see `secretsProjects`).
  const secretProjs = secretsProjects(workspace.projects);
  const onlySecretProj = secretProjs.length === 1 ? (secretProjs[0] ?? null) : null;

  // Processes outlive a view switch, so reconcile on mount rather than
  // assuming nothing is running.
  useEffect(() => {
    api
      .runningIds()
      .then((ids) => setRunning(new Set(ids)))
      .catch(() => {
        /* nothing running */
      });

    // Whether an Inspect affordance can be honoured at all: no sidecar means
    // no offer, in either direction.
    api
      .inspectStatus()
      .then(setInspectStatus)
      .catch(() => {
        /* the inspector reports its own unavailability in the Objects tab */
      });

    // Processes outlive a view switch, so what is attachable is read rather
    // than assumed empty.
    void refreshAttachable();
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  /**
   * The Inspect affordances for the tab the user is looking at.
   *
   * A build session is excluded from the live offer: attaching to MSBuild
   * answers no question anyone asked. Both offers require the inspector to
   * actually be present — `available` is false when the sidecar was never
   * built, and an offer that can only fail is not an offer.
   */
  const activeLabel =
    sessions.find((s) => s.id === activeSession)?.label ?? activeSession ?? "";
  const activeInspect = activeSession ? inspectInfo[activeSession] : undefined;
  const inspectorReady = inspectStatus?.available === true;

  /**
   * The attach offer for the tab being looked at, taken from the backend.
   *
   * Not from this view's own record of the `started` pid: that pid is whatever
   * the supervisor spawned, which for a .NET application is the `dotnet run`
   * CLI rather than the application, whose heap holds none of the user's
   * objects. The backend now lists both — the launcher it started and the
   * application underneath it — so the offer is the application where one was
   * found, and the launcher with its caveat where it was not.
   *
   * `preferApplicationProcess` is shared with the Objects tab on purpose: two
   * copies of "which of these is the real one" could disagree, and a button
   * here that aims somewhere other than the row selected there is the bug this
   * whole change exists to remove. A build session never appears in the list,
   * so MSBuild is excluded without a rule here.
   */
  const liveProcess =
    inspectorReady && activeSession != null
      ? preferApplicationProcess(
          attachable.filter((process) => process.configId === activeSession),
        )
      : null;

  /** Stated beside the button that pays for it, not after the snapshot. */
  const attachCaveats = inspectStatus?.attachCaveats ?? [];

  // Offered only for a crash: a cancelled exit was force-killed and wrote
  // nothing, and a successful one has nothing to explain.
  const crashDump =
    inspectorReady &&
    activeInspect?.exit &&
    !activeInspect.exit.cancelled &&
    !activeInspect.exit.success
      ? (activeInspect.runDump ?? null)
      : null;
  const crashCode = activeInspect?.exit?.code ?? null;

  // The open tabs split into the pinned row and the normal row; with nothing
  // pinned this is exactly the old single strip.
  const { pinned: pinnedTabs, unpinned: unpinnedTabs } = partitionTabs(
    openFiles,
    pinnedFiles,
  );

  /** One file tab — reused by both the pinned and the normal row. */
  const renderFileTab = (file: OpenFile) => {
    const isPinned = pinnedFiles.has(file.id);
    return (
      <button
        key={file.id}
        className={file.id === activeFile ? "active" : ""}
        onClick={() => {
          setActiveFile(file.id);
          // Only workspace files enter the back/forward stack: a secrets tab has
          // no reopenable path (its id is `secrets:<project>`), and go-to-def
          // never lands on it.
          if (file.source.kind === "workspace") recordNav(file.source.path);
        }}
        // Middle-click closes, like browser tabs. The mousedown guard stops the
        // autoscroll cursor.
        onMouseDown={(e) => e.button === 1 && e.preventDefault()}
        onAuxClick={(e) => {
          if (e.button === 1) closeFile(file.id);
        }}
        title={file.source.kind === "secrets" ? file.source.project : file.source.path}
      >
        {dirtyFiles.has(file.id) && (
          <span className="dirty-dot" title="Unsaved changes — Ctrl+S to save">
            ●
          </span>
        )}
        {file.name}
        <span
          className={isPinned ? "row-action pin pinned" : "row-action pin"}
          role="button"
          title={isPinned ? "Unpin this file" : "Pin this file"}
          onClick={(e) => {
            e.stopPropagation();
            setPinnedFiles((previous) => togglePin(previous, file.id));
          }}
        >
          📌
        </span>
        <span
          className="row-action"
          role="button"
          title={
            dirtyFiles.has(file.id)
              ? "Close this file (unsaved changes are discarded)"
              : "Close this file"
          }
          onClick={(e) => {
            e.stopPropagation();
            closeFile(file.id);
          }}
        >
          ×
        </span>
      </button>
    );
  };

  return (
    <>
      <Sidebar>
        {/* Above the tree, not below it: the tree is as long as the repository
            and this would never be seen under it. Rows here are text, not
            controls — there is nothing to run, and the reason is on the row
            and in the tooltip so it is readable without opening a console. */}
        {unreadableProjects.length > 0 && (
          <>
            <div className="group-label">Could not be read</div>
            {unreadableProjects.map((project) => (
              <div
                key={project.id}
                className="row project-unreadable"
                title={`${project.manifestPath}\n${project.unreadable}`}
              >
                <div className="project-unreadable-name">{project.name}</div>
                <div className="faint">{project.unreadable}</div>
              </div>
            ))}
          </>
        )}

        <div className="group-label">Files</div>
        <FileTree
          refreshToken={workspace}
          activePath={activeFile}
          onOpenFile={(path, name) => {
            openFile(path, name);
            recordNav(path);
          }}
        />
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <button
            className="primary"
            onClick={() => selected && start(selected)}
            disabled={!selected || running.has(selected.id) || selectedUnreadable !== null}
            title={selectedUnreadable ?? undefined}
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
            disabled={!selected || selectedUnreadable !== null}
            title={selectedUnreadable ?? "Stop and start again"}
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
            disabled={
              selected?.ecosystem !== "dotnet" || building || selectedUnreadable !== null
            }
            title={selectedUnreadable ?? "Build the project"}
          >
            🔨
          </button>
          <button
            onClick={() => selected && runBuild(selected, "rebuild")}
            disabled={
              selected?.ecosystem !== "dotnet" || building || selectedUnreadable !== null
            }
            title={selectedUnreadable ?? "Rebuild the project (full, non-incremental)"}
          >
            ⟳
          </button>
          <button
            onClick={() => selected && runBuild(selected, "clean")}
            disabled={
              selected?.ecosystem !== "dotnet" || building || selectedUnreadable !== null
            }
            title={selectedUnreadable ?? "Clean the project's build output"}
          >
            🧹
          </button>

          <button onClick={() => selected && setEditing(selected)} disabled={!selected}>
            Edit
          </button>
          {onlySecretProj ? (
            <button
              onClick={() => openSecrets(onlySecretProj.manifestPath)}
              title={`Edit ${onlySecretProj.name}'s .NET user secrets`}
            >
              Secrets…
            </button>
          ) : secretProjs.length > 1 ? (
            <div className="dropdown">
              <button
                onClick={() => setSecretsOpen((was) => !was)}
                title="Choose a .NET project to edit its user secrets"
              >
                Secrets… ▾
              </button>
              {secretsOpen && (
                <>
                  <div className="dropdown-backdrop" onClick={() => setSecretsOpen(false)} />
                  <div className="dropdown-menu" style={{ minWidth: 220 }}>
                    {secretProjs.map((project) => (
                      <div
                        key={project.id}
                        className="dropdown-item"
                        onClick={() => {
                          openSecrets(project.manifestPath);
                          setSecretsOpen(false);
                        }}
                      >
                        {project.name}
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>
          ) : (
            <button disabled title="User secrets are available for .NET projects">
              Secrets…
            </button>
          )}
          {selected?.ecosystem === "dotnet" && (
            <EnvironmentPicker state={environments} onChange={saveEnvironments} />
          )}
          {/* The run-configuration picker sits just right of the environment
              dropdown, so what you run and what you run it in are together. */}
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

          <span style={{ flex: 1 }} />
          {/* Silent unless a server is starting, missing or dead. The key is
              the open-file set because opening a file is what starts a server:
              a `didOpen` from `FileEditor`, not anything this component does. */}
          <LspStatusIndicator pollKey={openFiles.map((f) => f.id).join("\0")} />
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
        {selectedUnreadable && (
          <div className="warning inspect-notice">
            <strong>This configuration's project could not be read.</strong>
            <div>{selectedUnreadable}</div>
            <div className="muted">
              Running it would start the same runner over the same file and fail
              several seconds later with the runner's wording, so Run, Restart
              and the build buttons are disabled until the manifest parses.
            </div>
          </div>
        )}
        {selected?.warnings?.map((warning) => (
          <div className="warning" key={warning}>
            {warning}
          </div>
        ))}

        {/* Deliberately outside `.console-area`: that subtree hosts xterm and
            its sizing is not to be disturbed. */}
        {/* A dump is only called *this* run's crash when it carries the pid
            this run reported. Otherwise it is a dump that was written while
            this ran — with two configurations up it is as likely to be the
            other one's — and it is offered named rather than claimed. */}
        {crashDump && (
          <div className="toolbar">
            <span className="muted" style={{ fontSize: 11 }}>
              {crashDump.certain ? (
                <>
                  {activeLabel} crashed
                  {crashCode != null ? ` (exit ${crashCode})` : ""} and a dump
                  was captured.
                </>
              ) : (
                <>
                  {activeLabel} exited
                  {crashCode != null ? ` (exit ${crashCode})` : ""}. A dump was
                  written while it was running — nothing confirms it came from
                  this configuration rather than another one:{" "}
                  <span className="mono">
                    {crashDump.dump.executable} · pid {crashDump.dump.pid}
                  </span>
                  .
                </>
              )}
            </span>
            <button
              className={crashDump.certain ? "primary" : undefined}
              title={`Read ${crashDump.dump.executable} · pid ${crashDump.dump.pid}`}
              onClick={() =>
                onInspect({
                  target: { kind: "dump", path: crashDump.dump.path },
                  root: { kind: "crashException" },
                  reason: crashDump.certain
                    ? `crash in ${activeLabel}${
                        crashCode != null ? ` (exit ${crashCode})` : ""
                      }`
                    : `${crashDump.dump.executable} · pid ${crashDump.dump.pid}, a dump written while ${activeLabel} was running — not confirmed to be its crash`,
                })
              }
            >
              {crashDump.certain ? "Inspect crash" : "Inspect this dump"}
            </button>
          </div>
        )}

        {/* What an attach costs, and what the pid actually is, before the
            click — pressing either button below starts the snapshot in the
            same commit that the Objects tab first renders its own warning, so
            a caveat that only lives there arrives after the pause. */}
        {liveProcess != null &&
          (attachCaveats.length > 0 || liveProcess.launcherCaveat != null) && (
            <div className="warning">
              {liveProcess.launcherCaveat != null && (
                <div>
                  <strong>
                    pid {liveProcess.pid} is not{" "}
                    {liveProcess.configName ?? activeLabel} itself.
                  </strong>{" "}
                  {liveProcess.launcherCaveat}
                </div>
              )}
              {attachCaveats.map((caveat) => (
                <div key={caveat}>{caveat}</div>
              ))}
            </div>
          )}

        {liveProcess != null && (
          <div className="toolbar">
            {/* The process name is stated, not just the pid: for a `dotnet
                run` configuration the attachable application is a different
                executable from the one the supervisor launched, and naming it
                is how the user can tell the offer aims at their code. */}
            <span
              className="muted"
              style={{ fontSize: 11 }}
              title={liveProcess.path ?? undefined}
            >
              {activeLabel} is running — {liveProcess.name} (pid{" "}
              {liveProcess.pid}).
            </span>
            <button
              title="Attach to the running process and read every exception still on its heap — including ones it caught and logged. This copies the process's memory: expect a brief pause and a memory spike."
              onClick={() =>
                onInspect({
                  target: { kind: "live", pid: liveProcess.pid },
                  root: { kind: "exceptions" },
                  reason: `exceptions in ${activeLabel} (pid ${liveProcess.pid})`,
                })
              }
            >
              Inspect exceptions
            </button>
            <input
              placeholder="Namespace.TypeName"
              value={liveType}
              onChange={(e) => setLiveType(e.target.value)}
              style={{ width: 190 }}
              title="A type to read instances of. There is no option to guess something interesting — a live heap holds millions of objects."
            />
            <button
              disabled={liveType.trim() === ""}
              title={
                liveType.trim() === ""
                  ? "Enter the type to look for"
                  : `Read up to ${LIVE_TYPE_LIMIT} live instances of ${liveType.trim()}. This copies the process's memory: expect a brief pause and a memory spike.`
              }
              onClick={() =>
                onInspect({
                  target: { kind: "live", pid: liveProcess.pid },
                  root: {
                    kind: "type",
                    name: liveType.trim(),
                    limit: LIVE_TYPE_LIMIT,
                  },
                  reason: `${liveType.trim()} in ${activeLabel} (pid ${liveProcess.pid})`,
                })
              }
            >
              Inspect instances
            </button>
          </div>
        )}

        <div className="content console-area">
          <div className="editor-console-split" ref={splitRef}>
            {openFiles.length > 0 && (
              <>
                <div
                  className="editor-pane"
                  // Collapsed, the editor takes everything the tab strip below
                  // it does not want, rather than holding its stored fraction
                  // and leaving a gap where the console used to be.
                  style={
                    consoleCollapsed
                      ? { flex: "1 1 auto" }
                      : { flex: `0 1 ${Math.round(split * 100)}%` }
                  }
                >
                  {/* Pinned tabs get their own row above the rest; with none
                      pinned only the normal strip renders, unchanged. */}
                  {pinnedTabs.length > 0 && (
                    <div className="console-tabs pinned-tabs">
                      {pinnedTabs.map(renderFileTab)}
                    </div>
                  )}
                  {unpinnedTabs.length > 0 && (
                    <div className="console-tabs">
                      {unpinnedTabs.map(renderFileTab)}
                    </div>
                  )}
                  <div className="editor-area">
                    {openFiles.map((file) => (
                      <div
                        key={file.id}
                        style={{
                          display: file.id === activeFile ? "block" : "none",
                          height: "100%",
                        }}
                      >
                        <FileEditor
                          source={file.source}
                          onDirtyChange={(dirty) => setFileDirty(file.id, dirty)}
                          revealLine={reveal?.path === file.id ? reveal.line : null}
                          revealToken={reveal?.path === file.id ? reveal.token : 0}
                          onNavigate={(target, name, line) =>
                            onNavigate?.(target, name, line)
                          }
                        />
                      </div>
                    ))}
                  </div>
                </div>
                {!consoleCollapsed && (
                  <div
                    className="split-resizer"
                    onMouseDown={startSplitDrag}
                    title="Drag to resize"
                  />
                )}
              </>
            )}

            <div className={consoleCollapsed ? "console-pane collapsed" : "console-pane"}>
              {(sessions.length > 0 || openFiles.length > 0) && (
                <div className="console-tabs">
                  {/*
                    The collapse control, and — collapsed — the whole of the
                    panel that remains on screen. Offered only with a file open:
                    with no editor above it this pane is the entire view, so
                    putting it away would leave a strip and nothing else.
                  */}
                  {openFiles.length > 0 && (
                    <button
                      className="pane-toggle"
                      onClick={toggleConsole}
                      title={consoleCollapsed ? "Show the terminal" : "Hide the terminal"}
                      aria-expanded={!consoleCollapsed}
                    >
                      <span className="twisty">{consoleCollapsed ? "▸" : "▾"}</span>
                      {sessions.length === 0 && "Terminal"}
                    </button>
                  )}
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
