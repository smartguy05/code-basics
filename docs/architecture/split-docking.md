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
- **The SQL console** and **the embedded browser** dock too, as the singleton
  dockables `{ kind: "sql", id: "sql" }` and `{ kind: "browser", id: "browser" }`.
  The SQL console is ordinary DOM and rides the same `StablePortal` as editors, so
  its connections, CodeMirror doc and streaming query survive the move; its two
  fixed overlays are scoped to the region by a `.sql-docked` marker. The browser is
  an OS surface that lives in the Rust host keyed by root — docking moves only its
  stateless chrome (via `createPortal`) and the page follows the `.browser-page`
  placeholder's rect through the ordinary `sync`, so the page-lifecycle effects
  never remount. A docked panel has no minimize pill — the region tab strip is its
  header — and a panel whose feature is switched off is pruned from its region
  (the provider's `panelIds`). Docked browser occlusion is raise-*unaware*
  (`occludedByPanels`): it sits at the workspace layer, so any overlapping floating
  peer blanks the page.
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

The one hard constraint (no remount) is met with a **stable portal host**.
`RegionContext` (`RegionProvider`) owns the layout and a **slot registry**:
`RegionHost` lays out empty DOM slots and registers each one; the content owners
render through `StablePortal` (`components/StablePortal.tsx`), which keeps **one
host node for the component's whole life** and moves *that node* between the center
host and a region slot with `appendChild`. An imperative DOM move does not touch
React's tree, so a dockable moves between the center and a region — or between
regions — with no remount, keeping the `FileEditor` document and the xterm session
alive.

> **Why not swap the portal container directly?** Changing the container passed to
> `createPortal` is **not** a re-parent in React — it deletes the old portal fiber
> (unmounting the children and running their effect cleanups) and creates a fresh
> one. That is exactly what used to happen on every dock/undock: a docked
> terminal's mount-once effect tore down its PTY and spawned a new empty shell, and
> a docked editor lost its CodeMirror buffer and language-server document. Keeping
> the container stable and moving the node is what fixes it.

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
