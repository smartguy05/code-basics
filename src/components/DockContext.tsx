//! The React plumbing for the shared minimized-window dock. All *decisions* live
//! in `dockLogic.ts` (pure, node-tested); this file only carries the registry
//! setter to the panels and keeps their entries alive while they are minimized.
//!
//! # Why a context here, in a codebase that otherwise threads props
//!
//! Every floating panel that can minimize would otherwise have to hand a
//! `setDockEntry` callback down through `WorkspaceTab` (five panels) and from
//! `App` (Notes). A context lets each panel register directly. The context value
//! is a **stable** setter (never the entries array), so a panel registering does
//! not re-render every other panel — only `App` (which holds the array) and
//! `Dock` (which renders it) update.

import { createContext, useContext, useEffect } from "react";
import type { DockEntry } from "./dockLogic";

/** A minimized panel's live dock entry: the data {@link DockEntry} plus the
 * callback that restores the panel when its pill is clicked. */
export interface LiveDockEntry extends DockEntry {
  onRestore: () => void;
}

/** Register/update an entry (`entry`), or remove it (`null`), by id. */
export type SetDockEntry = (id: string, entry: LiveDockEntry | null) => void;

/**
 * The default is a no-op: a panel rendered outside a {@link DockProvider} simply
 * registers nothing rather than throwing. That keeps the panels usable in
 * isolation (a test harness, a story) with no dock mounted.
 */
const DockContext = createContext<SetDockEntry>(() => {});

export function DockProvider({
  setDockEntry,
  children,
}: {
  setDockEntry: SetDockEntry;
  children: React.ReactNode;
}) {
  return <DockContext.Provider value={setDockEntry}>{children}</DockContext.Provider>;
}

/**
 * Register a panel's minimized presence in the dock.
 *
 * Pass the live entry while minimized, or `null` when not — passing `null`
 * removes it. The effect keys on every *field* it reads, so an attention flash or
 * a colour change re-registers (in place, via `upsertEntry`); unmount cleans up.
 *
 * `onRestore` is **not** in the dependency list, so a panel must pass a **stable**
 * closure for it (a `useCallback`, or one that only touches refs/stable setters).
 * Depending on it instead would re-run the effect every render, since panels
 * rebuild the closure each time; keying on the data fields is what keeps the
 * registry quiet. A field change re-registers and picks up the current closure.
 */
export function useDockEntry(entry: LiveDockEntry | null): void {
  const setDockEntry = useContext(DockContext);
  const id = entry?.id ?? null;
  useEffect(() => {
    if (!entry) return;
    setDockEntry(entry.id, entry);
    return () => setDockEntry(entry.id, null);
  }, [
    setDockEntry,
    id,
    entry?.scope,
    entry?.label,
    entry?.order,
    entry?.pinned,
    entry?.color,
    entry?.status,
    entry?.attention,
    entry?.spinner,
  ]);
}
