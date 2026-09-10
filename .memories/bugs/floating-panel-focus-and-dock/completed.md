# Completed — floating-panel focus & terminal dock remount

## Bug 2: docking a terminal closed it and opened a new one (PTY/session lost)
Root cause: `createPortal` container swap on dock/undock remounts children.
- **New** `src/components/StablePortal.tsx` — stable host node, imperatively moved.
- `src/components/DockableTerminal.tsx` — render via `StablePortal` (also fixed the
  false "no remount" doc).
- `src/components/DockableEditorSlot.tsx` — same swap (also fixes editor LSP/scroll
  loss on dock/undock).
- `src/components/RegionContext.tsx` — corrected the module doc.

## Bug 1: clicking the browser didn't bring it forward (behind terminals/Notes)
Root cause: no cross-panel raise order + purely geometric page occlusion.
- **New** `src/components/focusOrderLogic.ts` (+ `.test.ts`) — `raiseFocus`,
  `syncFocusOrder`, `focusOffset`, `FOCUS_STACK_SPAN`. Generalises the terminal
  stack helpers.
- `src/components/terminalLogic.ts` — re-exports the old names
  (`raiseTerminal`/`syncStackOrder`/`stackOffset`/`TERMINAL_STACK_SPAN`) from it,
  so existing callers/tests untouched.
- **New** `src/components/focusOrderContext.tsx` — App-level `FocusOrderProvider`,
  `useFocusOrder`/`useFocusOffset`/`useFocusEntry`.
- `src/App.tsx` — wrapped tree in `<FocusOrderProvider>` (inside OcclusionProvider).
- `src/components/WorkspaceTab.tsx` — dropped per-workspace `stackOrder`; terminals
  + browser use the shared order (namespaced `dockId`); presence raise/release.
- `src/components/BrowserPanel.tsx` — `focusId` prop; raise on pointerdown/mount/
  restore; writes `--cb-stack`; `peerPanelRects` now returns `{rect, offset}`;
  `sync` uses `occludedByAbovePanels`; re-sync on `focusOrder` change.
- `src/components/browserPanelLogic.ts` (+ test) — added `occludedByAbovePanels`.
- `src/components/NotesPanel.tsx` — joins the focus order (raise + `--cb-stack`).
- `src/styles.css` — `.browser-panel` and `.notes-panel` z-index now
  `calc(var(--z-panel) + 1 + var(--cb-stack,0))`; band comment updated; `--z-notes`
  retained but unused.

## Verification
- `pnpm typecheck` clean.
- `pnpm test` — 2067 pass (incl. new focusOrderLogic + occludedByAbovePanels tests;
  terminal-stack + CSS-span pin tests green via re-export).
- `pnpm coverage` — 98.21% lines, gate passed.
- Manual (`pnpm tauri dev`) still to run by user: dock/undock keeps terminal
  session + docked-editor state; click browser ↔ terminal/Notes swaps front and the
  page shows/hides accordingly; URL bar still focuses on click.
