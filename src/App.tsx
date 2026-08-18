import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ArchitectureView } from "./views/ArchitectureView";
import { BranchMenu } from "./components/BranchMenu";
import { ChangesView } from "./views/ChangesView";
import { MenuBar } from "./components/MenuBar";
import { HistoryView } from "./views/HistoryView";
import { InspectView } from "./views/InspectView";
import { RunView } from "./views/RunView";
import { ReviewPanel } from "./components/ReviewPanel";
import { SearchEverywhere } from "./components/SearchEverywhere";
import { TestsView } from "./views/TestsView";
import * as api from "./ipc/api";
import type { AgentMode } from "./ipc/api";
import { applyEditorFontSize, loadEditorFontSize } from "./editorFontSize";
import { DEFAULT_EDITOR_FONT_SIZE, recogniseFontSizeShortcut, stepFontSize } from "./editorFontSizeLogic";
import { loadRecents, rememberRecent } from "./recentsLogic";
import type { InspectTarget, RootSpec, Workspace } from "./ipc/types";

type Tab = "tests" | "run" | "changes" | "history" | "architecture" | "inspect";

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

/**
 * A file the search palette asked to be opened, held until the Run tab has it.
 *
 * The editor state itself is **not** lifted: `RunView` still owns `openFiles`,
 * `activeFile` and `openFile`, and this is only the request to call it, the same
 * shape as `InspectRequest` above. Lifting the editor would move a pane's worth
 * of state up here to serve one keystroke, and the two views would then have to
 * agree about tab order, dirty files and focus.
 *
 * `token` is what makes the request re-fire. The interesting case is choosing a
 * symbol in a file that is *already* open: the path is unchanged, so an equality
 * check on path — or on the whole object were it rebuilt from equal fields —
 * would decide nothing had happened and leave the user looking at the line they
 * jumped from. A number that only ever goes up cannot collide with itself.
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
 *
 * Starting a process off a fuzzy-matched keystroke is the kind of wrong answer
 * this codebase refuses on principle: the match is a guess about what was meant,
 * and the cost of guessing wrong is a build, a port, or a service talking to
 * something real. Selecting puts the configuration under the Run button and
 * leaves the decision to press it where it was.
 */
export interface SelectConfigRequest {
  configId: string;
  token: number;
}

const TABS: { id: Tab; label: string }[] = [
  { id: "run", label: "Run" },
  { id: "tests", label: "Tests" },
  { id: "changes", label: "Changes" },
  { id: "history", label: "History" },
  // The id matches the label on purpose. `inspect`/"Objects" below is the one
  // place they differ, and CLAUDE.md complains about it by name: grepping the
  // tree for "Objects" never finds the view that draws it. One such trap is
  // enough.
  { id: "architecture", label: "Architecture" },
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
  // The agent panel (adversarial Review + Enhancements "Run Agent") is hosted
  // here, not in a tab, so a running agent survives switching tabs. One slot,
  // one agent at a time; a new request replaces the previous panel.
  const [agentPanel, setAgentPanel] = useState<{
    initialPromptId?: string;
    initialMode: AgentMode;
    title: string;
  } | null>(null);

  /** Open the agent panel as an adversarial review (Changes tab + menu bar). */
  const openReview = () =>
    setAgentPanel({ initialMode: "read-only", title: "Adversarial review" });

  /** Send the user to the Objects tab with something already chosen to read. */
  function requestInspect(request: InspectRequest) {
    setInspectRequest(request);
    setTab("inspect");
  }

  /**
   * What the palette chose, held only until the Run tab has consumed it.
   *
   * Same one-slot arrangement as `inspectRequest`: there is one user pressing
   * Enter one row at a time, and a queue would only let a stale choice arrive
   * after the one that replaced it.
   */
  const [openRequest, setOpenRequest] = useState<OpenFileRequest | null>(null);
  const [selectRequest, setSelectRequest] = useState<SelectConfigRequest | null>(null);
  const requestToken = useRef(0);

  /** Send the user to the Run tab with a file open, and a line revealed. */
  function requestOpenFile(path: string, name: string, line?: number) {
    requestToken.current += 1;
    setOpenRequest({ path, name, line, token: requestToken.current });
    setTab("run");
  }

  /** Send the user to the Run tab with a configuration selected. */
  function requestSelectConfig(configId: string) {
    requestToken.current += 1;
    setSelectRequest({ configId, token: requestToken.current });
    setTab("run");
  }

  /**
   * The editor font size: restored on start, and driven by Ctrl+= / Ctrl+- /
   * Ctrl+0 from anywhere in the app.
   *
   * Registered here rather than in a view because the size is app-wide — the
   * diff, the file editor and the diagram editor all read it — and because the
   * keystroke should work whichever tab is showing. Capture phase, matching
   * `SearchEverywhere`: CodeMirror binds Ctrl+- itself in some keymaps, and a
   * bubble-phase listener would never see it.
   */
  useEffect(() => {
    applyEditorFontSize(loadEditorFontSize());

    const onKeyDown = (event: KeyboardEvent) => {
      const action = recogniseFontSizeShortcut(event);
      if (action === null) return;

      // Ctrl+= / Ctrl+- are the webview's own zoom, which would scale the
      // whole chrome rather than the code.
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
        {/* File (Open / Rescan / Exit) and Enhancements (Instructions / Prompts).
            The standalone Open…/Rescan buttons below remain as shortcuts. */}
        <MenuBar
          onOpen={pickFolder}
          onRescan={rescan}
          onRunAgent={(promptId) =>
            setAgentPanel({ initialPromptId: promptId, initialMode: "read-only", title: "Run agent" })
          }
          onOpenReview={openReview}
        />

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
          Changes, History and Architecture re-mount so they re-read what is on
          disk on every visit — git state for the first two, and for
          Architecture the manifests every diagram is derived from. */}
      <div className="body" hidden={tab !== "run"}>
        {/* `onNavigate` is an editor jump — Go to definition, a usage row — and
            is the third caller of this one request path, after the palette and
            the architecture diagram. Deliberately not a fourth mechanism, and
            deliberately not a shortcut into `RunView`'s own `openFile`: only
            `requestToken` makes a jump *inside the file already showing* fire at
            all. See `OpenFileRequest` above. */}
        <RunView
          key={workspace.root}
          workspace={workspace}
          onWorkspaceChange={setWorkspace}
          onInspect={requestInspect}
          pendingOpen={openRequest}
          onOpenConsumed={() => setOpenRequest(null)}
          pendingSelect={selectRequest}
          onSelectConsumed={() => setSelectRequest(null)}
          onNavigate={requestOpenFile}
          active={tab === "run"}
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
          <ChangesView key={workspace.root} onOpenReview={openReview} />
        </div>
      )}
      {tab === "history" && (
        <div className="body">
          <HistoryView key={workspace.root} />
        </div>
      )}
      {tab === "architecture" && (
        <div className="body">
          {/* Clicking a box routes through `requestOpenFile` — the same
              request-and-consume path the search palette uses, and deliberately
              not a third mechanism. It carries the monotonic token because
              jumping to a file that is already open changes no field the Run
              tab could compare, and switches the user to Run, which is where
              the only editor in this app lives. */}
          <ArchitectureView
            key={workspace.root}
            workspace={workspace}
            onOpenFile={requestOpenFile}
          />
        </div>
      )}

      {/* Inside the workspace branch only: with nothing open there is no index,
          no configurations and no editor to open a result in, so the palette
          would be a keystroke that produces an empty box. */}
      <SearchEverywhere
        key={workspace.root}
        onOpenFile={requestOpenFile}
        onRunAction={requestSelectConfig}
      />

      {/* Hosted here rather than in a tab: the agent runs as a background
          process and its panel minimizes to a pill, so it must outlive a tab
          switch. Mounted only while open (or minimized), and it cancels its
          process on close. Keyed so a new request remounts a fresh panel. */}
      {agentPanel && (
        <ReviewPanel
          key={`${agentPanel.title}:${agentPanel.initialPromptId ?? ""}`}
          onClose={() => setAgentPanel(null)}
          initialPromptId={agentPanel.initialPromptId}
          initialMode={agentPanel.initialMode}
          title={agentPanel.title}
        />
      )}
    </div>
  );
}
