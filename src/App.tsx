import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AboutDialog } from "./components/AboutDialog";
import { AppOutputPanel } from "./components/AppOutputPanel";
import { BranchMenu } from "./components/BranchMenu";
import { LauncherPicker } from "./components/LauncherPicker";
import { FeaturesPicker } from "./components/FeaturesPicker";
import { ContextMenu } from "./components/ContextMenu";
import { LspStatusIndicator } from "./components/LspStatus";
import { activeLspPollKey, lspWatching } from "./components/lspStatusLogic";
import { MenuBar } from "./components/MenuBar";
import { isInside } from "./components/launcherLogic";
import { NotesPanel } from "./components/NotesPanel";
import { NotificationHost } from "./components/NotificationHost";
import { Dock } from "./components/Dock";
import { DockProvider, type LiveDockEntry, type SetDockEntry } from "./components/DockContext";
import { removeEntry, upsertEntry } from "./components/dockLogic";
import { OcclusionProvider, Occluder } from "./components/occlusionContext";
import { FocusOrderProvider } from "./components/focusOrderContext";
import { pluginMenuAvailable, pluginMenuRows } from "./components/pluginMenuLogic";
import {
  describeUnexpectedStop,
  dismissNotification,
  notifiesUnexpectedStop,
  pushNotification,
  type AppNotification,
} from "./components/notificationLogic";
import { RunningPanel } from "./components/RunningPanel";
import { liveCount } from "./components/runningLogic";
import {
  addTab,
  applyEvent,
  bufferHeadless,
  closeTab,
  liveTabCount,
  makeTab,
  revealsHeadlessFailure,
  setTabSeverity,
  shouldOpenTab,
  type AppTab,
} from "./components/appOutputLogic";
import {
  liveKeysByEntry,
  newTerminalButton,
  shortcutActionLabel,
  shortcutEntries,
  terminalMenuRows,
  type TerminalMenuAction,
} from "./components/terminalMenuLogic";
import type { Severity } from "./components/consoleLogic";
import type { ConsoleHandle } from "./components/OutputConsole";
import { WorkspaceTab, type WorkspaceTabHandle } from "./components/WorkspaceTab";
import { SettingsDialog } from "./components/SettingsDialog";
import {
  acknowledgeAttention,
  addOpenWorkspace,
  attentionActive,
  type AttentionPulses,
  closeOpenWorkspace,
  mergeSignal,
  nextPulseExpiry,
  pulseAttention,
  reorderWorkspaces,
  tabLabels,
  tabSignalClass,
} from "./components/workspaceTabsLogic";
import type { TabSignal } from "./components/workspaceTabsLogic";
import {
  clearLabel,
  customLabel,
  labelFor,
  loadLabels,
  MAX_LABEL_LENGTH,
  saveLabels,
  setLabel,
  type WorkspaceLabels,
} from "./components/workspaceRenameLogic";
import * as api from "./ipc/api";

/**
 * How long a `done` signal stays on a tab: two runs of the 0.9s `ws-tab-flash`
 * animation, plus enough slack that the class outlives the last frame rather
 * than cutting it. "A terminal finished" is worth a glance and nothing more, so
 * unlike the other three signals it expires without being acknowledged.
 */
const DONE_SIGNAL_MS = 1900;
import { applyAppearance, loadAppearance, onAppearanceChange } from "./appearance";
import { applyWindowOpacity } from "./windowTransparency";
import { transparencySupport, windowBackgroundOpacity } from "./windowTransparencyLogic";
import { dispatchShortcut, registerCommand } from "./shortcuts";
import { loadRecents, rememberRecent } from "./recentsLogic";
import type {
  InspectTarget,
  Launchable,
  LauncherGroups,
  ProcessEvent,
  FeatureInfo,
  RootSpec,
  RunningReport,
  ShellInfo,
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
  // Drag-to-reorder of the codebase tabs: the index being dragged and the tab it
  // is currently hovering, so the drop target can be outlined. Cleared on drop or
  // drag-end. Reordering never changes which root is active (identity is `root`).
  const [tabDrag, setTabDrag] = useState<{ from: number; over: number } | null>(null);
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
  // The embedded browser is per-codebase now (bugs 6+7): each open codebase
  // keeps its own live page and only the active one is visible, so its state
  // and its panel live inside `WorkspaceTab`, not here. The Plugins menu and the
  // `plugin.browser` command open it on the foreground tab through
  // `activeHandle()?.openBrowser()`, like `openSql`.
  /** Help → About. App-level like the other dialogs: it describes the build, not a codebase. */
  const [aboutOpen, setAboutOpen] = useState(false);
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
  /**
   * The terminal split button's menu: where it opened, and the saved commands
   * it lists.
   *
   * `null` groups is "not read yet", not "there are none" — `terminalMenuRows`
   * is told which of the two it is, because an empty section under a heading
   * claims the user has saved no shortcuts and that would be a guess.
   */
  const [terminalMenu, setTerminalMenu] = useState<{ x: number; y: number } | null>(null);
  /**
   * The shells detected on this machine, for the menu's one-off pick rows.
   *
   * `null` is "not detected yet", exactly as `launcherGroups` is: the menu is
   * told which of the two it is, because an empty shells section would report
   * that this machine has no shell at all — a much stronger claim than "we have
   * not looked", and one the user might act on.
   */
  const [shells, setShells] = useState<ShellInfo[] | null>(null);
  const [pluginMenu, setPluginMenu] = useState<{ x: number; y: number } | null>(null);
  const [launcherGroups, setLauncherGroups] = useState<LauncherGroups | null>(null);
  /**
   * Which launcher entry each live supervisor key was started from.
   *
   * Held here because a `RunningRecord` carries no launcher id — the key is
   * minted in this file, so this is the only place the two can be joined, and
   * the menu needs the join to know a service is already up. Entries are
   * dropped as their process ends, so this cannot grow across a session.
   */
  const [launchEntryIds, setLaunchEntryIds] = useState<Record<string, string>>({});
  // Per-codebase terminal-attention flag, so a background tab can flash to show
  // which project a minimized terminal's bell is coming from. Live state, not an
  // event: a terminal is asking for you until it is restored, so this goes back
  // down on its own and is never latched.
  const [attentionByRoot, setAttentionByRoot] = useState<Record<string, boolean>>({});

  // The attention flag above is *sticky* (it clears only on focus) and an agent
  // TUI like Codex rings the bell on nearly every redraw, so flashing straight
  // off it blinks a background tab forever. `attentionPulses` turns each rising
  // edge into one bounded flash (see `workspaceTabsLogic`): a `now` clock, bumped
  // by a timer scheduled to the soonest pulse expiry, drives the re-render that
  // ends the flash.
  const [attentionPulses, setAttentionPulses] = useState<AttentionPulses>({});
  const [pulseNow, setPulseNow] = useState(() => Date.now());

  // Arm a flash for every root now asking for attention, and clear the ones that
  // have gone quiet so a later bell can flash again. `pulseAttention` is a no-op
  // while a root is already flashing or has settled, so the constant bells that
  // keep `attentionByRoot` true cannot restart the flash.
  useEffect(() => {
    const now = Date.now();
    setAttentionPulses((prev) => {
      let next = prev;
      for (const root of new Set([...Object.keys(prev), ...Object.keys(attentionByRoot)])) {
        next = attentionByRoot[root]
          ? pulseAttention(next, root, now)
          : acknowledgeAttention(next, root);
      }
      return next;
    });
    setPulseNow(now);
  }, [attentionByRoot]);

  // One timer, aimed at the soonest in-flight pulse: firing it bumps the clock,
  // which re-renders the tab (ending its flash) and re-runs this effect to arm
  // the next expiry, or nothing once every pulse has settled.
  useEffect(() => {
    const remaining = nextPulseExpiry(attentionPulses, Date.now());
    if (remaining === null) return;
    const timer = window.setTimeout(() => setPulseNow(Date.now()), remaining + 1);
    return () => window.clearTimeout(timer);
  }, [attentionPulses, pulseNow]);

  /**
   * Each open codebase's open-file set, as its Run view reports it.
   *
   * Read for the *active* root only: the language-server indicator in the
   * bottom status bar is one instance for the whole app, and `lsp_status`
   * answers for the active workspace slot. `activeLspPollKey` composes the key
   * that re-arms it, and `lspWatching` — a separate question — decides whether
   * to keep asking at all.
   */
  const [lspPollKeyByRoot, setLspPollKeyByRoot] = useState<Record<string, string>>({});

  /**
   * Whether each open codebase's editor area holds a tab, as its Run view
   * reports it. Read for the *active* root only — the window is one window, and
   * a background codebase's editors are not what the user is looking at.
   *
   * A root **absent** from this map has not reported yet, which
   * `windowBackgroundOpacity` reads as "unknown" and resolves to opaque.
   */
  const [editorTabsByRoot, setEditorTabsByRoot] = useState<Record<string, boolean>>({});

  /**
   * The app-wide notification stack.
   *
   * Global, not per-workspace, because the first thing it reports belongs to
   * no codebase: a launcher entry's `cwd` may sit outside every open
   * workspace, so the tab-signal mechanism has no tab to outline.
   */
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const notificationSeq = useRef(0);
  /** Raise a notification. The one way in; `notificationLogic` owns the rules. */
  const notify = useCallback((next: Omit<AppNotification, "id">) => {
    notificationSeq.current += 1;
    const id = `note-${notificationSeq.current}`;
    setNotifications((list) => pushNotification(list, { ...next, id }));
  }, []);
  const dismissNote = useCallback((id: string) => {
    setNotifications((list) => dismissNotification(list, id));
  }, []);

  // The shared minimized-window dock. Every floating panel registers here while
  // minimized (via `useDockEntry`); `Dock` lays them out in one strip, scoped to
  // the active codebase. The setter is stable so registering does not re-render
  // the other panels — only this component and `Dock` update.
  const [dockEntries, setDockEntries] = useState<LiveDockEntry[]>([]);
  const setDockEntry = useCallback<SetDockEntry>((id, entry) => {
    setDockEntries((list) => (entry ? upsertEntry(list, entry) : removeEntry(list, id)));
  }, []);

  /**
   * Launches the user declared to be long-running services, by key.
   *
   * Kept beside `headlessLaunches` rather than read back off the launcher
   * store: the entry could be edited or forgotten while the process runs, and
   * what matters is what it was declared to be *when it was started*.
   */
  const persistentLaunches = useRef(new Map<string, { label: string }>());

  /**
   * Record, or drop, one codebase's key. A closing tab reports `null`, which
   * deletes rather than blanks: a blank entry reads the same to the status bar
   * but leaves one dead key behind per codebase ever opened.
   */
  const setLspPollKeyForRoot = useCallback((root: string, key: string | null) => {
    setLspPollKeyByRoot((prev) => {
      if (key === null) {
        if (!(root in prev)) return prev;
        const { [root]: _closed, ...rest } = prev;
        return rest;
      }
      // Guarded because this fires on every codebase's editor-tab change, and an
      // unconditional new object re-renders the app for a key that did not move.
      return prev[root] === key ? prev : { ...prev, [root]: key };
    });
  }, []);
  /**
   * Record, or drop, one codebase's editor state. Deletes on `null` for the
   * same reason the poll key does: a stale entry for a closed codebase would go
   * on answering the window's question.
   */
  const setEditorTabsForRoot = useCallback((root: string, open: boolean | null) => {
    setEditorTabsByRoot((prev) => {
      if (open === null) {
        if (!(root in prev)) return prev;
        const { [root]: _closed, ...rest } = prev;
        return rest;
      }
      // Guarded, like the poll key: this fires on every codebase's editor-tab
      // change, and an unconditional new object re-renders the app for an
      // answer that did not move.
      return prev[root] === open ? prev : { ...prev, [root]: open };
    });
  }, []);

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

  /**
   * The names the user gave their open codebases, keyed by root.
   *
   * Not held on the `Workspace` object: `onWorkspaceChange` and
   * `addOpenWorkspace` replace that object in place on every rescan and
   * re-open, so a rename kept there would be silently discarded.
   */
  const [wsLabels, setWsLabels] = useState<WorkspaceLabels>(() => loadLabels(localStorage));
  /** The root whose tab is being edited inline, and the draft text. */
  const [renamingRoot, setRenamingRoot] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  /** The right-clicked tab and where its menu opened. */
  const [tabMenu, setTabMenu] = useState<{ root: string; x: number; y: number } | null>(null);

  /**
   * Set while Escape is abandoning an edit, so the blur that may follow does not
   * commit it.
   *
   * A ref and not state, and not a reliance on ordering: whether removing a
   * focused input fires `blur` at all differs between browsers and React
   * versions, so the rule is made explicit here rather than inferred from an
   * event that may or may not arrive. Escape is the one gesture that must never
   * save.
   */
  const abandoningRename = useRef(false);

  /** Open the inline editor, seeded with whatever the user would be editing. */
  const beginRename = (root: string) => {
    setTabMenu(null);
    const derived = openWorkspaces.find((w) => w.root === root)?.name ?? root;
    // `labelFor`, not `labels[i]`: the strip may show a disambiguating path
    // prefix this app generated, and seeding the box with it would invite the
    // user to save our disambiguation as their chosen name.
    setRenameDraft(labelFor(wsLabels, root, derived));
    abandoningRename.current = false;
    setRenamingRoot(root);
  };

  /**
   * Accept the typed name, or keep the old one. `setLabel` returns null for a
   * blank name — an empty tab label is unclickable — and the abstain rule says
   * fall back to the scanned name rather than inventing one.
   */
  const commitRename = (root: string, raw: string) => {
    if (abandoningRename.current) {
      abandoningRename.current = false;
      setRenamingRoot(null);
      return;
    }
    const next = setLabel(wsLabels, root, raw);
    if (next) {
      setWsLabels(next);
      saveLabels(localStorage, next);
    }
    setRenamingRoot(null);
  };

  /** Drop a rename and follow the scan again. */
  const resetRename = (root: string) => {
    const next = clearLabel(wsLabels, root);
    setWsLabels(next);
    saveLabels(localStorage, next);
    setTabMenu(null);
  };

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
  /**
   * Launches that were started headless and so have no tab yet.
   *
   * The entry is what a tab would need if the run fails: "no tab" must not
   * become "no answer", so a headless run that dies gets its console after the
   * fact, replaying the output buffered under its key. Removed as soon as a tab
   * exists — from then on it is an ordinary launch.
   */
  const headlessLaunches = useRef(new Map<string, { label: string; cwd: string }>());

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
    const headless = headlessLaunches.current.get(key);
    const handle = appConsoles.current.get(key);
    if (handle) {
      handle.handle(event);
    } else {
      const queued = appPending.current.get(key) ?? [];
      // A headless run has no console to drain its buffer, and may run for
      // days, so its buffer is capped. Everything else is drained within a tick
      // by `registerAppConsole` and is left whole.
      appPending.current.set(key, headless ? bufferHeadless(queued, event) : [...queued, event]);
    }
    const root = appWorkspaceRoots.current.get(key);
    if (root && event.type === "exited" && !event.cancelled) {
      raiseSignal(root, event.success ? "success" : "error");
    } else if (root && event.type === "failed") {
      raiseSignal(root, "error");
    }
    // The process is over, so the key can no longer be running: drop the join
    // that told the terminal menu this entry's service was up.
    if (event.type === "exited" || event.type === "failed") {
      setLaunchEntryIds(({ [key]: _ended, ...rest }) => rest);
    }
    const reveal = headless !== undefined && revealsHeadlessFailure(event);
    // A headless run that ends is done with either way, so its bookkeeping goes
    // now — not only when a failure is revealed. Clearing on `reveal` alone
    // leaked an entry in all three refs for every headless run that succeeded or
    // was stopped, and those are the common cases: a headless run mints no output
    // tab, so `closeAppTab` (the only other place that clears them) is
    // unreachable. The buffered events go too — up to 200 per run — and nothing
    // can replay them once the process is gone unless the failure path above is
    // taking them right now.
    const service = persistentLaunches.current.get(key);
    if (service && notifiesUnexpectedStop(event, true)) {
      // Deduped on the key, so a crash-looping service stays one entry that
      // keeps updating rather than a new toast per restart.
      notify({ ...describeUnexpectedStop(service.label, event), dedupeKey: key });
    }
    const ended = event.type === "exited" || event.type === "failed";
    if (ended) persistentLaunches.current.delete(key);
    if (reveal || (headless !== undefined && ended)) {
      headlessLaunches.current.delete(key);
      appWorkspaceRoots.current.delete(key);
      if (!reveal) appPending.current.delete(key);
    }
    setAppTabs((tabs) => {
      if (!reveal || tabs.some((t) => t.key === key)) return applyEvent(tabs, key, event);
      // Minting the tab now is what makes the buffered output reachable: the
      // console mounts, registers, and `registerAppConsole` replays everything
      // that arrived while nobody was watching.
      const revealed = makeTab(
        { key, id: key, label: headless.label, cwd: headless.cwd },
        appWorkspaceRoots.current.get(key) ?? null,
      );
      return applyEvent([...tabs, revealed], key, event);
    });
    if (reveal) {
      setActiveAppKey(key);
      setAppOutputOpen(true);
    }
  }, [raiseSignal]);

  /**
   * Launch a command from the picker: open a tab for it, then start it.
   *
   * A **headless** launch skips the tab (`shouldOpenTab`) but not the channel:
   * it is still supervised, still listed in Running, still stoppable, and its
   * output is buffered under its key so a failure can be shown after the fact.
   */
  const launchApp = useCallback(
    (spec: {
      command: string;
      cwd: string;
      shell: boolean;
      label?: string;
      headless?: boolean;
      /** Declared a long-running service: any end to it is worth reporting. */
      persistent?: boolean;
    }) => {
      // The key is minted here, not by the backend: output starts arriving the
      // moment the process spawns, which is before `launchCommand` resolves.
      const key = `ext:${crypto.randomUUID()}`;
      const label = spec.label?.trim() || spec.command;
      // Attributed by the command's OWN cwd, not by whichever codebase happens
      // to be in front. A launcher entry can live outside every open workspace
      // — that is exactly what the picker's `global` group means — and blaming
      // the foreground codebase for it outlined an unrelated tab red. No
      // matching workspace means no attribution; the notification carries it
      // instead, which is part of why that surface exists.
      const root = openWorkspaces.find((w) => isInside(w.root, spec.cwd))?.root ?? null;
      if (root) appWorkspaceRoots.current.set(key, root);
      if (spec.persistent) persistentLaunches.current.set(key, { label });
      if (shouldOpenTab(spec)) {
        const added = addTab(appTabs, makeTab({ key, id: key, label, cwd: spec.cwd }, root));
        setAppTabs(added.tabs);
        setActiveAppKey(added.activeKey);
        setAppOutputOpen(true);
      } else {
        headlessLaunches.current.set(key, { label, cwd: spec.cwd });
      }

      api
        .launchCommand({ ...spec, key }, (event) => onAppEvent(key, event))
        .then((app) => {
          // Adopt the backend's label (it applies an earlier rename) and the id
          // of the recents entry, which the panel addresses pin/rename by.
          setAppTabs((tabs) =>
            tabs.map((t) => (t.key === key ? { ...t, label: app.label, entryId: app.id } : t)),
          );
          setLaunchEntryIds((prev) => ({ ...prev, [key]: app.id }));
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
      headlessLaunches.current.delete(key);
      if (result.tabs.length === 0) setAppOutputOpen(false);
    },
    [appTabs, activeAppKey, stopApp],
  );

  /**
   * Open the terminal menu under the caret, with a freshly read command list.
   *
   * Cleared first, exactly as the Run tab's Stop menu does: a list left over
   * from the last time the menu was open is worse than no list, because the
   * user would act on it.
   */
  const openTerminalMenu = (event: React.MouseEvent) => {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    setTerminalMenu({ x: box.left, y: box.bottom + 2 });
    setLauncherGroups(null);
    api
      .listLaunchables()
      .then(setLauncherGroups)
      .catch((e) => setError(api.errorMessage(e)));
    // Shells are re-detected on every open under the same rule, which also
    // means a shell installed mid-session shows up without a restart.
    setShells(null);
    api
      .listShells()
      .then(({ shells: found }) => setShells(found))
      .catch((e) => setError(api.errorMessage(e)));
  };

  /** Run one saved command shortcut, honouring its headless flag. */
  const runShortcut = (entry: Launchable) => {
    launchApp({
      command: entry.command,
      cwd: entry.cwd,
      shell: entry.shell,
      label: entry.label ?? undefined,
      headless: entry.headless,
      persistent: entry.persistent,
    });
  };

  /** Carry out one terminal-menu row. The row decided what; this only does it. */
  const runTerminalMenuAction = (action: TerminalMenuAction) => {
    setTerminalMenu(null);
    switch (action.kind) {
      case "newTerminal":
        activeHandle()?.openTerminal();
        return;
      case "newTerminalIn":
        activeHandle()?.openTerminalIn(action.shell);
        return;
      case "launcher":
        setLauncherOpen(true);
        return;
      case "running":
        setRunningOpen(true);
        return;
      case "apps":
        setAppOutputOpen(true);
        return;
      case "runShortcut":
        runShortcut(action.entry);
        return;
      case "stopShortcut":
        for (const key of action.keys) stopApp(key);
        return;
    }
  };

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

  /**
   * The window's background opacity, and whether this platform can honour it.
   *
   * `appearance` tracks the *applied* settings rather than what is in storage:
   * the Settings dialog previews unpersisted, so re-reading `localStorage` here
   * would preview nothing. That is what `onAppearanceChange` now hands over.
   *
   * `os` stays `null` until `about_info` answers, and a failed read leaves it
   * there — `transparencySupport(null)` is unsupported, so the window abstains
   * to fully opaque rather than guessing at a platform.
   */
  const [appearance, setAppearance] = useState(loadAppearance);
  const [os, setOs] = useState<string | null>(null);
  useEffect(() => onAppearanceChange(setAppearance), []);
  useEffect(() => {
    let live = true;
    void api
      .aboutInfo()
      .then((info) => {
        if (live) setOs(info.os);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  // The single writer of `--app-bg-opacity`. Every input is state this
  // component already holds, and the whole decision is the pure, tested
  // `windowBackgroundOpacity` — nothing is decided here.
  useEffect(() => {
    applyWindowOpacity(
      windowBackgroundOpacity({
        opacity: appearance.windowOpacity,
        supported: transparencySupport(os).supported,
        activeRoot,
        editorTabsByRoot,
      }),
    );
  }, [appearance.windowOpacity, os, activeRoot, editorTabsByRoot]);

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
      registerCommand("plugin.browser", () => activeHandle()?.openBrowser()),
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
    // clicked" in the signal's promise, and it also frees the attention pulse so
    // a later background bell can flash the tab afresh.
    clearSignal(root);
    setAttentionPulses((prev) => acknowledgeAttention(prev, root));
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
    setAttentionPulses((prev) => acknowledgeAttention(prev, root));
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

  const labels = tabLabels(openWorkspaces, wsLabels);

  return (
    <OcclusionProvider>
    <FocusOrderProvider>
    <DockProvider setDockEntry={setDockEntry}>
    <div className="app">
      {/* Three zones, not a flex row with a spacer: the branch widget is meant to
          sit in the *window's* centre, and a spacer can only centre it when the
          two sides happen to be the same width. `.titlebar` is a
          three-column grid so the middle zone is centred regardless of how
          much chrome sits either side of it. */}
      <div className="titlebar">
        <div className="titlebar-left">
          {/* File (Open / Rescan) and Enhancements — the agent actions target the
              foreground tab through its registered handle. */}
          <MenuBar
            onOpen={pickFolder}
            onRescan={rescan}
            onRunAgent={(promptId) => activeHandle()?.openRunAgent(promptId)}
            onOpenReview={() => activeHandle()?.openReview()}
            onOpenFeatures={() => setFeaturesOpen(true)}
            onOpenSettings={() => setSettingsOpen(true)}
            onOpenAbout={() => setAboutOpen(true)}
          />
        </div>

        {/* The zone is rendered even with no workspace open, so the left and
            right columns do not shift the moment a branch widget appears. */}
        <div className="titlebar-center">
          {activeWorkspace && (
            /* Keyed by the active root, so switching codebases re-reads branches. */
            <BranchMenu key={activeRoot ?? ""} onOpenWorktree={(path) => void openPath(path)} />
          )}
        </div>

        <div className="titlebar-right">
          {activeWorkspace && (
            <span className="muted" style={{ fontSize: 11 }}>
              {activeWorkspace.projects.length} project
              {activeWorkspace.projects.length === 1 ? "" : "s"}
            </span>
          )}

          {/* The optional features' own surface. The SQL console used to live in
              the terminal menu, which is a menu about *running things*; a
              database console is not one, and the next plugin would have been a
              second guest there. Hidden entirely when every plugin is off —
              `pluginMenuAvailable` — rather than opening onto nothing. */}
          {pluginMenuAvailable({ features, workspaceOpen: activeWorkspace !== null }) && (
            <button
              onClick={(event) => {
                const box = event.currentTarget.getBoundingClientRect();
                setPluginMenu({ x: box.left, y: box.bottom + 4 });
              }}
              title="Open an optional feature"
            >
              Plugins ▾
            </button>
          )}
          <button onClick={showNotes} title="Open or restore the notes / scratchpad panel">
            Notes
          </button>
          {/* One split button in place of Launch / Apps / Running / + Terminal:
              the body opens a terminal, the caret holds the rest. Notes keeps
              its own button — it is global and about nothing running. The
              running count moved onto the caret so the information the two
              badges carried is still visible with the menu shut. */}
          <span className="split-button">
            <button
              data-command="terminal.new"
              onClick={() => activeHandle()?.openTerminal()}
              disabled={newTerminalButton({ workspaceOpen: activeWorkspace !== null }).disabled}
              title={newTerminalButton({ workspaceOpen: activeWorkspace !== null }).title}
            >
              New Terminal
            </button>
            <button
              className="split-button-arrow"
              onClick={openTerminalMenu}
              title="Launch, running processes and your saved commands"
              aria-label="Terminal and launcher menu"
            >
              ▾
              {liveCount(runningReport) > 0 && (
                <span className="running-badge">{liveCount(runningReport)}</span>
              )}
            </button>
          </span>
          <button onClick={rescan} title="Re-detect projects and configurations">
            Rescan
          </button>
          <button onClick={pickFolder}>Open…</button>
        </div>
      </div>

      {/* The open-codebases tab strip, above each workspace's own inner tabs. */}
      <div className="tabs ws-tabs">
        {openWorkspaces.map((w, i) => (
          <div
            key={w.root}
            className={`ws-tab ${w.root === activeRoot ? "active" : ""}${
              tabDrag?.from === i ? " dragging" : ""
            }${tabDrag && tabDrag.from !== i && tabDrag.over === i ? " drag-over" : ""}${tabSignalClass(
              w.root,
              activeRoot,
              // A ringing bell is live state and outranks nothing it is folded
              // into; a latched signal keeps showing once it stops ringing. The
              // attention half is a bounded pulse, not the raw sticky flag, so a
              // background tab flashes once and settles rather than blinking for
              // as long as the terminal keeps ringing.
              attentionActive(attentionPulses, w.root, pulseNow)
                ? mergeSignal(signalByRoot[w.root], "attention")
                : (signalByRoot[w.root] ?? null),
            )}`}
            // The tab is a drag handle for reordering, except while its rename
            // box is open — a draggable container would swallow the text
            // selection the input needs.
            draggable={renamingRoot !== w.root}
            onDragStart={(e) => {
              setTabDrag({ from: i, over: i });
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(e) => {
              if (!tabDrag) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              if (tabDrag.over !== i) setTabDrag({ ...tabDrag, over: i });
            }}
            onDrop={(e) => {
              e.preventDefault();
              if (tabDrag) setOpenWorkspaces((list) => reorderWorkspaces(list, tabDrag.from, i));
              setTabDrag(null);
            }}
            onDragEnd={() => setTabDrag(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setTabMenu({ root: w.root, x: e.clientX, y: e.clientY });
            }}
          >
            {renamingRoot === w.root ? (
              <input
                className="ws-tab-rename"
                autoFocus
                value={renameDraft}
                // In UTF-16 units, while the stored cap is in code points, so a
                // pasted paragraph is stopped here and `normalizeLabel` does the
                // exact trim. The two agreeing to the character is not worth a
                // second implementation of the cap in the DOM.
                maxLength={MAX_LABEL_LENGTH}
                onChange={(e) => setRenameDraft(e.target.value)}
                // Blur commits as well as Enter: clicking away from a rename box
                // reads as "that is the name", not as "throw it away". Escape is
                // the exception, and it says so through `abandoningRename`.
                onBlur={() => commitRename(w.root, renameDraft)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename(w.root, renameDraft);
                  else if (e.key === "Escape") {
                    abandoningRename.current = true;
                    setRenamingRoot(null);
                  }
                }}
              />
            ) : (
              <button
                className="ws-tab-label"
                onClick={() => void activateWorkspace(w.root)}
                onDoubleClick={() => beginRename(w.root)}
                title={w.root}
              >
                {labels[i]}
              </button>
            )}
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

      {/* The tab's right-click menu, through the shared ContextMenu shell — it
          adds Escape-to-close and viewport clamping, which the two remaining
          hand-rolled copies still lack. Its default stacking level applies, so
          no z-index is written here. */}
      {tabMenu && (
        <ContextMenu x={tabMenu.x} y={tabMenu.y} elevated onClose={() => setTabMenu(null)}>
          <div className="dropdown-item" onClick={() => beginRename(tabMenu.root)}>
            Rename…
          </div>
          <div
            className={`dropdown-item${
              customLabel(wsLabels, tabMenu.root) === undefined ? " disabled" : ""
            }`}
            onClick={() => {
              // Nothing to reset on a tab that was never renamed. Doing nothing
              // beats writing the scanned name back as if it were a choice.
              if (customLabel(wsLabels, tabMenu.root) !== undefined) resetRename(tabMenu.root);
            }}
          >
            Reset name
          </div>
          <div
            className="dropdown-item"
            onClick={() => {
              const root = tabMenu.root;
              setTabMenu(null);
              void closeWorkspace(root);
            }}
          >
            Close
          </div>
        </ContextMenu>
      )}

      {/* Every row — label, disabled reason and action — comes from
          `pluginMenuRows`; nothing is decided here. */}
      {pluginMenu && (
        <ContextMenu
          x={pluginMenu.x}
          y={pluginMenu.y}
          elevated
          onClose={() => setPluginMenu(null)}
        >
          {pluginMenuRows({ features, workspaceOpen: activeWorkspace !== null }).map((row) => (
            <div
              key={row.id}
              className={`dropdown-item${row.disabled ? " disabled" : ""}`}
              title={row.title}
              onClick={() => {
                if (row.action === null) return;
                setPluginMenu(null);
                if (row.action.kind === "sql") activeHandle()?.openSql();
                if (row.action.kind === "ask") activeHandle()?.openAsk();
                if (row.action.kind === "mcp") activeHandle()?.openMcp();
                if (row.action.kind === "browser") activeHandle()?.openBrowser();
                if (row.action.kind === "tasks") activeHandle()?.openTasks();
                if (row.action.kind === "roslynMcp") activeHandle()?.openRoslynMcp();
              }}
            >
              {row.label}
            </div>
          ))}
        </ContextMenu>
      )}

      {error && <div className="error">{error}</div>}

      {/* The terminal split button's menu, through the shared ContextMenu shell.
          Every row — its label, badge, disabled reason and action — comes from
          `terminalMenuRows`; nothing is decided here. */}
      {terminalMenu && (
        <ContextMenu
          x={terminalMenu.x}
          y={terminalMenu.y}
          elevated
          onClose={() => setTerminalMenu(null)}
        >
          {terminalMenuRows({
            workspaceOpen: activeWorkspace !== null,
            runningCount: liveCount(runningReport),
            appTabCount: appTabs.length,
            liveAppCount: liveTabCount(appTabs),
            shortcuts: launcherGroups ? shortcutEntries(launcherGroups) : [],
            liveKeys: liveKeysByEntry(runningReport, new Map(Object.entries(launchEntryIds))),
            shortcutsLoading: launcherGroups === null,
            shells: shells ?? [],
            shellsLoading: shells === null,
          }).map((row) => (
            <div key={row.id}>
              {row.separator && <div className="dropdown-separator" />}
              {row.section !== undefined && (
                <div className="dropdown-section">{row.section}</div>
              )}
              <div
                className={`dropdown-item${row.disabled ? " disabled" : ""}`}
                title={row.title}
                onClick={() => row.action && runTerminalMenuAction(row.action)}
              >
                {row.live !== undefined && (
                  <span className={`shortcut-dot${row.live ? " live" : ""}`} aria-hidden="true" />
                )}
                <span className="terminal-menu-label">{row.label}</span>
                {row.badge !== null && <span className="running-badge">{row.badge}</span>}
                {row.live !== undefined && (
                  <span className="terminal-menu-action">{shortcutActionLabel(row)}</span>
                )}
              </div>
            </div>
          ))}
        </ContextMenu>
      )}

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
          onLspPollKeyChange={setLspPollKeyForRoot}
          onEditorTabsChange={setEditorTabsForRoot}
          onSignal={raiseSignal}
          onNotify={notify}
          features={features}
        />
      ))}

      {/* The global Notes / scratchpad panel — one instance, not per-workspace.
          Its "send to agent" runs in the foreground tab. */}
      {featuresOpen && (
        <>
          <Occluder />
          <FeaturesPicker
            features={features}
            onChange={setFeatures}
            onClose={() => setFeaturesOpen(false)}
          />
        </>
      )}

      {settingsOpen && (
        <>
          <Occluder />
          <SettingsDialog onClose={() => setSettingsOpen(false)} />
        </>
      )}

      {aboutOpen && (
        <>
          <Occluder />
          <AboutDialog onClose={() => setAboutOpen(false)} />
        </>
      )}

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
        <>
          <Occluder />
          <LauncherPicker
            root={activeRoot}
            onLaunch={launchApp}
            onClose={() => setLauncherOpen(false)}
          />
        </>
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

      {/* One global notification stack. Not a workspace tab signal: a
          launcher entry's cwd may sit outside every open codebase, so a
          service dying is often nobody's tab to outline. */}
      <NotificationHost notifications={notifications} onDismiss={dismissNote} />

      {/* The shared minimized-window dock: every minimized floating panel's pill,
          scoped to the active codebase, in one collision-free strip. */}
      <Dock entries={dockEntries} activeRoot={activeRoot} />

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
        {/* Silent unless a server is starting, missing or dead. Mounted outside
            the `activeWorkspace` guard so it keeps one poll loop across codebase
            switches rather than remounting — the key is what tells it the
            codebase changed. Opening a file is what starts a server (a `didOpen`
            from `FileEditor`), which is why `watching` is false with nothing
            open and the poll stops rather than running for the life of the app. */}
        <LspStatusIndicator
          pollKey={activeLspPollKey(activeRoot, lspPollKeyByRoot)}
          watching={lspWatching(activeRoot, lspPollKeyByRoot)}
        />
      </div>
    </div>
    </DockProvider>
    </FocusOrderProvider>
    </OcclusionProvider>
  );
}
