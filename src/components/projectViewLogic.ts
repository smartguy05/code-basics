//! Pure decisions for the merged **Project** tab — the view that puts the old
//! Run tab's file tree and the old Changes tab's working-tree list behind one
//! icon rail, sharing a single editor area.
//!
//! Extracted so the component stays a rendering shell: vitest runs in the node
//! environment, so anything that touches React, the DOM or `localStorage`
//! cannot be tested at all. What is worth checking here is not markup but three
//! rules that are each easy to get subtly wrong:
//!
//! - which toolbar the user is looking at (it follows the **active editor
//!   tab**, not the rail — see {@link toolbarFor}),
//! - when an action means the side panel should switch pane (mostly: never —
//!   see {@link paneForAction}),
//! - and when the changes panel has to re-read git, which used to happen for
//!   free and no longer does (see {@link shouldRefreshChanges}).
//!
//! This module deliberately knows nothing about *how* a diff tab is minted. It
//! reads one field — the source `kind` — so it composes with whatever
//! `editorSourceLogic` grows, and so a test here needs no editor.

/** Which surface the side panel is showing. */
export type ProjectPane = "files" | "changes";

/** Which toolbar sits above the editor area. */
export type ProjectToolbar = "run" | "changes";

/**
 * The `EditorSource.kind` that means "this tab is showing a diff".
 *
 * Named rather than inlined because it is the single string this module and the
 * tab-minting code have to agree on, and a silent disagreement here shows the
 * wrong toolbar rather than failing.
 */
export const DIFF_SOURCE_KIND = "diff";

/**
 * The only thing this module needs to know about an open editor tab.
 *
 * Structural on purpose: the real tab type (`OpenEditorFile` in
 * `editorSourceLogic.ts`) carries an id, a label and a per-variant payload,
 * none of which decide a toolbar. Taking the narrow shape keeps this testable
 * with a literal and keeps it compiling as new source variants are added.
 */
export interface ToolbarTab {
  source: { kind: string };
}

/**
 * Which toolbar the Project tab shows, given the tab the editor area is
 * currently displaying.
 *
 * **The toolbar follows the active editor tab, not the rail.** Tying it to the
 * rail would put Stage / Unstage / Revert buttons above a file the user is
 * reading source in — controls that act on something other than what is on
 * screen, which is the one failure mode worth designing around here.
 *
 * The mapping is *diff is the exception*, not *file is the rule*: a diff tab
 * gets the Changes toolbar and **everything else** gets the Run toolbar. That
 * asymmetry is deliberate. The Run controls (run, debug, build, stop) are
 * workspace-level and stay meaningful no matter what is on screen, so showing
 * them for a tab this module does not recognise is harmless; showing the
 * Changes controls for one would arm file-mutating actions against a file that
 * is not the one being displayed. So a future source variant — a settings tab,
 * an image preview — degrades toward the safe answer without an edit here.
 *
 * With **no tab open** the answer is the Run toolbar, for the same reason: the
 * Run controls are workspace-level and are exactly what an empty editor area
 * wants to offer, while a Changes toolbar with no diff to act on is a row of
 * disabled buttons.
 */
export function toolbarFor(active: ToolbarTab | null | undefined): ProjectToolbar {
  return active?.source.kind === DIFF_SOURCE_KIND ? "changes" : "run";
}

/**
 * Something the user (or the app, on their behalf) just did that might mean the
 * side panel should be showing a different pane.
 *
 * This union is the *complete* list of things allowed to move the rail. Adding
 * a case is how a new caller earns that right; anything not listed leaves the
 * pane alone by construction.
 *
 * - `railSelect` — the user clicked a rail icon. The only unconditional move.
 * - `revealInTree` — "select opened file" / a reveal request: the point of the
 *   action is a row scrolled to *in the tree*, so the tree has to be visible.
 * - `highlightChange` — an erosion flag, an intent hunk or a changed-file row
 *   being pointed at: again, the result is rendered inside the changes panel.
 * - `openEditorTab` — a file or a diff opened into the editor area. Implies
 *   **nothing**, which is the entry most worth stating (see below).
 */
export type ProjectAction =
  | { kind: "railSelect"; pane: ProjectPane }
  | { kind: "revealInTree" }
  | { kind: "highlightChange" }
  | { kind: "openEditorTab" };

/**
 * The pane an action requires, or `null` when it requires nothing.
 *
 * The rule is narrow on purpose: **an action moves the rail only when its own
 * result is rendered inside the panel.** Revealing a file means a row in the
 * tree; highlighting a changed hunk means a row in the changes list. Neither is
 * visible with the other pane showing, so those two move it.
 *
 * Opening an editor tab does not, and that is the case implementations get
 * wrong. The result of opening a tab is in the *main area*, which both panes
 * share, so switching the panel would spend the user's context for nothing —
 * and it actively hurts in the two flows this tab exists to support: jumping to
 * a definition while working through a list of changed files would yank that
 * list away, and opening a diff while browsing the tree would close the tree
 * the next file was going to come from. The rail is the user's own
 * navigation state; the app leaves it where they put it. That is the same
 * decoupling {@link toolbarFor} relies on in the other direction: the toolbar
 * follows the tab, the panel does not.
 *
 * Returning `null` rather than the current pane keeps "no opinion" and
 * "deliberately this pane" distinguishable at the call site, so a caller can
 * skip the state update entirely instead of writing back a value it invented.
 */
export function paneForAction(action: ProjectAction): ProjectPane | null {
  switch (action.kind) {
    case "railSelect":
      return action.pane;
    case "revealInTree":
      return "files";
    case "highlightChange":
      return "changes";
    case "openEditorTab":
      return null;
  }
}

/**
 * Everything that decides whether the changes panel is actually on screen.
 *
 * Three terms, not one, because the merge destroyed **two** mount gates and it
 * is easy to restore only the visible one. The old Changes tab was rendered as
 * `active && tab === "changes"` (`WorkspaceTab.tsx`): `active` is *this
 * codebase is the foreground one*, `tab` is *the Changes tab is the foreground
 * inner tab*. Inside an always-mounted Project tab neither survives, and the
 * rail adds a third. A predicate that knew only about the rail would leave
 * every open codebase scanning git forever behind the Tests tab.
 */
export interface ChangesVisibility {
  /** Which surface the rail has selected. */
  pane: ProjectPane;
  /** The Project tab is the foreground inner tab of its codebase. */
  tabForeground: boolean;
  /** This codebase is the foreground one in the app. */
  codebaseActive: boolean;
}

/** Whether the user can actually see the changes panel right now. */
export function changesOnScreen(visibility: ChangesVisibility): boolean {
  return (
    visibility.pane === "changes" &&
    visibility.tabForeground &&
    visibility.codebaseActive
  );
}

/**
 * Whether the changes panel must re-read git, given how visibility just
 * changed.
 *
 * **This exists to replace behaviour that used to be free.** The old Changes
 * tab was mounted *conditionally*, so leaving and returning unmounted and
 * remounted it, and its mount effect re-read the working tree from disk. That
 * accident is relied on: it is how the list is correct again after a
 * regeneration, an external commit, or an agent editing files in a terminal.
 * Inside an always-mounted Project tab the unmount never happens, so nothing
 * re-reads unless something asks — and the symptom of forgetting is the worst
 * kind, a stale list that looks authoritative.
 *
 * It is **edge-triggered on becoming visible**, which is what makes it cover
 * every transition the old unmount covered rather than only the rail:
 *
 * - rail switched to Changes,
 * - the user left the Project tab and came back,
 * - the user switched to another codebase and back.
 *
 * Keying on the rail alone restores the first and silently drops the other
 * two — and those are exactly the ones an agent working in a terminal changes
 * the tree behind.
 *
 * Two consequences worth stating, both deliberate:
 *
 * - **The first mount** (`previous` is `null`) refreshes if the panel is on
 *   screen. Whether the panel also reads on its own mount is not knowable from
 *   here, and the asymmetry decides it: a redundant read costs one git status
 *   that de-churns to no re-render, while a missed read shows a stale list.
 * - **Re-entering the same pane while it never left the screen** is `false`.
 *   Clicking the icon that is already selected expressed no intent to refresh.
 */
export function shouldRefreshChanges(
  previous: ChangesVisibility | null,
  next: ChangesVisibility,
): boolean {
  if (!changesOnScreen(next)) return false;
  return previous === null || !changesOnScreen(previous);
}

/**
 * Whether the changes panel's background poll should be running.
 *
 * The poll re-reads git status and re-runs the intent, erosion and coverage
 * scans on a timer. It was scoped by mounting: the interval lived inside a
 * component that only existed while its tab was foreground *and* its codebase
 * was active. Always-mounted, it would keep scanning forever behind the file
 * tree — repeated work nobody can see, on every open codebase at once.
 *
 * `paused` carries the reasons the panel already knows about and this module
 * has no business duplicating: an action in flight, a hidden window, a sub-view
 * that owns its own refresh. Everything the *merge* introduced is in
 * {@link changesOnScreen}.
 */
export function shouldPollChanges(
  visibility: ChangesVisibility,
  paused: boolean,
): boolean {
  return changesOnScreen(visibility) && !paused;
}
