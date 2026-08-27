# Completed

## 1. Bell-only terminal attention
- `src/components/terminalLogic.ts` — `outputNeedsAttention` now returns true only
  when the chunk contains `\x07` (bell); dropped the `text.length > 0` fallback.
- `src/components/TerminalPanel.tsx` — removed `setAttention(true)` from the
  `exited` and `failed` branches (status text kept). Added prop
  `onAttentionChange?(attention)` + two effects reporting the flag up and clearing
  it (`false`) on unmount.
- Tests: `terminalLogic.test.ts` updated (ordinary output → no flash; bell → flash).

## 2. Workspace-tab flash (which project wants attention)
- `src/components/workspaceTabsLogic.ts` — new pure `shouldFlashWorkspaceTab(root,
  activeRoot, hasAttention) = hasAttention && root !== activeRoot`. Purely derived,
  no imperative clear — switching to the tab makes it active and it stops.
- `src/components/WorkspaceTab.tsx` — `attentionKeys: Set<string>` aggregated from
  each terminal's `onAttentionChange`; reports the aggregate up via new prop
  `onAttentionChange(root, hasAttention)` (and `false` on unmount).
- `src/App.tsx` — `attentionByRoot` state; passes callback to each WorkspaceTab;
  clears entry in `closeWorkspace`; applies `attention` class via the helper.
- `src/styles.css` — `.ws-tab.attention` + `@keyframes ws-tab-flash` (amber, no
  drop shadow — the pill's shadow looked wrong on a flat tab).
- Tests: `workspaceTabsLogic.test.ts` + `shouldFlashWorkspaceTab` cases.

## 3. Crash-safe Notes
- `crates/core/src/notes.rs::save` — atomic write via sibling `.tmp` + `fs::rename`
  (no truncation window); `sibling()` helper; empty-over-non-empty first copies to
  `notes.json.bak`.
- `src/components/notesLogic.ts` — new pure `flushDelay(pendingSince, now, debounce,
  maxWait)` capping the debounce so continuous typing still flushes.
- `src/components/NotesPanel.tsx` — `AUTOSAVE_MAX_WAIT_MS = 1500`, `pendingSince`
  ref, `scheduleSave` uses `flushDelay`; added `pagehide`/`beforeunload` best-effort
  flush effect.
- Tests: `notes_tests.rs` (no temp left behind; `.bak` on empty overwrite; no `.bak`
  on ordinary edit) + `notesLogic.test.ts` `flushDelay` cases.
- Residual limit (documented): a hard SIGKILL can lose the sub-max-wait window, but
  the file never corrupts and graceful close/restart always persist.

## 4. Terminal copy/paste
- `src/components/terminalLogic.ts` — pure `terminalKeyAction(event, hasSelection)`
  → `copy | paste | passthrough`. Ctrl+Shift+C / Ctrl+Shift+V, Ctrl+Insert (copy,
  selection only) / Shift+Insert (paste); plain Ctrl+C stays the shell interrupt.
- `src/components/TerminalView.tsx` — `attachCustomKeyEventHandler` wiring to
  `navigator.clipboard` (best-effort, optional-chained).
- Tests: `terminalLogic.test.ts` `terminalKeyAction` cases.

## Gates run
- `pnpm typecheck` clean; `pnpm test` 982 passed; `cargo test -p cb-core --lib
  notes::` 9 passed; `cargo fmt --check` clean.
- Not yet run: manual `pnpm tauri dev` verification (see plan's Verification).
