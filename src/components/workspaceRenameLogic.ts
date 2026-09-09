//! Custom, user-chosen labels for the open-codebase tabs.
//!
//! The label cannot live on the `Workspace` object. `Workspace.name` comes from
//! the backend scan, and `App.onWorkspaceChange` / `addOpenWorkspace` replace
//! the stored object **in place** on every re-open and rescan — so a rename
//! held there would be silently discarded the next time the user pressed
//! Rescan. It is instead a `localStorage` map keyed on `root`, which is the
//! identity of an open workspace everywhere else in the app (the tab key, the
//! setup-dismissal key, the recents entries).
//!
//! Everything here is pure except {@link loadLabels} / {@link saveLabels},
//! which take a `Storage` rather than reaching for the global one — that keeps
//! the whole module testable under vitest's node environment, where there is no
//! DOM.

/**
 * The `localStorage` key the map of renamed codebases lives under.
 *
 * One entry for the whole map rather than one per root: the strip reads every
 * label on each render, and a single value is one read instead of N.
 */
export const WORKSPACE_LABELS_KEY = "cb.workspaceTabs.labels";

/**
 * The longest custom label accepted, in characters.
 *
 * `.ws-tab-label` is `max-width: 220px` with 8px of horizontal padding, so a
 * little over 200px of text is visible and anything past it is ellipsised —
 * roughly 30 characters at the UI font size. The cap is deliberately set above
 * that rather than at it: truncating what the user typed to exactly what fits
 * would be this module guessing at font metrics it cannot measure from here,
 * and the ellipsis already handles overflow honestly. What the cap is actually
 * for is the accident — a paste of a whole path or a paragraph into the rename
 * box — which 40 characters stops without ever getting in the way of a name a
 * person would choose.
 */
export const MAX_LABEL_LENGTH = 40;

/** Renamed codebases, keyed by workspace `root`. Roots with no entry are unnamed. */
export type WorkspaceLabels = Record<string, string>;

/**
 * The name the user gave this root, or `undefined` if they never named it.
 *
 * Guards the value's type as well as its presence, because a `Record` lookup
 * also finds inherited `Object.prototype` members — no real root path is
 * spelled `toString`, but a lookup that can return a function is not one to
 * leave open in a labelling path.
 *
 * Exported because `workspaceTabsLogic.tabLabels` needs the same lookup, and
 * two copies of "has this tab been renamed?" is two places for the answer to
 * drift. This is the only question this module answers about display; what the
 * strip actually *shows* — including disambiguating same-named roots — is
 * `tabLabels`, and it is deliberately not reimplemented here.
 */
export function customLabel(labels: WorkspaceLabels, root: string): string | undefined {
  const value = labels[root];
  return typeof value === "string" ? value : undefined;
}

/**
 * Clean up a label the user typed, or reject it.
 *
 * Returns `null` for anything that is not a usable name — empty, whitespace
 * only, or nothing but control characters — rather than storing it, because an
 * empty tab label is unclickable and indistinguishable from a bug. The caller
 * treats `null` as "no rename
 * happened" and keeps the derived name, which is the abstain rule: the honest
 * scanned name beats a name we made up out of a blank field.
 */
export function normalizeLabel(raw: string): string | null {
  // Whitespace runs — including the newlines a pasted paragraph carries — become
  // single spaces. The length cap alone does not handle these: it bounds how
  // much of the paste is kept without removing the line breaks inside it, and
  // HTML would silently collapse them anyway, so the stored value would not be
  // what the tab shows.
  //
  // Control and bidi-override characters are dropped outright. A tab label is
  // rendered next to other tabs' labels, and U+202E in the middle of one
  // reverses the text after it, so a name typed as `api<RLO>gnp.exe` draws as
  // `apiexe.png`. This is a local desktop app, so the stake is confusion rather
  // than attack, but a label that does not read as what was typed is a wrong
  // answer either way.
  //
  // Spelled as escapes, never as the literal characters: every one of these is
  // invisible in an editor, so a later reader could not otherwise see what the
  // class contains — or notice one being removed.
  const cleaned = raw
    .replace(
      /[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g,
      " ",
    )
    .replace(/\s+/g, " ")
    .trim();
  if (cleaned === "") return null;

  // Sliced by code POINT, not by code unit. `String.prototype.slice` cuts UTF-16
  // units, so a name whose 40th unit is half a surrogate pair is stored as a
  // lone surrogate — it renders as U+FFFD, and because `JSON.stringify` escapes
  // it rather than rejecting it, the broken label survives a round trip and
  // comes back corrupted on every launch.
  const points = Array.from(cleaned);
  return points.length <= MAX_LABEL_LENGTH ? cleaned : points.slice(0, MAX_LABEL_LENGTH).join("");
}

/**
 * Serialise the map for `localStorage`.
 *
 * Keys are sorted so an unchanged map produces an unchanged string, which keeps
 * repeated writes idempotent and the stored value diffable by eye — the same
 * reason `encodeCollapsedFolders` sorts.
 */
export function encodeLabels(labels: WorkspaceLabels): string {
  const sorted: WorkspaceLabels = {};
  for (const key of Object.keys(labels).sort()) {
    const value = customLabel(labels, key);
    if (value !== undefined) sorted[key] = value;
  }
  return JSON.stringify(sorted);
}

/**
 * Read back what {@link encodeLabels} wrote.
 *
 * A missing, malformed or wrongly-shaped value yields an **empty** map rather
 * than an error — the same tolerance `cb_core::notes::load` and
 * `decodeCollapsedFolders` apply, and for the same reason: a corrupt
 * preference must never stop the tab strip drawing. Falling back to no custom
 * labels shows the scanned names, which are always correct if not always what
 * the user picked.
 *
 * Individual entries are re-validated through {@link normalizeLabel} on the way
 * in, so a value that was hand-edited into the store (or written by an older,
 * looser build) cannot put a blank or unbounded string on a tab.
 */
export function decodeLabels(raw: string | null): WorkspaceLabels {
  if (raw === null) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    const pairs: [string, string][] = [];
    for (const [root, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (root === "" || typeof value !== "string") continue;
      const label = normalizeLabel(value);
      if (label !== null) pairs.push([root, label]);
    }
    // `fromEntries` defines own properties, so a stored `__proto__` key becomes
    // ordinary data rather than reassigning the prototype.
    return Object.fromEntries(pairs);
  } catch {
    return {};
  }
}

/** Load the renamed-codebase map, tolerating a storage that throws (private mode, quota). */
export function loadLabels(storage: Storage): WorkspaceLabels {
  try {
    return decodeLabels(storage.getItem(WORKSPACE_LABELS_KEY));
  } catch {
    return {};
  }
}

/**
 * Persist the renamed-codebase map.
 *
 * Failures are swallowed: a rename that could not be written back still shows
 * for this session, and there is nothing useful to say to the user about a full
 * or disabled `localStorage` at the moment they renamed a tab.
 */
export function saveLabels(storage: Storage, labels: WorkspaceLabels): void {
  try {
    storage.setItem(WORKSPACE_LABELS_KEY, encodeLabels(labels));
  } catch {
    /* a label is a convenience; losing the persistence of one is not an error */
  }
}

/**
 * The **editable** name for one root: the user's name if they set one, else the
 * scanned one.
 *
 * This is what seeds the rename box, and it is deliberately *not* what the tab
 * strip renders — `tabLabels` may prefix a path segment onto a colliding name,
 * and seeding the editor with `one/api` would invite the user to save a
 * disambiguation the app generated rather than a name they chose.
 */
export function labelFor(labels: WorkspaceLabels, root: string, derived: string): string {
  return customLabel(labels, root) ?? derived;
}

/**
 * Record a rename, or refuse it.
 *
 * Returns a **new** map (never a mutation — the caller holds this in React
 * state), or `null` when the typed name is not usable, which the caller reads
 * as "leave the tab as it was". A rename to exactly the derived name is still
 * stored deliberately: it pins the tab to that word, so a later rescan that
 * renames the folder — or a second codebase arriving and colliding — leaves the
 * label the user chose alone. {@link clearLabel} is how you get back to
 * following the scan.
 */
export function setLabel(labels: WorkspaceLabels, root: string, raw: string): WorkspaceLabels | null {
  const label = normalizeLabel(raw);
  if (label === null) return null;
  return { ...labels, [root]: label };
}

/**
 * Drop a rename, returning the tab to its scanned name.
 *
 * Entries for closed codebases are deliberately **not** pruned here: closing a
 * tab is not a decision about its name, and re-opening a codebase you had
 * renamed should find it as you left it. The map is a handful of short strings
 * keyed by path, so nothing is at stake in keeping them.
 */
export function clearLabel(labels: WorkspaceLabels, root: string): WorkspaceLabels {
  if (customLabel(labels, root) === undefined) return labels;
  const next = { ...labels };
  delete next[root];
  return next;
}
