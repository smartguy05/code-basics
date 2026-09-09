# Split docking (regions)

Drag a **file editor tab** or a **terminal** to a screen edge and it docks into a
resizable region there, for side-by-side work. Dragged back to the middle it
undocks. This is a different feature from the minimize-to-pill **dock** (`Dock`,
`dockLogic`, `DockContext`) — to keep the two apart the split-docking code is
called **regions** everywhere.

## What can dock, and what cannot

- **File editors** and **terminals** dock. Both are backed by state that must not
  be thrown away when they move — a `FileEditor`'s buffer is saved only on Ctrl+S,
  and an xterm holds scrollback no reload can recover — so moving one must not
  remount it.
- **Diffs do not dock.** A docked diff would be a second `DiffPane`, and the view
  documents that exactly one may ever be mounted: two share one `ChangesModel` and
  both answer `changes.stage`/`change.next`, so `pickCommandTarget` would route a
  stage to the wrong one. Diff tabs are therefore not drag sources. (Making diffs
  dockable needs per-pane command scoping first — a deliberate follow-up.)

## The model — pure, in `regionLayoutLogic.ts`

The workspace is a **center** plus up to four edge **regions**
(`left`/`right`/`top`/`bottom`). Each region is an ordered stack of **dockables**
with its own active index and a size (a fraction of the workspace on its axis). A
dockable is a bare reference — `{ kind: "tab", id }` (an `openFiles` tab id) or
`{ kind: "terminal", id }` (a terminal key) — never a component. Everything
decidable is here and unit-tested (`regionLayoutLogic.test.ts`): `edgeHitTest`,
`dockInto`/`removeDockable`, `resizeRegion` (clamped 0.15–0.85), `centerTabIds`
(the center is every open tab a region does not claim — the function that makes
duplicate editors impossible), `pruneLayout`, and per-root persistence under
`cb.regions.layout:<root>`.

## The runtime — a portal registry

The one hard constraint (no remount) is met with **portals**. `RegionContext`
(`RegionProvider`) owns the layout and a **slot registry**: `RegionHost` lays out
empty DOM slots and registers each one; the content owners portal into the
matching slot. A portal's container can change without unmounting its children, so
a dockable moves between the center and a region — or between regions — with no
remount, keeping the `FileEditor` document and the xterm session alive.

- **Editors**: `RunView` wraps each `FileEditor` in `DockableEditorSlot`, which
  owns a center host and portals the editor into a region slot when docked.
- **Terminals**: `DockableTerminal` always portals its `TerminalPanel` — into a
  region slot when docked, otherwise into a per-codebase **float layer**
  (`.terminal-float-layer`, a zero-size node inside `.workspace-tab`, so a
  backgrounded codebase's terminals still hide with it). `TerminalPanel` gains a
  `docked` prop that fills the slot and hides its own header (the region's tab
  strip is the header); its `⇥` button docks it to the right to start.

`RegionHost` renders the layout (center + edge regions + `RegionSplitter`s),
`RegionDropOverlay` paints the target edge during a drag (the same `EDGE_BAND` the
hit-test uses), and each region's tab strip reads a small **meta** registry
(label + close) so it need not know editors from terminals.

## State lives per codebase

`RegionProvider` is mounted per `WorkspaceTab`, keyed to the codebase root, so a
region and its contents belong to one open project and hide with it — exactly like
the terminals and floating panels around it. Region layout is persisted per root
and pruned whenever a tab closes or a terminal exits, so a region never renders a
stale id.

## Where the pieces live

`src/components/`: `regionLayoutLogic.ts` (+ test), `RegionContext.tsx`,
`RegionHost.tsx`, `RegionSplitter.tsx`, `RegionDropOverlay.tsx`,
`DockableEditorSlot.tsx`, `DockableTerminal.tsx`; integration in
`views/RunView.tsx` (editors) and `components/WorkspaceTab.tsx` (provider, host,
terminals). CSS is the `Split docking (regions)` block in `styles.css`.
