import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as api from "../ipc/api";
import type { ReviewAgentInfo } from "../ipc/types";
import {
  TEXT_ENTRY_ANCESTORS,
  canAsk,
  launchBlockedReason,
  shouldAbstainForFocus,
} from "./askLogic";
import { registerCommand } from "../shortcuts";
import type { FocusedSurface } from "./askLogic";
import {
  loadAgentPrefs,
  modelsFor,
  preferredAgentId,
  preferredModel,
  saveAgentPrefs,
} from "./reviewLogic";

/**
 * The focused element, reduced to the facts `shouldAbstainForFocus` decides on.
 *
 * Pure DOM and nothing else — every selector it asks about comes from
 * `askLogic`, so widening the rule is a change to that file's exported lists and
 * their tests, not to this query. `closest()` rather than a match on the element
 * itself because focus inside either surface lands on an inner node: CodeMirror
 * focuses `.cm-content`, and xterm focuses a hidden `.xterm-helper-textarea`.
 *
 * `document.body` is what `activeElement` reports when nothing is focused, and
 * it is reported here as `null` — "nothing focused" is the shortcut's own case,
 * and letting it arrive as a `<body>` descriptor would leave that decision
 * depending on a tag name rather than on the fact it means.
 */
function describeFocus(): FocusedSurface | null {
  const el = document.activeElement;
  if (!(el instanceof Element)) return null;
  if (el === document.body) return null;
  return {
    tagName: el.tagName,
    // `[contenteditable]` alone would match `contenteditable="false"`, which is
    // an explicit *opt out*; the `:not` keeps `""`, `"true"` and
    // `"plaintext-only"` without enumerating them.
    contentEditable: el.closest("[contenteditable]:not([contenteditable='false'])") !== null,
    ancestors: TEXT_ENTRY_ANCESTORS.filter((selector) => el.closest(selector) !== null),
  };
}

/**
 * "Ask the codebase": a small modal that takes a question and an agent, and
 * opens a real interactive terminal running that agent with the question
 * already asked.
 *
 * Modelled on `LauncherPicker` — the same `.launcher-overlay` backdrop, the
 * same click-outside-to-close, the same "type and press Enter" shape — because
 * it is the same kind of thing: a transient box that starts a process and gets
 * out of the way. It is **not** the `ReviewPanel`, which hosts a headless run
 * and its console; this one hands the session to a terminal and closes.
 *
 * Every decision it makes is in the pure, tested `askLogic` (which chord this
 * is, whether the button may be pressed, why it may not, what the terminal tab
 * is called) or in `reviewLogic` (the remembered agent and model) — except the
 * one that touches the DOM, which is documented at its call site below.
 */
export function AskPanel({
  active,
  enabled,
  onAsk,
}: {
  /**
   * Whether this is the foreground codebase. The key listener is window-level,
   * so without this gate every open codebase's panel would open at once on a
   * single chord — the same reason `SearchEverywhere` takes it.
   */
  active: boolean;
  /**
   * Whether the `askCodebase` feature is on. When it is off the listener is
   * never registered at all, rather than registered and made to abstain: Ctrl+/
   * must return **cleanly** to CodeMirror's `toggleComment`, and a handler that
   * decides not to act is one more thing between the key and the editor.
   */
  enabled: boolean;
  /** Open a terminal asking `question` of `agentId` (with `model`, if any). */
  onAsk: (question: string, agentId: string, model: string | undefined) => void;
}) {
  const [open, setOpen] = useState(false);
  const [question, setQuestion] = useState("");
  const [agents, setAgents] = useState<ReviewAgentInfo[]>([]);
  const [agentId, setAgentId] = useState<string | undefined>(undefined);
  const [model, setModel] = useState<string | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!enabled || !active) return;
    return registerCommand("agent.ask", () => {
      if (shouldAbstainForFocus(describeFocus())) return;
      setOpen(true);
      setTimeout(() => inputRef.current?.focus(), 0);
    });
  }, [active, enabled]);

  // Read the installed agents when the box opens. `review_agents` resolves each
  // CLI on PATH, so it is re-read per open rather than cached for the session:
  // an agent installed while the app was running should appear without a
  // restart.
  useEffect(() => {
    if (!open) return;
    let live = true;
    api
      .reviewAgents()
      .then((list) => {
        if (!live) return;
        setAgents(list);
        // The agent and model are shared with the Review panel — one "which
        // agent do I use" answer for the whole app, not two that drift.
        const prefs = loadAgentPrefs(localStorage);
        const chosen = preferredAgentId(prefs, list);
        setAgentId(chosen);
        setModel(preferredModel(prefs, list, chosen));
      })
      .catch((e) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [open]);

  const models = useMemo(() => modelsFor(agents, agentId), [agents, agentId]);
  const blocked = launchBlockedReason(agents, agentId);

  const close = useCallback(() => {
    setOpen(false);
    setError(null);
  }, []);

  const ask = () => {
    // Both guards, not one: `blocked` is about the agent and `canAsk` about the
    // question, and they fail for different reasons the user must fix
    // differently. Neither opens a terminal.
    if (blocked !== null) return;
    if (!canAsk(question)) return;
    if (agentId === undefined) return; // unreachable once `blocked` is null

    saveAgentPrefs(localStorage, { ...loadAgentPrefs(localStorage), agentId, model });
    onAsk(question.trim(), agentId, model);
    setQuestion("");
    close();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      return;
    }
    // Enter asks; Shift+Enter is a newline, because a question worth asking a
    // codebase is often several lines long.
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      ask();
    }
  };

  // `!enabled` as well as `!open`: the feature can be switched off in the picker
  // while the box is up, and a modal that outlived its feature would keep a
  // backdrop over the app with no way back to it.
  if (!open || !enabled) return null;

  const readyToAsk = blocked === null && canAsk(question);

  return (
    <div className="launcher-overlay" onMouseDown={close}>
      <div className="ask-panel" onMouseDown={(e) => e.stopPropagation()} onKeyDown={onKeyDown}>
        <div className="ask-header">
          <h2>Ask the codebase</h2>
          <button className="ask-close" title="Close" onClick={close}>
            ✕
          </button>
        </div>

        <textarea
          ref={inputRef}
          className="ask-question"
          placeholder="What do you want to know? Enter to ask, Shift+Enter for a new line."
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
        />

        <div className="ask-options">
          <label className="ask-field">
            <span>Agent</span>
            <select
              value={agentId ?? ""}
              onChange={(e) => {
                const next = e.target.value;
                setAgentId(next);
                // The remembered model belongs to the *previous* agent and may
                // mean nothing to this one, so it is re-derived rather than kept.
                setModel(preferredModel(loadAgentPrefs(localStorage), agents, next));
              }}
            >
              {agents.length === 0 && <option value="">No agent installed</option>}
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label}
                </option>
              ))}
            </select>
          </label>

          {/* Hidden when the agent offers none: an empty list means the agent
              runs with its own configured default, which is not a choice we can
              present. */}
          {models.length > 0 && (
            <label className="ask-field">
              <span>Model</span>
              <select value={model ?? ""} onChange={(e) => setModel(e.target.value)}>
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </label>
          )}

          <button className="ask-go" disabled={!readyToAsk} onClick={ask}>
            Ask
          </button>
        </div>

        {/* The reason is shown, never just a greyed-out button: the three ways
            this can be blocked need three different actions from the user. */}
        {blocked !== null && <div className="ask-blocked">{blocked}</div>}
        {error !== null && <div className="ask-error">{error}</div>}
      </div>
    </div>
  );
}
