# Todos — optional features

## Blocked on the user

- [ ] **The quality-gate Stop hook is currently SILENCED** (2026-08-30), because
      it shells out to `pnpm typecheck`, which cannot pass from an agent shell
      (see `notes.md`). Removed from **both** `.claude/settings.json` (tracked by
      git — do not commit the emptied file without meaning to) and
      `~/.claude/settings.json`. Backups: `<file>.qgate-bak` beside each.
      Restore by copying the backup back, or reinstall from the app:
      Changes → Intent → *Set up agent intent capture* → **Quality gate**.
      The `record-intent` hooks were deliberately left in place.
- [ ] Run `pnpm typecheck`, `pnpm test`, `pnpm coverage` — unrunnable from the
      agent shell (pnpm junctions; see `notes.md`).
- [ ] Decide what to do about the flaky
      `process::tests::cancel_stops_a_long_running_process` (see `notes.md`).

## Stage 2 — Ask the codebase

- [ ] `review::agent_args_interactive` + tests.
- [ ] `terminal_open` gains `program` / `args`; `spec_program` free function
      beside `spec_cwd`, tested.
- [ ] `TerminalDescriptor.command`, `makeAgentTerminal`, `TerminalPanel` prop.
- [ ] `askLogic.ts` + tests; `AskPanel.tsx`; `WorkspaceTabHandle.openAskTerminal`.
- [ ] Gate on `askCodebase` — when off, never register the key listener, so
      Ctrl+/ returns cleanly to the editor's `toggleComment`.

## Stages 3-5 — SQL console

Tracked in `.memories/features/sql-console/` when that work starts.

- [ ] `cargo +1.82 check` against `sqlparser`, `sqlx 0.8`, `tiberius 0.12`
      **before** any of it lands — MSRV is 1.82.
- [ ] Add `{ id: "sql", label: "SQL" }` to `TABS` and `sql: "sqlConsole"` to
      `FEATURE_BY_TAB` in `WorkspaceTab.tsx` (the gate is already wired; the map
      is deliberately empty until there is something to gate).
- [ ] Measure and record the build-time and `target/` size cost of the drivers.

## Stage 6 — installers

- [x] Fork `installer.nsi` into `src-tauri/installer/windows/`, header comment
      naming the upstream tag, nsDialogs page after `MUI_PAGE_DIRECTORY`.
- [x] `bundle.linux.deb.files` seed at `/usr/share/code-basics/features.json`.
- [x] Pin the installer→app contract with tests in `features/store_tests.rs`
      (NSIS bytes, Linux seed, both seed paths, every `FeatureId` in both).
- [ ] `docs/getting-started/installation.md` — the fork's re-sync step.
- [ ] First-ever Linux build; decide whether to add `linux-x64` to
      `scripts/build-sidecar.mjs` (currently the Objects tab ships inert there).
