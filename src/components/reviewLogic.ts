//! Pure decisions for the adversarial-review panel — extracted so they are
//! testable without a DOM (vitest runs in the node environment).

import type { ProcessEvent, PromptInfo, ReviewAgentInfo } from "../ipc/types";

/** Where the review is in its lifecycle. */
export type ReviewPhase = "idle" | "running" | "done";

/**
 * The agent to pre-select: the first installed one (the backend returns them in
 * preference order, Claude Code first).
 */
export function defaultAgentId(agents: ReviewAgentInfo[]): string | undefined {
  return agents[0]?.id;
}

/** The models offered by the selected agent, or an empty list. */
export function modelsFor(agents: ReviewAgentInfo[], agentId: string | undefined): string[] {
  return agents.find((a) => a.id === agentId)?.models ?? [];
}

/**
 * The model to pre-select. The backend offers aliases most-capable-first, so we
 * prefer `opus` for a thorough review and otherwise take the first offered.
 * Undefined when the agent offers no models (it uses its own default).
 */
export function defaultModel(models: string[]): string | undefined {
  if (models.includes("opus")) return "opus";
  return models[0];
}

/**
 * The prompt to pre-select: the canonical `code-review` prompt if present, else
 * the first prompt whose id or title mentions "review", else the first prompt.
 */
export function defaultPromptId(prompts: PromptInfo[]): string | undefined {
  const exact = prompts.find((p) => p.id === "code-review");
  if (exact) return exact.id;
  const mentions = prompts.find(
    (p) => p.id.toLowerCase().includes("review") || p.title.toLowerCase().includes("review"),
  );
  if (mentions) return mentions.id;
  return prompts[0]?.id;
}

/**
 * A short human status line for the panel header. Keeps a cancellation, a clean
 * finish, a non-zero exit, and a spawn failure as four distinct answers rather
 * than collapsing them into one "stopped".
 */
export function reviewStatus(phase: ReviewPhase, last: ProcessEvent | null): string {
  if (phase === "idle") return "Idle";
  if (phase === "running") return "Reviewing…";
  if (last?.type === "failed") return `Failed: ${last.message}`;
  if (last?.type === "exited") {
    if (last.cancelled) return "Cancelled";
    if (last.success) return "Done";
    return `Exited (code ${last.code ?? "?"})`;
  }
  return "Done";
}

// --- Remembering the last-chosen agent/model/prompt ------------------------

/** What the panel remembers across opens. Mode is deliberately not persisted —
 * "allow edits" should be an explicit choice every run, not a sticky default. */
export interface AgentPrefs {
  agentId?: string;
  model?: string;
  promptId?: string;
}

const PREFS_KEY = "cb.agentPanel";

/** Read the remembered selection. A missing or unparseable value is empty. */
export function loadAgentPrefs(storage: Pick<Storage, "getItem">): AgentPrefs {
  try {
    const raw = storage.getItem(PREFS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const { agentId, model, promptId } = parsed as Record<string, unknown>;
    return {
      agentId: typeof agentId === "string" ? agentId : undefined,
      model: typeof model === "string" ? model : undefined,
      promptId: typeof promptId === "string" ? promptId : undefined,
    };
  } catch {
    return {};
  }
}

/** Remember the selection just run. Never throws (storage may be unavailable). */
export function saveAgentPrefs(storage: Pick<Storage, "setItem">, prefs: AgentPrefs): void {
  try {
    storage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Ignore: persistence is a convenience, not a requirement.
  }
}

/**
 * The agent to pre-select: the remembered one if it is still installed,
 * otherwise the default (first in preference order).
 */
export function preferredAgentId(
  prefs: AgentPrefs,
  agents: ReviewAgentInfo[],
): string | undefined {
  if (prefs.agentId && agents.some((a) => a.id === prefs.agentId)) return prefs.agentId;
  return defaultAgentId(agents);
}

/**
 * The model to pre-select: the remembered one if the chosen agent still offers
 * it, otherwise that agent's default.
 */
export function preferredModel(
  prefs: AgentPrefs,
  agents: ReviewAgentInfo[],
  agentId: string | undefined,
): string | undefined {
  const models = modelsFor(agents, agentId);
  if (prefs.model && models.includes(prefs.model)) return prefs.model;
  return defaultModel(models);
}

/**
 * The prompt to pre-select. An explicit `initialPromptId` (the Run Agent entry)
 * always wins; otherwise the remembered prompt if it still exists, else the
 * canonical default.
 */
export function preferredPromptId(
  initialPromptId: string | undefined,
  prefs: AgentPrefs,
  prompts: PromptInfo[],
): string | undefined {
  if (initialPromptId && prompts.some((p) => p.id === initialPromptId)) return initialPromptId;
  if (prefs.promptId && prompts.some((p) => p.id === prefs.promptId)) return prefs.promptId;
  return defaultPromptId(prompts);
}
