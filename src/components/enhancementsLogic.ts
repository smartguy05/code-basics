/**
 * Pure decisions behind the Enhancements menu, kept out of the rendering shell
 * so they can be tested in the node environment (there is no DOM in vitest).
 */

import type { EnhancementInfo } from "../ipc/types";

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

/** Transient confirmation shown after a prompt is copied to the clipboard. */
export function copyFeedback(title: string): string {
  return `Copied "${title}"`;
}

/** The question shown before an instruction section is written to disk. */
export function confirmAddMessage(title: string): string {
  return `Add "${title}" to CLAUDE.md and AGENTS.md?`;
}
