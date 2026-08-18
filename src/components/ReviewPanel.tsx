import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "./OutputConsole";
import * as api from "../ipc/api";
import type { ProcessEvent, PromptInfo, ReviewAgentInfo } from "../ipc/types";
import {
  defaultAgentId,
  defaultModel,
  defaultPromptId,
  modelsFor,
  reviewStatus,
  type ReviewPhase,
} from "./reviewLogic";
import {
  claudeLineNeedsAttention,
  createNdjsonBuffer,
  formatClaudeStream,
} from "./reviewStreamLogic";

/**
 * The adversarial-review panel: pick an agent (Claude Code / Codex), a prompt
 * and a model, run it read-only against the open workspace, and watch it stream.
 *
 * Non-blocking by design. It is a floating panel, not a modal — no backdrop
 * captures clicks, so the rest of the app stays usable while a review runs. It
 * **minimizes to a loader pill** rather than closing, and the console stays
 * mounted while minimized so output keeps arriving. Hosted at the app level, so
 * a running review survives switching tabs.
 */
export function ReviewPanel({ onClose }: { onClose: () => void }) {
  const consoleRef = useRef<ConsoleHandle>(null);
  const [agents, setAgents] = useState<ReviewAgentInfo[]>([]);
  const [prompts, setPrompts] = useState<PromptInfo[]>([]);
  const [agentId, setAgentId] = useState<string | undefined>();
  const [promptId, setPromptId] = useState<string | undefined>();
  const [model, setModel] = useState<string | undefined>();
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
        setAgentId((cur) => cur ?? defaultAgentId(ags));
        setPromptId((cur) => cur ?? defaultPromptId(ps));
        setModel((cur) => cur ?? defaultModel(modelsFor(ags, defaultAgentId(ags))));
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
    // Model choices are per-agent, so re-seed the model for the new agent.
    setModel(defaultModel(modelsFor(agents, id)));
  };

  const start = () => {
    if (!promptId || !agentId || running) return;
    setError(null);
    setLast(null);
    setAttention(false);
    consoleRef.current?.clear();
    setPhase("running");

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
      .startReview(promptId, agentId, models.length ? model : undefined, (event) => {
        if (isClaude && event.type === "output" && event.stream === "stdout") {
          renderClaude(ndjson.push(event.text));
          return;
        }
        if (event.type === "exited" || event.type === "failed") {
          if (isClaude) renderClaude(ndjson.flush());
          consoleRef.current?.handle(event);
          setLast(event);
          setPhase("done");
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

  const status = reviewStatus(phase, last);

  // Minimized: a compact restore pill. The console stays mounted (hidden) below
  // so events keep flowing into it.
  return (
    <>
      {minimized && (
        <button
          className={`review-pill${attention ? " attention" : ""}`}
          onClick={restore}
          title={attention ? "The review needs your attention" : "Restore the review panel"}
        >
          {running && <span className="review-spinner" aria-hidden />}
          <span>Review — {attention ? "needs attention" : status}</span>
        </button>
      )}

      <div className="review-panel" hidden={minimized}>
        <div className={`review-header${attention ? " attention" : ""}`}>
          <strong>Adversarial review</strong>
          <span className="faint" style={{ fontSize: 12 }}>
            {running && <span className="review-spinner" aria-hidden style={{ marginRight: 6 }} />}
            {status}
          </span>
          <span style={{ flex: 1 }} />
          <button onClick={() => setMinimized(true)} title="Minimize (keeps running)">
            —
          </button>
          <button onClick={close} title="Close (stops the review)">
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
          {running ? (
            <button className="primary" onClick={cancel}>
              Cancel
            </button>
          ) : (
            <button className="primary" disabled={!promptId || !agentId} onClick={start}>
              Run review
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
