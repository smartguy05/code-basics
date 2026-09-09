# Todos

## Not done — needs the real hardware/adapters

- [ ] **End-to-end Debug against the oneflight solution.** NetCoreDbg 3.1.3-1062
      is now installed (`winget install Samsung.NetCoreDbg`, 2026-09-02) and
      resolves on the user PATH, so the adapter is no longer the blocker. Still
      to do: restart the app (it reads `PATH` from its own process environment,
      so a running instance keeps reporting "not found"), pick a .NET app
      configuration, press **Debug**, and confirm the Redis stream the process
      selects is the debugger-attached one — the whole point of the request.
      **Unknown:** whether NetCoreDbg 3.1.3 supports .NET 10 (the SDK here is
      10.0.203). Nothing on disk answers it; the launch is the test.
- [ ] **Node debug end-to-end.** The adapter now ships with the app, and the
      two previously untested pieces are verified: the bundled server starts
      and prints `Debug server listening at 127.0.0.1:<port>`, and
      `spawn_adapter`'s "last numeric run" parse picks the port out of exactly
      that line. What remains is an actual `pwa-node` launch of a real Node
      configuration — attach, output, breakpoint-free run to exit.
- [ ] **Packaging check: icon *and* bundled adapters.** `pnpm sidecar:build`
      then `pnpm tauri build` (which chains `pnpm debuggers:fetch` itself),
      install the NSIS output, and on a clean machine — or at least a clean
      shortcut — confirm three things: the title-bar/taskbar icon, that
      `resources/debuggers/` landed in the install directory, and that Debug
      works there with **no** `netcoredbg` on PATH and no `CB_DAP_*` set. That
      last one is the only real proof the bundling works, since this dev box
      has a winget netcoredbg that would mask a broken bundle.
      Adds ~11 MB to the installer.

## Deferred by design (stated in the plan, not a gap)

- [ ] Breakpoints, stepping, call stacks, watches and variables. The pure
      `cb_core::dap::{breakpoints, positions, sequence}` layers already exist.
- [ ] Debugging **test** configurations. `start_debug` refuses anything that is
      not `RunKind::App`.

## Housekeeping

- [ ] `src-tauri/.code-basics/symbols.json` is untracked build noise from a run
      that opened `src-tauri/` as a workspace. Delete it or gitignore
      `**/.code-basics/`.
