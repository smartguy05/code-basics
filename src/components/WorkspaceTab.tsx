import { useEffect, useRef, useState } from "react";
import { ArchitectureView } from "../views/ArchitectureView";
import { AskPanel } from "./AskPanel";
import { BehavioralPanel } from "./BehavioralPanel";
import { ChangesView } from "../views/ChangesView";
import { HistoryView } from "../views/HistoryView";
import { InspectView } from "../views/InspectView";
import { RunView } from "../views/RunView";
import { ReviewPanel } from "./ReviewPanel";
import { SearchEverywhere } from "./SearchEverywhere";
import { SetupPrompt } from "./SetupPrompt";
import { SqlView } from "../views/SqlView";
import { shouldPrompt, setDismissed } from "./setupPromptLogic";
import { TerminalPanel } from "./TerminalPanel";
import { TestsView } from "../views/TestsView";
import { terminalTitle } from "./askLogic";
import {
  makeAgentTerminal,
  makeTerminal,
  raiseTerminal,
  recolorTerminal,
  renameTerminal,
  stackOffset,
  syncStackOrder,
  type TerminalDescriptor,
} from "./terminalLogic";
import { sendToAgentTitle } from "./notesLogic";
import type { TabSignal } from "./workspaceTabsLogic";
import * as api from "../ipc/api";
import type { AgentMode } from "../ipc/api";
import type { BehavioralReport, FeatureInfo, Note, Workspace } from "../ipc/types";
import type { InspectRequest, OpenFileRequest, SelectConfigRequest } from "../App";
import { featureEnabled, tabAfterDisable, visibleTabs, type FeatureKey } from "./featuresLogic";
import { registerCommand } from "../shortcuts";

type Tab = "tests" | "run" | "changes" | "history" | "architecture" | "inspect" | "sql";

const TABS: { id: Tab; label: string }[] = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
  { id: "changes", label: "Changes" },
  { id: "history", label: "History" },
  { id: "architecture", label: "Architecture" },
  { id: "inspect", label: "Objects" },
  { id: "sql", label: "SQL" },
];

/**
 * Which optional feature owns each tab. A tab absent from this map is core and
 * cannot be switched off, which is why it is a partial map rather than a field
 * on `TABS` — most tabs have no feature and should not have to say so.
 */
const FEATURE_BY_TAB: Partial<Record<Tab, FeatureKey>> = {
  sql: "sqlConsole",
};

/**
 * The actions the titlebar (global chrome) and the global Notes panel route to
 * the *foreground* workspace. Each open tab registers one of these with `App`;
 * `App` invokes the active tab's handle.
 */
export interface WorkspaceTabHandle {
  openTerminal(): void;
  /**
   * Open an interactive terminal in this codebase already asking `question` of
   * `agentId`. Part of the handle rather than private to the tab because the
   * question may come from outside it (the panel is rendered here, but the
   * action is one the app routes to the foreground codebase, like the others).
   */
  openAskTerminal(question: string, agentId: string, model: string | undefined): void;
  openRunAgent(promptId: string): void;
  openReview(): void;
  openNoteInAgent(note: Note): void;
}

/**
 * One open codebase — everything that used to live in `App`'s workspace branch,
 * now instantiated once per open root and kept mounted whether or not it is the
 * foreground tab.
 *
 * # Why it stays mounted when backgrounded
 *
 * Backgrounding is `hidden={!active}` on the wrapper, never an unmount: React
 * preserves this subtree's state and DOM — a running Run/Test process and its
 * console, a live language server, and every floating terminal's xterm buffer —
 * so a background codebase keeps working exactly as the plan requires. The
 * floating panels (agent, before/after, terminals) are `position: fixed`
 * children of the hidden wrapper, so they vanish with the tab and reappear with
 * it without ever tearing down their processes.
 *
 * # What is per-tab vs global
 *
 * Everything here is this workspace's own: the inner Run/Tests/… selection, the
 * agent and before/after panels, the palette, the setup prompt, and this
 * workspace's terminals (each bound to `workspace.root` as its cwd), and the run
 * configuration dropdown (in the Run view's own toolbar, beside the environment
 * picker). The Notes panel and the editor font size are global and stay in
 * `App`; the titlebar's per-workspace *display* (the branch widget) and the
 * bottom status bar's (folder name and path) are bound to the active tab there,
 * and its per-workspace *actions* come back to the foreground tab through
 * {@link WorkspaceTabHandle}.
 */
export function WorkspaceTab({
  workspace,
  active,
  onWorkspaceChange,
  onRegister,
  onAttentionChange,
  onSignal,
  features,
}: {
  workspace: Workspace;
  /** Whether this is the foreground tab. Drives `hidden` and gates listeners. */
  active: boolean;
  /** A rescan/config-save handed back a fresh workspace for this root. */
  onWorkspaceChange: (workspace: Workspace) => void;
  /** Register (or, with `null`, unregister) this tab's action handle with `App`. */
  onRegister: (root: string, handle: WorkspaceTabHandle | null) => void;
  /**
   * Report whether any of this codebase's terminals wants attention, so `App`
   * can flash this tab in the top strip when it is not the foreground one.
   */
  onAttentionChange: (root: string, hasAttention: boolean) => void;
  /**
   * Report a one-shot event worth showing on this codebase's tab while it is in
   * the background: a build that succeeded or failed, or a minimized terminal
   * that finished.
   *
   * Separate from {@link onAttentionChange} because the two have different
   * lifetimes. Attention is *state* — a terminal is asking for you until it is
   * restored — so it is pushed up as a boolean that can go back down. These are
   * *events*: nothing about the codebase is still true afterwards, so `App`
   * latches them and the user clears them by looking.
   */
  onSignal: (root: string, signal: TabSignal) => void;
  /**
   * The optional features that are switched on, or `null` while the startup load
   * is in flight. Passed down rather than fetched here so every open codebase
   * renders the same answer from one read, and so the strip never flickers.
   */
  features: FeatureInfo[] | null;
}) {
  const [tab, setTab] = useState<Tab>("run");

  /**
   * The tabs this build actually shows, after the optional-feature gate.
   * Recomputed from `features` rather than stored, so switching a feature off in
   * the picker is reflected without a reopen.
   */
  const shownTabs = visibleTabs(TABS, features, FEATURE_BY_TAB);

  /**
   * Whether a tab survived the gate. Asked of `shownTabs` rather than of
   * `featureEnabled` again so a gated body and the tab strip cannot disagree:
   * there is one filtered list and both read it.
   */
  const tabShown = (id: Tab) => shownTabs.some((t) => t.id === id);

  /**
   * Keep the selected tab on something that still exists. Turning off the
   * feature that owns the tab you are *looking at* would otherwise leave a tab
   * strip with nothing beneath it; `tabAfterDisable` falls back to the first
   * visible tab and leaves a still-visible selection alone.
   */
  useEffect(() => {
    const next = tabAfterDisable(tab, shownTabs);
    if (next !== null && next !== tab) setTab(next as Tab);
  }, [tab, shownTabs]);

  useEffect(() => {
    if (!active) return;
    const registrations = shownTabs.map(({ id }) => registerCommand(`view.${id}`, () => setTab(id)));
    return () => registrations.forEach((unregister) => unregister());
  }, [active, shownTabs]);
  const [showSetup, setShowSetup] = useState(false);
  const [inspectRequest, setInspectRequest] = useState<InspectRequest | null>(null);
  const [openRequest, setOpenRequest] = useState<OpenFileRequest | null>(null);
  const [selectRequest, setSelectRequest] = useState<SelectConfigRequest | null>(null);
  const requestToken = useRef(0);

  const [agentPanel, setAgentPanel] = useState<{
    initialPromptId?: string;
    initialPromptBody?: string;
    initialMode: AgentMode;
    title: string;
    initialContext?: string;
    token: number;
  } | null>(null);

  const [behavioralPanel, setBehavioralPanel] = useState<{
    configId: string;
    httpFiles: string[] | null;
    verify: boolean;
    token: number;
  } | null>(null);
  const [behavioralReport, setBehavioralReport] = useState<BehavioralReport | null>(null);

  /**
   * Why the last ask did not open a terminal, if it did not. Shown rather than
   * swallowed: the command build can refuse (an agent that left PATH between the
   * picker and the click, a model it no longer offers), and a click that silently
   * does nothing is indistinguishable from the app being broken.
   */
  const [askError, setAskError] = useState<string | null>(null);

  const [terminals, setTerminals] = useState<TerminalDescriptor[]>([]);
  const terminalSeq = useRef(0);

  // Which terminal is in front, bottom-most key first. Kept *beside* `terminals`
  // rather than by reordering it: the array index places each pill and cascade
  // offset, so raising by reordering would teleport pills and shift un-dragged
  // panels. One reconciling effect keeps this in step with what is open, so it
  // cannot drift the way separate edits in `openTerminal`/`closeTerminal` could.
  const [stackOrder, setStackOrder] = useState<string[]>([]);
  useEffect(() => {
    setStackOrder((order) =>
      syncStackOrder(
        order,
        terminals.map((t) => t.key),
      ),
    );
  }, [terminals]);

  // Which of this codebase's terminals currently want attention (bell while
  // minimized). Aggregated so `App` flashes the tab while any of them does.
  const [attentionKeys, setAttentionKeys] = useState<Set<string>>(() => new Set());
  const setTerminalAttention = (key: string, wants: boolean) =>
    setAttentionKeys((prev) => {
      if (prev.has(key) === wants) return prev; // no change, keep the reference
      const next = new Set(prev);
      if (wants) next.add(key);
      else next.delete(key);
      return next;
    });

  const hasAttention = attentionKeys.size > 0;
  // Push the aggregate up; clear it when this tab unmounts (codebase closed).
  useEffect(() => {
    onAttentionChange(workspace.root, hasAttention);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasAttention, workspace.root]);
  useEffect(() => {
    return () => onAttentionChange(workspace.root, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function nextToken() {
    requestToken.current += 1;
    return requestToken.current;
  }

  const openTerminal = () => {
    terminalSeq.current += 1;
    setTerminals((open) => [...open, makeTerminal(terminalSeq.current, workspace.root)]);
  };
  /**
   * Open a terminal running an agent that has already been asked `question`.
   *
   * The command line is built by the **backend** (`agent_interactive_command`
   * over `cb_core::review`), never assembled here: the argument order, the model
   * validation and the three refusals are one decision and live in one place. A
   * failure — an agent that vanished from PATH between the picker and the click,
   * a model the agent does not offer — therefore surfaces as a message rather
   * than as a terminal spawning something wrong.
   *
   * The question crosses as an **argv argument, never as typed keystrokes**.
   * `PtyManager` spawns via `CommandBuilder` with the args as they stand, so
   * nothing on this side joins or re-splits them. On Windows that is not the
   * same as "no shell": an agent name can resolve to a `.cmd`/`.bat` shim, and
   * `cmd.exe` re-parses the command line, so the backend refuses a question
   * carrying `&`, `|`, `<`, `>`, `^`, `"` or `%` for such a target before
   * spawning (the terminal then shows that reason). Through a real executable
   * the guard does not apply and the question crosses verbatim. Typing a
   * multi-line question into
   * the agent's TUI would instead submit it at the first `
`, asking only a
   * fragment; and `TerminalPanel` resolves its session id asynchronously, so an
   * early write would be dropped entirely.
   */
  const openAskTerminal = (question: string, agentId: string, model: string | undefined) => {
    void api
      .agentInteractiveCommand(agentId, model, question)
      .then((command) => {
        terminalSeq.current += 1;
        setTerminals((open) => [
          ...open,
          makeAgentTerminal(
            terminalSeq.current,
            workspace.root,
            command.program,
            command.args,
            terminalTitle(question),
          ),
        ]);
      })
      .catch((e) => setAskError(String(e)));
  };

  const closeTerminal = (key: string) =>
    setTerminals((open) => open.filter((t) => t.key !== key));
  const renameTerminalTo = (key: string, title: string) =>
    setTerminals((open) => renameTerminal(open, key, title));
  const recolorTerminalTo = (key: string, color: string | undefined) =>
    setTerminals((open) => recolorTerminal(open, key, color));

  const openRunAgent = (promptId: string) =>
    setAgentPanel({
      initialPromptId: promptId,
      initialMode: "read-only",
      title: "Run agent",
      token: nextToken(),
    });

  const openReview = () =>
    setAgentPanel({ initialMode: "read-only", title: "Adversarial review", token: nextToken() });

  const openNoteInAgent = (note: Note) =>
    setAgentPanel({
      initialMode: "read-only",
      title: sendToAgentTitle(note),
      initialPromptBody: note.body,
      token: nextToken(),
    });

  const openVerifyClaims = (context: string) =>
    setAgentPanel({
      initialPromptId: "verify-claims",
      initialMode: "read-only",
      title: "Verify claims",
      initialContext: context,
      token: nextToken(),
    });

  const openBehavioral = (configId: string, httpFiles: string[] | null, verify: boolean) =>
    setBehavioralPanel({ configId, httpFiles, verify, token: nextToken() });

  function requestInspect(request: InspectRequest) {
    setInspectRequest(request);
    setTab("inspect");
  }

  function requestOpenFile(path: string, name: string, line?: number) {
    requestToken.current += 1;
    setOpenRequest({ path, name, line, token: requestToken.current });
    setTab("run");
  }

  function requestSelectConfig(configId: string) {
    requestToken.current += 1;
    setSelectRequest({ configId, token: requestToken.current });
    setTab("run");
  }

  // Register this tab's action handle so the global titlebar and Notes panel can
  // reach the foreground tab. A stable object reads through a ref so `App` never
  // needs to re-register when a handler identity changes between renders.
  const handleRef = useRef<WorkspaceTabHandle>({
    openTerminal,
    openAskTerminal,
    openRunAgent,
    openReview,
    openNoteInAgent,
  });
  handleRef.current = { openTerminal, openAskTerminal, openRunAgent, openReview, openNoteInAgent };
  useEffect(() => {
    const stable: WorkspaceTabHandle = {
      openTerminal: () => handleRef.current.openTerminal(),
      openAskTerminal: (question, agentId, model) =>
        handleRef.current.openAskTerminal(question, agentId, model),
      openRunAgent: (id) => handleRef.current.openRunAgent(id),
      openReview: () => handleRef.current.openReview(),
      openNoteInAgent: (note) => handleRef.current.openNoteInAgent(note),
    };
    onRegister(workspace.root, stable);
    return () => onRegister(workspace.root, null);
  }, [workspace.root, onRegister]);

  // First-open setup prompt: offered when this workspace lacks the agent hooks
  // and the prompt has not been dismissed for it.
  useEffect(() => {
    let live = true;
    void Promise.all([api.intentCaptureStatus(), api.qualityGateStatus("claudeCode")])
      .then(([providers, gate]) => {
        if (live) setShowSetup(shouldPrompt(providers, gate, localStorage, workspace.root));
      })
      .catch(() => {
        /* status unavailable — do not prompt */
      });
    return () => {
      live = false;
    };
  }, [workspace.root]);

  return (
    <div className="workspace-tab" hidden={!active}>
      <div className="tabs tabs-row">
        {shownTabs.map(({ id, label }) => (
          <button key={id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </div>

      {/* Run, Tests and Objects stay mounted while hidden (they own processes and
          consoles); Changes, History and Architecture mount only while this is
          the foreground tab and their inner tab is chosen, so a background
          codebase never re-reads disk or polls git for the active pointer. */}
      <div className="body" hidden={tab !== "run"}>
        <RunView
          workspace={workspace}
          onWorkspaceChange={onWorkspaceChange}
          onInspect={requestInspect}
          pendingOpen={openRequest}
          onOpenConsumed={() => setOpenRequest(null)}
          pendingSelect={selectRequest}
          onSelectConsumed={() => setSelectRequest(null)}
          onNavigate={requestOpenFile}
          onProcessResult={(ok) => onSignal(workspace.root, ok ? "success" : "error")}
          active={active && tab === "run"}
        />
      </div>
      <div className="body" hidden={tab !== "tests"}>
        <TestsView
          workspace={workspace}
          onInspect={requestInspect}
          onResult={(success) => onSignal(workspace.root, success ? "success" : "error")}
        />
      </div>
      {/* Two different gates, and the distinction is the point.

          *Hidden* (the feature is on, another tab is in front): stays mounted,
          like Run/Tests/Objects and unlike the conditionally-mounted views,
          because it owns live database connections and a query that may still
          be streaming rows — none of which survives an unmount.

          *Switched off* (the feature is off): unmounted. The mounted-while-
          hidden convention is about a tab the user can still reach in one
          click; a disabled feature is not that. Left mounted it would keep
          calling `sql_list_connections` on mount, keep its CodeMirror instance
          alive, and keep a query streaming against a live database with no
          route to the rows and no route to Stop — the user turned the console
          off and the console kept running.

          Abandoning an in-flight query on unmount is bounded, not a leak:
          `sql_execute` keeps draining its internal channel after `channel.send`
          starts failing (a closed frontend channel stops the sending, not the
          draining), calls `state.sql.finish`, and `run_plan` drops the driver
          connection when the statement completes. Nothing waits on this side.
          What is *not* claimed: this is not a server-side cancel, so a long
          statement runs to completion on the server exactly as Stop would have
          left it.

          `tabAfterDisable` above has already moved the selection off the tab,
          so `hidden` and the mount gate never contradict each other. */}
      {tabShown("sql") && (
        <div className="body" hidden={tab !== "sql"}>
          <SqlView workspace={workspace} />
        </div>
      )}
      <div className="body" hidden={tab !== "inspect"}>
        <InspectView
          workspace={workspace}
          pendingRequest={inspectRequest}
          onRequestConsumed={() => setInspectRequest(null)}
        />
      </div>
      {active && tab === "changes" && (
        <div className="body">
          <ChangesView
            workspace={workspace}
            behavioral={behavioralReport}
            onOpenReview={openReview}
            onRunBehavioral={(configId, httpFiles) => openBehavioral(configId, httpFiles, false)}
            onVerifyClaims={(configId, httpFiles) => openBehavioral(configId, httpFiles, true)}
          />
        </div>
      )}
      {active && tab === "history" && (
        <div className="body">
          <HistoryView />
        </div>
      )}
      {active && tab === "architecture" && (
        <div className="body">
          <ArchitectureView workspace={workspace} onOpenFile={requestOpenFile} />
        </div>
      )}

      <SearchEverywhere
        workspace={workspace}
        active={active}
        onOpenFile={requestOpenFile}
        onRunAction={requestSelectConfig}
      />

      {/* Gated on the feature rather than mounted-and-inert: when `askCodebase`
          is off `AskPanel` registers no key listener at all, so Ctrl+/ returns
          cleanly to CodeMirror's comment toggle. */}
      <AskPanel
        active={active}
        enabled={featureEnabled(features, "askCodebase")}
        onAsk={openAskTerminal}
      />

      {askError !== null && (
        <div className="ask-error-toast" onClick={() => setAskError(null)}>
          {askError}
        </div>
      )}

      {showSetup && (
        <SetupPrompt
          onDismiss={() => setShowSetup(false)}
          onDontAskAgain={() => {
            setDismissed(localStorage, workspace.root);
            setShowSetup(false);
          }}
          onInstalled={() => setShowSetup(false)}
        />
      )}

      {agentPanel && (
        <ReviewPanel
          key={`${agentPanel.title}:${agentPanel.initialPromptId ?? ""}:${agentPanel.token}`}
          onClose={() => setAgentPanel(null)}
          initialPromptId={agentPanel.initialPromptId}
          initialPromptBody={agentPanel.initialPromptBody}
          initialMode={agentPanel.initialMode}
          initialContext={agentPanel.initialContext}
          title={agentPanel.title}
        />
      )}

      {behavioralPanel && (
        <BehavioralPanel
          key={behavioralPanel.token}
          configId={behavioralPanel.configId}
          httpFiles={behavioralPanel.httpFiles}
          verify={behavioralPanel.verify}
          onReport={setBehavioralReport}
          onVerify={openVerifyClaims}
          onClose={() => setBehavioralPanel(null)}
        />
      )}

      {terminals.map((t, index) => (
        <TerminalPanel
          key={t.key}
          title={t.title}
          cwd={t.cwd}
          command={t.command}
          index={index}
          stackOffset={stackOffset(stackOrder, t.key)}
          color={t.color}
          workspaceActive={active}
          onClose={() => closeTerminal(t.key)}
          onRaise={() => setStackOrder((order) => raiseTerminal(order, t.key))}
          onAttentionChange={(wants) => setTerminalAttention(t.key, wants)}
          onCompleted={(success) => onSignal(workspace.root, success ? "done" : "error")}
          onRename={(title) => renameTerminalTo(t.key, title)}
          onRecolor={(color) => recolorTerminalTo(t.key, color)}
        />
      ))}
    </div>
  );
}
