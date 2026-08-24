import type { LineIntent } from "../ipc/types";

export function formatTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

/**
 * The recorded intent for a given line of a committed file, or `null`.
 *
 * A wrong "why" is worse than none: this returns a reason only for a line the
 * durable note actually resolved, and `null` for anything else — including a
 * `null` line (no caret). The History tab renders the empty state rather than a
 * guess.
 */
export function intentForLine(
  intents: LineIntent[],
  line: number | null,
): LineIntent | null {
  if (line == null) return null;
  return intents.find((intent) => intent.line === line) ?? null;
}

/**
 * The tooltip text for the line under the cursor, or `null` when that line has
 * no recorded reason (so no tooltip is shown — a wrong "why" is worse than none).
 *
 * Shows the label, whether it was stated by the agent or inferred from its
 * notes, and the user's original prompt when one was captured. Returned as a
 * plain multi-line string; the CodeMirror layer renders it verbatim.
 */
export function whyTooltip(intent: LineIntent | null): string | null {
  if (!intent) return null;

  const lines: string[] = [intent.label ?? "(no stated reason)"];

  if (intent.labelSource === "declared") {
    lines.push("— stated by the agent");
  } else if (intent.labelSource === "inferred") {
    lines.push("— inferred from the agent's notes");
  }

  if (intent.prompt) {
    lines.push(`Prompt: ${intent.prompt}`);
  }

  return lines.join("\n");
}

/** A short caption for the "Why" panel, or `null` when nothing resolved. */
export function whyCaption(intents: LineIntent[]): string | null {
  if (intents.length === 0) return null;
  const n = intents.length;
  return n === 1
    ? "1 line carries a recorded reason"
    : `${n} lines carry a recorded reason`;
}

/**
 * Delete branches one at a time, collecting the failures.
 *
 * Deliberately **sequential**: deleting a branch rewrites the repository's
 * shared `packed-refs`, so running deletes concurrently makes them race each
 * other — one grabs `packed-refs.lock` while another is blocked on it, or reads
 * `packed-refs` while a sibling is mid-rewrite and sees a truncated file
 * (`expected N bytes, read N-k`). Awaiting each in turn is slower but is the
 * only correct way to touch refs. Best-effort: a branch git refuses (e.g. one
 * checked out in a linked worktree) is recorded and does not stop the rest.
 *
 * @returns the branches that could not be deleted, in attempt order
 */
export async function bulkDeleteBranches(
  names: string[],
  deleteBranch: (name: string) => Promise<unknown>,
  describeError: (error: unknown) => string,
): Promise<{ name: string; error: string }[]> {
  const failed: { name: string; error: string }[] = [];
  for (const name of names) {
    try {
      await deleteBranch(name);
    } catch (error) {
      failed.push({ name, error: describeError(error) });
    }
  }
  return failed;
}

/**
 * Human summary of a bulk branch deletion. Deletion is best-effort — each
 * branch is attempted independently (an unmerged branch fails on its own
 * without aborting the rest) — so the message has to distinguish complete
 * success (nothing to say, `null`), partial success, and total failure.
 *
 * @param failed the branches that could not be deleted, with their reasons
 * @param total  how many were attempted
 */
export function bulkDeleteMessage(
  failed: { name: string; error: string }[],
  total: number,
): string | null {
  if (failed.length === 0) return null;
  const deleted = total - failed.length;
  const details = failed.map((f) => `${f.name}: ${f.error}`).join("\n");
  const prefix =
    deleted > 0
      ? `Deleted ${deleted} of ${total} branch${total === 1 ? "" : "es"}. ` +
        `${failed.length} could not be deleted:`
      : `Could not delete ${failed.length === 1 ? "the branch" : `any of the ${failed.length} branches`}:`;
  return `${prefix}\n${details}`;
}
