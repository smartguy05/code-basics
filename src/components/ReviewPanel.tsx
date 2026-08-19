import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "./OutputConsole";
import * as api from "../ipc/api";
import type { AgentMode } from "../ipc/api";
import type { ProcessEvent, PromptInfo, ReviewAgentInfo } from "../ipc/types";
import {
  loadAgentPrefs,
  modelsFor,
  preferredAgentId,
  preferredModel,
  preferredPromptId,
  reviewStatus,
  saveAgentPrefs,
  type AgentPrefs,
  type ReviewPhase,
} from "./reviewLogic";
import {
  claudeLineNeedsAttention,
  createNdjsonBuffer,
  formatClaudeStream,
} from "./reviewStreamLogic";
import {
  clampPanelPosition,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
} from "./reviewLayoutLogic";

/**
 * The agent panel: pick an agent (Claude Code / Codex), a prompt, a model and a
 * posture (read-only / allow-edits), run it against the open workspace, and
 * watch it stream. Serves both the adversarial **Review** and the Enhancements
 * **Run Agent** action.
 *
 * Non-blocking by design. It is a floating panel, not a modal — no backdrop
 * captures clicks, so the rest of the app stays usable while it runs. It
 * **minimizes to a loader pill** rather than closing, and the console stays
 * mounted while minimized so output keeps arriving. Hosted at the app level, so
 * a running agent survives switching tabs.
 *
 * `initialPromptId` pre-selects a prompt (the Run Agent entry); absent, the
 * canonical review prompt leads (the Review entry). `initialMode` seeds the
 * posture toggle. `title` labels the header and pill.
 */
export function ReviewPanel({
  onClose,
  initialPromptId,
  initialMode = "read-only",
  title = "Adversarial review",
}: {
  onClose: () => void;
  initialPromptId?: string;
  initialMode?: AgentMode;
  title?: string;
}) {
  const consoleRef = useRef<ConsoleHandle>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  // The panel's dragged position (top/left anchor). Undefined keeps the default
  // bottom-right CSS anchor; a stored layout seeds it so the panel reopens where
  // the user last left it.
  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  // The last-run selection, remembered across opens (agent/model/prompt only —
  // the edit posture is deliberately never sticky).
  const [prefs] = useState<AgentPrefs>(() => loadAgentPrefs(localStorage));
  const [agents, setAgents] = useState<ReviewAgentInfo[]>([]);
  const [prompts, setPrompts] = useState<PromptInfo[]>([]);
  const [agentId, setAgentId] = useState<string | undefined>();
  const [promptId, setPromptId] = useState<string | undefined>(initialPromptId);
  const [model, setModel] = useState<string | undefined>();
  const [mode, setMode] = useState<AgentMode>(initialMode);
  const [phase, setPhase] = useState<ReviewPhase>("idle");
  const [last, setLast] = useState<ProcessEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [minimized, setMinimized] = useState(false);
  // The agent needs the user: a permission was denied/blocked, or the run ended
  // while minimized. Flashes the pill until the panel is restored.
  const [attention, setAttention] = useState(false);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [ags, ps] = await Promise.all([api.reviewAgents(), api.listPrompts()]);
        if (!alive) return;
        setAgents(ags);
        setPrompts(ps);
        // Seed from the remembered selection, falling back to defaults. An
        // explicit initialPromptId (the Run Agent entry) still wins for the
        // prompt.
        const agent = preferredAgentId(prefs, ags);
        setAgentId((cur) => cur ?? agent);
        setPromptId((cur) => cur ?? preferredPromptId(initialPromptId, prefs, ps));
        setModel((cur) => cur ?? preferredModel(prefs, ags, agent));
      } catch (e) {
        if (alive) setError(String(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const running = phase === "running";
  const models = modelsFor(agents, agentId);

  const chooseAgent = (id: string) => {
    setAgentId(id);
    // Model choices are per-agent, so re-seed the model for the new agent
    // (keeping the remembered model when the new agent still offers it).
    setModel(preferredModel(prefs, agents, id));
  };

  const start = () => {
    if (!promptId || !agentId || running) return;
    setError(null);
    setLast(null);
    setAttention(false);
    consoleRef.current?.clear();
    setPhase("running");

    // Capture what is being run now, so a run-once prompt is recorded on a
    // successful finish even if the selection changes afterwards.
    const runPromptId = promptId;
    const runIsOnce = prompts.find((p) => p.id === runPromptId)?.once ?? false;

    // Remember this selection for the next open (posture excluded on purpose).
    saveAgentPrefs(localStorage, { agentId, model: models.length ? model : undefined, promptId });

    // Claude streams NDJSON (--output-format stream-json); render it into
    // readable console text. Codex's `exec` already prints human text, so its
    // output passes through untouched.
    const isClaude = agentId === "claude-code";
    const ndjson = createNdjsonBuffer();
    const renderClaude = (lines: string[]) => {
      for (const line of lines) {
        const text = formatClaudeStream(line);
        if (text) consoleRef.current?.write(text);
        // A denied/blocked action is the closest a headless review gets to
        // "requires input" — flash so a minimized panel gets noticed.
        if (claudeLineNeedsAttention(line)) setAttention(true);
      }
    };

    void api
      .startReview(promptId, agentId, models.length ? model : undefined, mode, (event) => {
        if (isClaude && event.type === "output" && event.stream === "stdout") {
          renderClaude(ndjson.push(event.text));
          return;
        }
        if (event.type === "exited" || event.type === "failed") {
          if (isClaude) renderClaude(ndjson.flush());
          consoleRef.current?.handle(event);
          setLast(event);
          setPhase("done");
          // A run-once prompt is recorded only on a clean, successful finish, so
          // a failed or cancelled setup can be retried without a confirm.
          if (
            runIsOnce &&
            event.type === "exited" &&
            event.success &&
            !event.cancelled
          ) {
            void api.markAgentRun(runPromptId).catch(() => {});
          }
          // A failure always warrants attention; a clean finish only needs it
          // when minimized (the visible panel already shows the result).
          setMinimized((min) => {
            if (event.type === "failed" || min) setAttention(true);
            return min;
          });
          return;
        }
        // Codex output, Claude's stderr, and the started banner pass through the
        // console's own renderer.
        consoleRef.current?.handle(event);
      })
      .catch((e) => {
        setError(String(e));
        setPhase("done");
      });
  };

  const cancel = () => void api.cancelReview();

  // Restoring the panel is the acknowledgement, so it clears the flash.
  const restore = () => {
    setMinimized(false);
    setAttention(false);
  };

  // Closing stops a running review — its console is going away with it.
  const close = () => {
    if (running) void api.cancelReview();
    onClose();
  };

  // Drag the panel by its header. The minimize/close buttons keep their normal
  // click behaviour — a press that lands on one of them is not a drag. The
  // clamp decision is pure (reviewLayoutLogic); this only wires the pointer.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    const panel = panelRef.current;
    if (!panel) return;

    const rect = panel.getBoundingClientRect();
    // Where inside the panel the pointer grabbed, so the panel follows the
    // cursor without jumping.
    const grabX = e.clientX - rect.left;
    const grabY = e.clientY - rect.top;
    const header = e.currentTarget;
    header.setPointerCapture(e.pointerId);

    let latest: PanelLayout = { left: rect.left, top: rect.top };
    // A press that never moves is a click, not a drag: it must not convert the
    // panel from its default bottom-right anchor to a persisted top/left one.
    let moved = false;
    const onMove = (ev: PointerEvent) => {
      moved = true;
      const size = { width: panel.offsetWidth, height: panel.offsetHeight };
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      latest = clampPanelPosition(
        { left: ev.clientX - grabX, top: ev.clientY - grabY },
        size,
        viewport,
      );
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  const status = reviewStatus(phase, last);

  // Minimized: a compact restore pill. The console stays mounted (hidden) below
  // so events keep flowing into it.
  return (
    <>
      {minimized && (
        <button
          className={`review-pill${attention ? " attention" : ""}`}
          onClick={restore}
          title={attention ? "The agent needs your attention" : "Restore the agent panel"}
        >
          {running && <span className="review-spinner" aria-hidden />}
          <span>
            {title} — {attention ? "needs attention" : status}
          </span>
        </button>
      )}

      <div
        className="review-panel"
        hidden={minimized}
        ref={panelRef}
        // Once dragged, switch from the bottom-right anchor to a top-left one so
        // the native resize grip grows the panel on-screen.
        style={
          pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : undefined
        }
      >
        <div
          className={`review-header${attention ? " attention" : ""}`}
          onPointerDown={onHeaderPointerDown}
        >
          <strong>{title}</strong>
          <span className="faint" style={{ fontSize: 12 }}>
            {running && <span className="review-spinner" aria-hidden style={{ marginRight: 6 }} />}
            {status}
          </span>
          <span style={{ flex: 1 }} />
          <button onClick={() => setMinimized(true)} title="Minimize (keeps running)">
            —
          </button>
          <button onClick={close} title="Close (stops the agent)">
            ✕
          </button>
        </div>

        <div className="review-controls">
          {agents.length > 1 && (
            <label>
              Agent
              <select
                value={agentId ?? ""}
                disabled={running}
                onChange={(e) => chooseAgent(e.target.value)}
              >
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.label}
                  </option>
                ))}
              </select>
            </label>
          )}
          {agents.length === 1 && (
            <span className="faint" style={{ fontSize: 12, alignSelf: "center" }}>
              {agents[0]?.label}
            </span>
          )}
          <label>
            Prompt
            <select
              value={promptId ?? ""}
              disabled={running || prompts.length === 0}
              onChange={(e) => setPromptId(e.target.value)}
            >
              {prompts.length === 0 && <option value="">No prompts found</option>}
              {prompts.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.title}
                </option>
              ))}
            </select>
          </label>
          {models.length > 0 && (
            <label>
              Model
              <select
                value={model ?? ""}
                disabled={running}
                onChange={(e) => setModel(e.target.value)}
              >
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label title="Read-only explores and reports; Allow edits lets the agent modify files">
            Mode
            <select
              value={mode}
              disabled={running}
              onChange={(e) => setMode(e.target.value as AgentMode)}
            >
              <option value="read-only">Read-only</option>
              <option value="edit">Allow edits</option>
            </select>
          </label>
          {running ? (
            <button className="primary" onClick={cancel}>
              Cancel
            </button>
          ) : (
            <button className="primary" disabled={!promptId || !agentId} onClick={start}>
              {mode === "edit" ? "Run (edits)" : "Run"}
            </button>
          )}
        </div>

        {agents.length === 0 && (
          <div className="warning">
            Neither Claude Code (`claude`) nor Codex (`codex`) is on your PATH.
          </div>
        )}
        {error && <div className="warning">{error}</div>}

        <div className="review-console">
          <OutputConsole ref={consoleRef} />
        </div>
      </div>
    </>
  );
}
