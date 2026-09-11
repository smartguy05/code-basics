//! Tells the embedded browser panel when some DOM surface is on screen that the
//! OS webview would otherwise paint over (bug 3). The page is a WebView2 child
//! HWND that composites above the whole DOM, so a menu, a modal or Search
//! Everywhere opens *behind* it unless the page hides — and the only way to hide
//! it is through the host (`browser_set_visible`). This carries the "something is
//! covering things" signal to `BrowserPanel`, which feeds it to `pageVisible`.
//!
//! # Why a count, and why an `<Occluder/>` rather than props
//!
//! The occluding overlays are scattered — some are App-level modals, some live in
//! `WorkspaceTab`, some are context menus — so threading a boolean to the browser
//! from each is impractical. Instead each overlay mounts an invisible
//! {@link Occluder} beside itself (or calls {@link useOccluder}); the provider
//! keeps a **count** so two overlapping opens (a menu over a modal) do not
//! prematurely clear the signal, and the browser reads {@link useOcclusionCount}.
//!
//! Peer *floating panels* (a terminal, Notes, a review panel dragged over the
//! page) are deliberately **not** counted here — a panel merely being open must
//! not blank the browser. Those are handled geometrically in `BrowserPanel.sync`
//! (`occludedByPanels`), which only hides the page when a panel actually overlaps
//! its rect.

import { createContext, useContext, useEffect, useState } from "react";

interface OcclusionApi {
  /** How many occluding surfaces are currently open. */
  count: number;
  /** Register one open surface; the returned function releases it. */
  acquire: () => () => void;
}

const OcclusionContext = createContext<OcclusionApi>({
  count: 0,
  // The default no-ops so an overlay works outside a provider (a test, a story):
  // it simply contributes no occlusion.
  acquire: () => () => {},
});

export function OcclusionProvider({ children }: { children: React.ReactNode }) {
  const [count, setCount] = useState(0);
  const acquire = useState(() => () => {
    setCount((c) => c + 1);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      setCount((c) => c - 1);
    };
  })[0];
  return (
    <OcclusionContext.Provider value={{ count, acquire }}>{children}</OcclusionContext.Provider>
  );
}

/** How many occluding surfaces are open right now (0 = none). */
export function useOcclusionCount(): number {
  return useContext(OcclusionContext).count;
}

/**
 * Register this component as an occluding surface while `active` (default true).
 * Acquires on mount / when `active` turns true, releases on unmount / when it
 * turns false. Use it inside a component whose open-state is internal (Search
 * Everywhere); for an externally-gated overlay, mount an {@link Occluder} instead.
 */
export function useOccluder(active: boolean = true): void {
  const { acquire } = useContext(OcclusionContext);
  useEffect(() => {
    if (!active) return;
    const release = acquire();
    return release;
  }, [active, acquire]);
}

/**
 * An invisible marker: while it is mounted, the browser page counts one occluding
 * surface. Mount it beside a conditionally-rendered overlay
 * (`{open && <><Occluder/><Modal/></>}`) so the occlusion tracks the overlay's
 * lifetime with no change to the overlay component itself.
 */
export function Occluder(): null {
  useOccluder(true);
  return null;
}
