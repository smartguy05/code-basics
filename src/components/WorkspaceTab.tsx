import { useEffect, useRef, useState } from "react";
import { ArchitectureView } from "../views/ArchitectureView";
import { BehavioralPanel } from "./BehavioralPanel";
import { ChangesView } from "../views/ChangesView";
import { HistoryView } from "../views/HistoryView";
import { InspectView } from "../views/InspectView";
import { RunView } from "../views/RunView";
import { ReviewPanel } from "./ReviewPanel";
import { SearchEverywhere } from "./SearchEverywhere";
import { SetupPrompt } from "./SetupPrompt";
import { shouldPrompt, setDismissed } from "./setupPromptLogic";
import { TerminalPanel } from "./TerminalPanel";
import { TestsView } from "../views/TestsView";
import {
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
import type { BehavioralReport, Note, Workspace } from "../ipc/types";
import type { InspectRequest, OpenFileRequest, SelectConfigRequest } from "../App";

type Tab = "tests" | "run" | "changes" | "history" | "architecture" | "inspect";

const TABS: { id: Tab; label: string }[] = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
  { id: "changes", label: "Changes" },
  { id: "history", label: "History" },
  { id: "architecture", label: "Architecture" },
  { id: "inspect", label: "Objects" },
];

/**
 * The actions the titlebar (global chrome) and the global Notes panel route to
 * the *foreground* workspace. Each open tab registers one of these with `App`;
 * `App` invokes the active tab's handle.
 */
export interface WorkspaceTabHandle {
  openTerminal(): void;
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
}) {
  const [tab, setTab] = useState<Tab>("run");
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
    openRunAgent,
    openReview,
    openNoteInAgent,
  });
  handleRef.current = { openTerminal, openRunAgent, openReview, openNoteInAgent };
  useEffect(() => {
    const stable: WorkspaceTabHandle = {
      openTerminal: () => handleRef.current.openTerminal(),
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
        {TABS.map(({ id, label }) => (
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
          onBuildResult={(ok) => onSignal(workspace.root, ok ? "success" : "error")}
          active={active && tab === "run"}
        />
      </div>
      <div className="body" hidden={tab !== "tests"}>
        <TestsView workspace={workspace} onInspect={requestInspect} />
      </div>
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
          index={index}
          stackOffset={stackOffset(stackOrder, t.key)}
          color={t.color}
          onClose={() => closeTerminal(t.key)}
          onRaise={() => setStackOrder((order) => raiseTerminal(order, t.key))}
          onAttentionChange={(wants) => setTerminalAttention(t.key, wants)}
          onCompleted={() => onSignal(workspace.root, "done")}
          onRename={(title) => renameTerminalTo(t.key, title)}
          onRecolor={(color) => recolorTerminalTo(t.key, color)}
        />
      ))}
    </div>
  );
}
