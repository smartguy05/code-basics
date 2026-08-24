/**
 * Pure decisions behind the Enhancements menu, kept out of the rendering shell
 * so they can be tested in the node environment (there is no DOM in vitest).
 */

import type { EnhancementInfo, PromptInfo, PromptRuns } from "../ipc/types";

/** What a click on a row does, given whether its section is already installed. */
export type RowAction = "add" | "remove";

export function actionFor(info: EnhancementInfo): RowAction {
  return info.installed ? "remove" : "add";
}

/** The small state badge shown after an installed row's title. */
export function statusBadge(info: EnhancementInfo): string | null {
  return info.installed ? "added" : null;
}

/**
 * Tooltip for a row's primary click target — it doubles as the accessible
 * description of what the click will do.
 */
export function actionTitle(info: EnhancementInfo): string {
  return info.installed
    ? `Remove "${info.title}" from CLAUDE.md and AGENTS.md`
    : `Add "${info.title}" to CLAUDE.md and AGENTS.md`;
}

/** Empty-state text when the templates directory holds nothing yet. */
export function emptyMessage(count: number): string | null {
  return count === 0
    ? "No instruction templates found. Drop a .md file into your instructions folder."
    : null;
}

/** Empty-state text for the Prompts submenu. */
export function emptyPromptsMessage(count: number): string | null {
  return count === 0
    ? "No prompts found. Drop a .md file into your prompts folder."
    : null;
}

/** The question shown before an instruction section is written to disk. */
export function confirmAddMessage(title: string): string {
  return `Add "${title}" to CLAUDE.md and AGENTS.md?`;
}

// --- Run Agent: run-once prompts -------------------------------------------

/** What clicking a prompt row should do, given the workspace's run record. */
export type PromptClickAction = "run" | "confirm-rerun";

/**
 * A run-once prompt that has already run for this repo asks before running
 * again; anything else runs straight away. A prompt not declared `once` never
 * confirms, no matter what the record says.
 */
export function promptClickAction(
  prompt: PromptInfo,
  runs: PromptRuns,
): PromptClickAction {
  return prompt.once && runs[prompt.id] ? "confirm-rerun" : "run";
}

/**
 * The "already run" badge for a run-once prompt with a record, or null. Only a
 * prompt declared `once` is ever badged — an ordinary prompt carries no history.
 */
export function runBadge(
  prompt: PromptInfo,
  runs: PromptRuns,
  now: number,
): string | null {
  if (!prompt.once) return null;
  const run = runs[prompt.id];
  if (!run) return null;
  return `ran ${relativeTime(run.lastRunAtMs, now)}`;
}

/** The confirmation shown before re-running an already-run run-once prompt. */
export function confirmRerunMessage(title: string): string {
  return `"${title}" already ran here — run again?`;
}

/**
 * A compact relative age ("just now", "5m ago", "3h ago", "2d ago", "4w ago").
 * Past only; a future stamp (clock skew) reads as "just now".
 */
export function relativeTime(atMs: number, now: number): string {
  const secs = Math.max(0, Math.floor((now - atMs) / 1000));
  if (secs < 60) return "just now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return `${Math.floor(days / 7)}w ago`;
}
