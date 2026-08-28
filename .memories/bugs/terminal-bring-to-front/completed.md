# Completed: clicking a terminal did not bring it to front

## Symptom

With several floating terminals overlapping, clicking one did not raise it.

## Root cause

Every floating panel shares the `.review-panel` base at a flat `z-index: 60`
(`styles.css`), and the `.terminal-panel` / `.notes-panel` modifiers set no
z-index at all. Stacking was therefore pure DOM order, and there was no
bring-to-front mechanism anywhere in the app. Notes sat above terminals only
*incidentally*, because `App.tsx` renders it after the `WorkspaceTab` list.

## Fix

- **Separate stack state, not a reordered array.** `WorkspaceTab` gains
  `stackOrder: string[]` (terminal keys, bottom-most first) beside `terminals`,
  reconciled by one effect. The `terminals` array index drives `pillBottom` and
  `cascadeShift`, so reordering *it* to raise a panel would teleport minimized
  pills to different slots and shift un-dragged panels diagonally. Positional
  identity and temporal recency are different facts and must not share a
  representation.
- **Pure helpers in `terminalLogic.ts`** (beside `pillBottom`/`cascadeShift`):
  `raiseTerminal`, `syncStackOrder`, `stackOffset`, `TERMINAL_STACK_SPAN`.
  `raiseTerminal` and `syncStackOrder` return the **same array reference** on a
  no-op — that is what makes `setStackOrder` bail (clicking the front terminal
  costs no render) and what stops the reconciling effect looping.
- **Bands: CSS owns the bases, TS owns the ordinal.** `:root` gains
  `--z-panel: 60`, `--z-panel-stack-span: 100`, `--z-notes: 200`,
  `--z-overlay: 300`. `.terminal-panel` is
  `calc(var(--z-panel) + 1 + var(--cb-stack, 0))`, so no z-index integer is ever
  written in TypeScript. `60 + 1 + 99 < 200`: a terminal can never reach Notes.
  `TERMINAL_STACK_SPAN` is pinned by a test as the drift alarm.
- **Raise trigger** is `onPointerDownCapture` on the **panel root** — root not
  header, so clicking the terminal *body* raises; capture phase so it runs before
  the drag handler and xterm. It neither `preventDefault`s nor stops
  propagation, which is what leaves xterm selection, dragging and the header
  buttons untouched. `restore()` raises too (the pill is a sibling the root
  handler never sees).

## Gotchas

- **`.launcher-overlay` had to move to `--z-overlay`.** It also sat at 60, so
  lifting the terminal band would have floated a terminal over the launcher's own
  dim backdrop — a regression the band change introduces if you only touch the
  panels.
- **One-way consequence of "only Notes stays above":** Review / Behavioral /
  Apps output / Running now sit *below* every terminal and have no raise trigger
  of their own, so they cannot be brought forward. If that bites, give them the
  same `--cb-stack` mechanism inside the base band — do not lower the terminal
  band.
- `stackOffset` clamps the **bottom** of an oversized stack, never the top.
- **No persistence**, deliberately: terminals do not survive a restart and keys
  are `term-${seq}` from a `useRef(0)` that restarts at 1, so a remembered order
  would apply a previous session's stacking to unrelated terminals.

## Files

`src/components/terminalLogic.ts` (+ `.test.ts`), `TerminalPanel.tsx`,
`WorkspaceTab.tsx`, `src/styles.css`.

## Verified

15 vitest cases written for the new helpers. `pnpm test` / `pnpm typecheck` could
**not** be run in that session — every `node_modules` entry is a junction the
agent shell cannot traverse ("untrusted mount point"), which breaks `npx vitest`
and `tsc` alike. The helpers were verified by running the same assertions through
plain `node`; the suite and the typecheck still need a real run, plus the manual
overlap check.
