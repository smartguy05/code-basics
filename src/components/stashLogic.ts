import type { StashEntry } from "../ipc/types";

export { formatTime } from "../views/historyLogic";

/**
 * The one-line label a stash row shows.
 *
 * Git prefixes its own messages with `On <branch>:` / `WIP on <branch>:`; the
 * backend has already parsed the branch out into `entry.branch`, so here we
 * show the human part of the message (the text after the first `: ` when the
 * branch was recognised) and let the row render the branch separately. When
 * there is no recognisable prefix, or stripping it would leave nothing, we show
 * the message verbatim rather than guessing where it starts.
 */
export function stashSummary(entry: StashEntry): string {
  const marker = entry.message.indexOf(": ");
  if (entry.branch != null && marker !== -1) {
    return entry.message.slice(marker + 2).trim() || entry.message;
  }
  return entry.message;
}
