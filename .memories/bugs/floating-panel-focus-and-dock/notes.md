# Notes — floating-panel focus & terminal dock remount

## Two root causes (both about floating panels)

### 1. Changing `createPortal`'s container REMOUNTS the children (React fact)
`DockableTerminal`/`DockableEditorSlot` used to `createPortal(panel, target)` and
swap `target` between the float layer and a region slot on dock/undock. React
treats the container as part of the portal fiber's identity: when it changes,
React **deletes the old portal fiber (unmounting children, running cleanups) and
creates a new one**. For a terminal that fired the mount-once open effect's
cleanup (`terminalClose`) + a fresh `terminalOpen` → the reported "docking closes
the terminal and opens a new one." Editors silently lost CodeMirror/LSP state.

**Fix:** `components/StablePortal.tsx` — one host `<div>` created once (stable
`createPortal` container), moved between float/slot with `appendChild`. An
imperative DOM move does NOT touch React's tree, so children stay mounted. Any
future dockable must portal through StablePortal, never a bare `createPortal` with
a moving container. The old "no remount" comments in `DockableTerminal` /
`RegionContext` were factually wrong and are now corrected.

### 2. The browser page is hidden by GEOMETRIC occlusion, with no raise order
The WebView2 page composites above all DOM; it is shown/hidden only via
`pageVisible`. Terminals "covered" the browser because `occludedByPanels` blanked
the page whenever *any* peer rect overlapped it — there was no cross-panel raise
order, so clicking the browser could never win.

**Fix:** one app-wide focus order (`focusOrderContext` + pure `focusOrderLogic`,
generalised from the old terminal-only stack helpers, which now re-export from it).
Terminals, browser and Notes all derive `--cb-stack` and z-index from it, and
occlusion became raise-aware: `occludedByAbovePanels` blanks the page only for a
peer stacked STRICTLY ABOVE the browser. Notes lost its always-above-terminals
fixed band (`--z-notes` now unused) — deliberate, confirmed with user (unified
last-clicked-on-top).

## Gotchas that bit / to watch
- The browser reads each peer's focus offset from the DOM (`node.style
  --cb-stack`), so **every floating panel must write `--cb-stack`** for occlusion
  to be correct. The dock (`.dock`) carries none → reads 0, harmless (never
  overlaps the page, and must be *above* to occlude).
- A raise that leaves the browser's own offset unchanged (it was at the band floor
  and a terminal appeared above it) does not move `sync`'s deps → added an effect
  re-running `sync` on any `focusOrder` change. `useEffect` runs post-commit, so
  peer `--cb-stack` is current when read.
- Terminal focus ids are namespaced `dockId(root, term-${number})` — keys restart
  at `term-1` per workspace and would collide in the single global order.
- Notes/browser stay MOUNTED while minimized (0×0), so mount-raise doesn't cover
  restore → explicit "raise on !minimized" effect.
