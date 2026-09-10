//! The React plumbing for the app-wide floating-panel focus order. All
//! *decisions* live in `focusOrderLogic.ts` (pure, node-tested); this file only
//! holds the order state and hands panels the raise/release callbacks and their
//! current offset.
//!
//! # Why one App-level context
//!
//! The order must span panels that live in different parts of the tree — the
//! browser and terminals inside each `WorkspaceTab`, Notes at the App level — so
//! "last clicked is on top" is a single fact rather than three private ones. A
//! context lets each panel register and read the order directly, without threading
//! an order + setter through every host.
//!
//! Unlike the dock's stable setter, `order` itself is in the value: a panel's
//! z-index (and the browser page's occlusion) is derived from it, so the panels
//! that read their offset *must* re-render when the order changes. That is exactly
//! the set of floating panels — the same ones the old per-workspace terminal
//! `stackOrder` already re-rendered on every raise.

import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { focusOffset, raiseFocus } from "./focusOrderLogic";

interface FocusOrderApi {
  /** Panel ids, bottom-most first. */
  order: string[];
  /** Bring a panel to the front. Idempotent when already top (no re-render). */
  raise: (id: string) => void;
  /** Remove a panel from the order (on unmount), freeing its raise budget. */
  release: (id: string) => void;
}

/**
 * The default is inert — empty order, no-op raise/release — so a panel rendered
 * outside a {@link FocusOrderProvider} still renders (at the band base) rather
 * than throwing. Same convention as `DockContext`.
 */
const FocusOrderContext = createContext<FocusOrderApi>({
  order: [],
  raise: () => {},
  release: () => {},
});

export function FocusOrderProvider({ children }: { children: React.ReactNode }) {
  const [order, setOrder] = useState<string[]>([]);
  const raise = useCallback((id: string) => {
    setOrder((prev) => raiseFocus(prev, id));
  }, []);
  const release = useCallback((id: string) => {
    setOrder((prev) => (prev.includes(id) ? prev.filter((k) => k !== id) : prev));
  }, []);
  return (
    <FocusOrderContext.Provider value={{ order, raise, release }}>
      {children}
    </FocusOrderContext.Provider>
  );
}

/** The full focus API: the order plus raise/release. */
export function useFocusOrder(): FocusOrderApi {
  return useContext(FocusOrderContext);
}

/** The raise step a panel with `id` renders at (its `--cb-stack`). */
export function useFocusOffset(id: string): number {
  return focusOffset(useFocusOrder().order, id);
}

/**
 * Register a floating panel's presence in the focus order and return its
 * `raise` callback (bind it to `onPointerDownCapture`).
 *
 * Raises the panel on mount so a freshly opened/restored panel starts on top, and
 * releases it on unmount so a closed panel stops consuming raise budget. Re-runs
 * when `id` changes (a browser id is per-workspace), releasing the old id.
 */
export function useFocusEntry(id: string): () => void {
  const { raise, release } = useFocusOrder();
  useEffect(() => {
    raise(id);
    return () => release(id);
  }, [id, raise, release]);
  return useCallback(() => raise(id), [raise, id]);
}
