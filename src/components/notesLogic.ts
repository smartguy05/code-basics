//! Pure decisions for the Notes / scratchpad panel — creating, renaming,
//! deleting and updating notes, picking the active tab, and the persistence
//! keys — extracted so they are testable without a DOM (vitest runs in the node
//! environment). The React plumbing that drives them lives in `NotesPanel.tsx`
//! and decides nothing.
//!
//! The clock and the id sequence are passed in (never read here), so every
//! result is reproducible in a test — the same discipline as `makeTerminal` and
//! `enhancements/runs.rs`.

import type { Note } from "../ipc/types";

/** The localStorage key the panel persists its shared layout under. */
export const NOTES_LAYOUT_KEY = "cb.notes.layout";

/** The localStorage key remembering which note tab was last active. */
export const NOTES_ACTIVE_KEY = "code-basics.notes.activeId";

/**
 * The localStorage key remembering the minimized Notes bar's colour. There is
 * one Notes bar (not one per note), so this is a single value — unlike a
 * terminal's colour, which lives on its descriptor. Persisted because the Notes
 * panel, unlike a terminal, is long-lived and expected to reopen as the user
 * left it.
 */
export const NOTES_COLOR_KEY = "cb.notes.pillColor";

/** The fallback title for a note whose title is blank. */
export const UNTITLED = "Untitled";

/**
 * Build a fresh note from a monotonic sequence number and the current time.
 * Monotonic (not "lowest unused") so an id is never reused within a session —
 * the same reasoning as `makeTerminal`.
 */
export function makeNote(seq: number, now: number): Note {
  return {
    id: `note-${seq}`,
    title: `Note ${seq}`,
    body: "",
    color: undefined,
    createdAtMs: now,
    updatedAtMs: now,
  };
}

/** Append a fresh note; returns the new list and the new note's id. */
export function addNote(
  notes: Note[],
  seq: number,
  now: number,
): { notes: Note[]; activeId: string } {
  const note = makeNote(seq, now);
  return { notes: [...notes, note], activeId: note.id };
}

/**
 * Rename a note. A blank title falls back to {@link UNTITLED} so a tab never
 * renders as empty and unclickable. `updatedAtMs` is bumped.
 */
export function renameNote(notes: Note[], id: string, title: string, now: number): Note[] {
  const clean = title.trim() || UNTITLED;
  return notes.map((n) => (n.id === id ? { ...n, title: clean, updatedAtMs: now } : n));
}

/** Replace a note's body, bumping `updatedAtMs`. */
export function updateBody(notes: Note[], id: string, body: string, now: number): Note[] {
  return notes.map((n) => (n.id === id ? { ...n, body, updatedAtMs: now } : n));
}

/** Remove a note by id. */
export function deleteNote(notes: Note[], id: string): Note[] {
  return notes.filter((n) => n.id !== id);
}

/**
 * The tab to show, given the notes and the id the panel last remembered.
 *
 * The stored id wins when it still names a note; otherwise the first note leads
 * (a deleted-active note, or a first open with no memory). An empty list has no
 * active tab, which the panel renders as its empty state.
 */
export function resolveActiveId(notes: Note[], storedId: string | null | undefined): string | undefined {
  if (storedId && notes.some((n) => n.id === storedId)) return storedId;
  return notes[0]?.id;
}

/**
 * The tab to make active after deleting `deletedId`. When the deleted note was
 * the active one, the neighbour that slid into its place leads — the note now at
 * the deleted index, or the new last note if it was the final tab — so focus
 * does not jump to the far end of the list. Deleting a non-active note leaves
 * the active one where it is.
 */
export function nextActiveAfterDelete(
  notes: Note[],
  deletedId: string,
  activeId: string | undefined,
): string | undefined {
  const remaining = deleteNote(notes, deletedId);
  if (remaining.length === 0) return undefined;
  if (activeId !== deletedId) return activeId;
  const idx = notes.findIndex((n) => n.id === deletedId);
  return remaining[Math.min(idx, remaining.length - 1)]?.id;
}

/** The agent-panel header label when a note is sent to the agent. */
export function sendToAgentTitle(note: Note): string {
  return `Note: ${note.title}`;
}

/**
 * How long to wait before flushing a pending notes write, in ms.
 *
 * Normally the trailing `debounceMs` after the last keystroke — but a
 * trailing-only debounce, reset on every keystroke, would defer the write
 * indefinitely while the user types continuously, so a crash could lose an
 * unbounded amount of text. This caps the wait: once a pending write has been
 * outstanding `maxWaitMs`, it flushes now (delay `0`), and near that cap the
 * debounce is clamped so the write never lands later than the budget allows.
 *
 * `pendingSinceMs` is when the oldest unsaved change was made; `nowMs` is the
 * current time (both injected, never read here, so the result is testable).
 */
export function flushDelay(
  pendingSinceMs: number,
  nowMs: number,
  debounceMs: number,
  maxWaitMs: number,
): number {
  const remaining = maxWaitMs - (nowMs - pendingSinceMs);
  if (remaining <= 0) return 0;
  return Math.min(debounceMs, remaining);
}

/** Read the remembered active-note id. Never throws (storage may be absent). */
export function loadActiveId(storage: Pick<Storage, "getItem">): string | undefined {
  try {
    return storage.getItem(NOTES_ACTIVE_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}

/** Remember the active-note id. Never throws. */
export function saveActiveId(storage: Pick<Storage, "setItem">, id: string): void {
  try {
    storage.setItem(NOTES_ACTIVE_KEY, id);
  } catch {
    // Persistence is a convenience, not a requirement.
  }
}

/**
 * Read the remembered Notes-bar colour, or `undefined` for the theme default.
 * Never throws (storage may be absent). An empty stored value reads as "no
 * colour" so clearing back to default round-trips.
 */
export function loadPillColor(storage: Pick<Storage, "getItem">): string | undefined {
  try {
    return storage.getItem(NOTES_COLOR_KEY) || undefined;
  } catch {
    return undefined;
  }
}

/**
 * Remember the Notes-bar colour, or clear it when `undefined` (back to the
 * theme default). Never throws.
 */
export function savePillColor(storage: Pick<Storage, "setItem" | "removeItem">, color: string | undefined): void {
  try {
    if (color) storage.setItem(NOTES_COLOR_KEY, color);
    else storage.removeItem(NOTES_COLOR_KEY);
  } catch {
    // Persistence is a convenience, not a requirement.
  }
}
