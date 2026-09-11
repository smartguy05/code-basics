//! Pure decisions for the Tasks panel — building the agent prompt, toggling a
//! task's status and owner, cleaning a title, and picking the selected row after
//! a delete — extracted so they are testable without a DOM (vitest runs in the
//! node environment). The React plumbing that drives them lives in
//! `TasksPanel.tsx` and decides nothing.
//!
//! Unlike `notesLogic`, the *store* is the backend's: every mutation goes through
//! a `commands/tasks.rs` command that returns the whole updated `TasksFile`, so
//! there are no optimistic list transforms here. What is left is the genuinely
//! pure UI logic — the same discipline the SQL and Notes panels keep.

import type { Task, TaskOwner, TaskStatus } from "../ipc/types";

/** The fallback title for a task whose title is blank. */
export const UNTITLED = "Untitled";

/**
 * The prompt handed to the agent when a task is assigned to it.
 *
 * Title and body are joined with a blank line so the agent reads a short heading
 * and then the detail. Either being empty is tolerated: a title-only task is a
 * one-line instruction, and a body-only task (its title cleaned to
 * {@link UNTITLED} elsewhere) still carries its detail. Both are trimmed so a
 * trailing newline in the textarea does not become the whole first line the
 * agent's TUI would submit early.
 */
export function taskPrompt(task: Pick<Task, "title" | "body">): string {
  const title = task.title.trim();
  const body = task.body.trim();
  if (!title) return body;
  if (!body) return title;
  return `${title}\n\n${body}`;
}

/** Toggle a task's status: an open task completes, a done task reopens. */
export function toggledStatus(status: TaskStatus): TaskStatus {
  return status === "done" ? "open" : "done";
}

/** The other owner — the target of a single toggle, or the label to offer. */
export function otherOwner(owner: TaskOwner): TaskOwner {
  return owner === "ai" ? "me" : "ai";
}

/** The short human label for an owner badge. */
export function ownerLabel(owner: TaskOwner): string {
  return owner === "ai" ? "AI" : "Me";
}

/**
 * Whether a new task can be created from the draft title: a non-blank title.
 * The body is optional — a one-line task is a legitimate reminder — but a task
 * with no title at all would render as an empty, unclickable row.
 */
export function canAddTask(title: string): boolean {
  return title.trim().length > 0;
}

/**
 * A title cleaned for persistence: trimmed, falling back to {@link UNTITLED} so
 * a rename that blanks the field never leaves a row rendering as empty. (Add is
 * gated by {@link canAddTask} instead, which refuses a blank outright.)
 */
export function cleanTaskTitle(title: string): string {
  return title.trim() || UNTITLED;
}

/**
 * The task to select after deleting `deletedId`, mirroring
 * `notesLogic.nextActiveAfterDelete`. When the deleted task was the selected
 * one, the neighbour that slid into its place leads — the task now at the
 * deleted index, or the new last task if it was the final row — so the
 * selection does not jump to the far end of the list. Deleting a non-selected
 * task leaves the selection where it is. An empty list has no selection.
 */
export function nextSelectedAfterDelete(
  tasks: Task[],
  deletedId: string,
  selectedId: string | undefined,
): string | undefined {
  const remaining = tasks.filter((t) => t.id !== deletedId);
  if (remaining.length === 0) return undefined;
  if (selectedId !== deletedId) return selectedId;
  const idx = tasks.findIndex((t) => t.id === deletedId);
  return remaining[Math.min(idx, remaining.length - 1)]?.id;
}
