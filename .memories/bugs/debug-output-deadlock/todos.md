# Todos

- [x] **Verified in the running app** (2026-09-02): user confirmed Debug on
      `ONEflight.Server.Api` now streams application output. Ran from
      `target/release/cb-app.exe`, whose sibling `debuggers/` resource dir the
      cargo build had already populated.
- [x] Frontend gate run and green (2026-09-02): `pnpm typecheck` clean,
      `pnpm test` 57 files / 1441 tests passed. **The pnpm junctions were
      traversable in this session** — contradicting the standing note in
      `CLAUDE.md`, so the block is intermittent rather than permanent. Test
      `Test-Path node_modules/react/package.json` before assuming the frontend
      gate is unreachable.
- [ ] The handshake loop still *discards* `output` events that arrive before the
      `initialize` response (pre-existing `_ => {}` arm). Harmless today, but an
      adapter that greets on stdout would lose it.
- [ ] Two pre-existing unrelated test failures are worth their own work items
      (see `completed.md` for the names).

## How to reproduce the original bug

`scratchpad/drive3.js` in the session that fixed this: drive netcoredbg through
`initialize`/`launch`/`configurationDone`, then `child.stdout.pause()`. The
debuggee stops accumulating CPU entirely, every thread parks in
`Wait, UserRequest`, and it never binds its port.
