# Todos — app launcher

- [x] `cb-core` `launcher/` module: model, parse, store, recents (+ tests first)
- [x] `RunKind::External` + kill routing in `commands/running.rs`
- [x] `src-tauri/src/commands/launcher.rs` + register in `lib.rs`
- [x] `src/ipc/types.ts` + `api.ts` wrappers
- [x] `launcherLogic.ts` + `LauncherPicker.tsx` + titlebar button
- [x] `appOutputLogic.ts` + `AppOutputPanel.tsx`
- [x] Running panel: `external` glyph/label + View action
- [x] Docs: `commands.md`, `core-crate.md`, `frontend.md`, `using-the-app.md`, CLAUDE.md, `docs:index`
- [x] Docs round two: `README.md` (feature bullet), `docs/README.md` (section blurbs),
      `configuration.md` (new **User-global stores** table with every `CB_*_PATH` override),
      `tauri-shell.md` (command-module rows + which supervisor a spawn goes to),
      `ipc-contract.md` (the launcher's key-pinning test)
- [x] `cargo test -p cb-core`, `cargo test -p cb-app --lib`, `cargo fmt`

## Left to do

- [ ] **Run `pnpm typecheck` and `pnpm test`** (and `pnpm coverage`) — the agent shell cannot; see
      `notes.md`. Nothing here has been type-checked by a compiler.
- [ ] Smoke-test in `pnpm tauri dev`: launch `node -e "setInterval(()=>console.log(Date.now()),1000)"`,
      check the Apps panel streams it, the Running panel shows a ⚡ row with View/Kill, the row
      disappears on exit while the tab keeps the exit line, and pin/rename survive a restart.
- [ ] Check `echo hi | findstr hi` is refused with the shell hint when the checkbox is unticked, and
      runs when ticked.

## Possible follow-ups (not asked for)

- An `env` editor in the picker (`Launchable.env` is already stored and honoured, but nothing sets it).
- Auto-discovery of candidate commands (npm scripts, docker-compose services, `*.ps1`) — deliberately
  out of scope for the first cut.
- Restart on an output tab (today: Stop, then run it again from the picker).
