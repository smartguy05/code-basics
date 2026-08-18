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
