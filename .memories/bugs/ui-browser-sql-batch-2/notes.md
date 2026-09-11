# Notes — UI/browser/SQL bug batch 2 (branch fix/ui-and-browser-bugs)

Six bugs. Plan: `~/.claude/plans/ticklish-napping-crescent.md`.

## Bug 4 — SQL "allow writes" still refused (DONE)
Root cause: `sql/guard.rs` did not classify transaction-control statements, so a
`BEGIN; UPDATE…; COMMIT;` batch had `BEGIN`/`COMMIT` fall into the `other =>` arm
→ `Verdict::Refused{Unrecognised}`, which outranks `Write` and is never lifted by
writes-allowed. So the whole batch was refused with writes on.

Fix: `classify_statement` now returns `Verdict::ReadOnly` for
`Statement::{StartTransaction, Commit, Rollback, Savepoint, ReleaseSavepoint}`
(transaction control modifies no data; ReadOnly is the identity for `stricter`, so
the batch is classified by its real writes). `SetTransaction` is NOT a `Statement`
variant in sqlparser 0.62 (it lives under `Statement::Set(Set)`); general `SET`
stays `Unrecognised` on purpose.

Two existing tests encoded the OLD (buggy) behavior using `COMMIT` as an example of
`Unrecognised`/refused — updated to use `USE master` (`Statement::Use`, genuinely
unclassified) instead. New test: `transaction_control_is_neutral_so_a_wrapped_write_is_liftable`.
All 349 `sql::` tests green.

## Bugs 5/6 — shared minimized dock (DONE)
Replaced every panel's ad-hoc minimize corner (.notes-pill/.sql-pill/.browser-pill/
terminal pillBottom) with one App-level dock. New pure `dockLogic.ts` (dockId/
upsertEntry/removeEntry/visibleEntries) + `DockContext.tsx` (`useDockEntry` hook,
stable setter, entries in App state) + `Dock.tsx`. Each of the 6 panels calls
`useDockEntry(minimized ? {...} : null)`; Notes is scope "global" pinned, the rest
scope=workspace.root (dock shows global+activeRoot only — matches old hidden-tab
behavior). Terminal order/id uses its stable `number` (new prop), not array index.
CSS: `.review-pill` positioning stripped; `.dock`/`.dock-pill` added; new band
`--z-dock: 250` (between notes 200 and overlay 300). Removed `pillBottom`/`PILL_STEP`
from terminalLogic + its test.

## Bugs 1/2/3 — browser panel (DONE)
- Bug 1 (not resizeable): native `resize:both` grip sat inside `.browser-page`,
  which the OS webview covers. Added E/S/SE handles in a 10px gutter (`.browser-page`
  margin) the webview never covers; new pure `resizeFromHandle` in reviewLayoutLogic.
- Bug 2 (Agents modal behind page): added `setupOpen` to `pageVisible` — page hides
  while the panel's own BrowserMcpPanel modal is open.
- Bug 3 (page covers app chrome): new `occlusionContext.tsx` (count-based
  `OcclusionProvider`/`useOccluder`/`useOcclusionCount` + `<Occluder/>` marker).
  Instrumented via `<Occluder/>` at App modal render sites (Settings/About/Features/
  Launcher/SetupPrompt/McpServer) + `useOccluder` inside shared ContextMenu (all
  menus) and SearchEverywhere. Peer floating panels handled geometrically in
  `sync()` (`occludedByPanels` over `.review-panel,.dock` rects; re-checked on
  `pointerup`). Top clamp `clampBrowserTop` keeps page below the tab strip.
  Overlap-scoped to avoid flicker; no on-screen note for occlusion.

## Status
Frontend: typecheck clean, `pnpm test` 1984 pass. Rust: guard tests green,
`cargo fmt` clean. Full `cargo test -p cb-core` running to confirm no regression.
NOTE: not yet run in the live app (`pnpm tauri dev`) — resize/occlusion/dock need a
visual smoke test.
