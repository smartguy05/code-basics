# Completed — Running-processes panel + crash-orphan detection

## cb-core: `running/` module (new)
- `record.rs` — `RunKind` (Run/Build/Terminal/Review/Behavioral), `RunningRecord`
  {pid,kind,label,root,key,program,startedAtMs}, `RunningReport` {live,orphans,warnings}.
  camelCase, Type; key-pin tests in `record_tests.rs`.
- `classify.rs` — pure `classify_orphans(Vec<(record, Option<ProcInfo>)>)` +
  `identity_matches` (name basename case-insensitive + start-time within 2s). Dead pid →
  drop silently; reused pid → warn, never orphan.
- `probe.rs` — sysinfo seam: `probe(pid) -> Option<ProcInfo{name,startedAtMs}>`.
- `store.rs` — `RunningStore` (Arc<Mutex>, clone-cheap). `new(path)` (no I/O),
  `load_orphans(&self, probe)` (startup, mutates in place), record/remove/update_label/
  resolve_orphan/live/orphans/warnings. Atomic temp+rename persist to user-global
  `running.json` (`running_path()`, `CB_RUNNING_PATH` override) — mirrors notes.rs.
- `mod.rs` — `RunMeta{root,label,kind}` + `observe(pid,key,meta,fallback_program)` (probes
  own pid for identity, falls back to fallback+now).
- Added `sysinfo = "0.33"` (default-features=false, features=["system"]) to Cargo.toml.
- 18 unit tests pass.

## Spawn wiring
- `process/mod.rs`: `Supervisor` gains `store: Option<RunningStore>` + `with_store`;
  `run` → `run_inner(meta: Option)`, added `run_tracked`. Records on insert (with pid),
  removes on reap.
- `pty/mod.rs`: `PtyManager` gains `store` + `with_store`; `open` → `open_inner`, added
  `open_tracked`. Records on insert; waiter thread removes on reap (store+root moved in).
- `state.rs`: `AppState` now has `pub running: RunningStore`; manual `Default` builds it and
  injects clones into global supervisor + pty; `WorkspaceSlot::new(workspace, running)`
  builds per-slot `Supervisor::with_store`.

## Commands / IPC
- `commands/running.rs` (new): `list_running`, `kill_running` (routes by kind; orphan killed
  by pid only after identity re-probe via `identity_matches`).
- `commands/terminal.rs`: `terminal_open` gains `label`, uses `open_tracked`; new
  `terminal_set_label` (update_label on rename).
- `run.rs`/`review.rs`/`behavioral.rs`: switched their supervisor.run calls to run_tracked
  with RunMeta (Run/Build/Review/Behavioral).
- `lib.rs`: registered 3 new commands; setup hook calls `running.load_orphans(probe::probe)`
  once at startup.
- `ipc/types.ts`: `ProcessKind` (NOT RunKind — that name is taken by "app"|"test"),
  `RunningRecord`, `RunningReport`. `ipc/api.ts`: `listRunning`, `killRunning`,
  `terminalSetLabel`, `terminalOpen` label arg.

## Frontend
- `components/runningLogic.ts` (+ 12 tests): kindIcon/kindLabel, rootBasename, formatAge,
  liveCount, isEmpty, killRequest.
- `components/RunningPanel.tsx` (new): global floating panel, open/close only (no pill),
  drag/resize via reviewLayoutLogic, live + orphan sections, Kill buttons, confirm on orphan.
- `App.tsx`: titlebar "Running" button w/ live-count badge; polls `listRunning` every 2s;
  `killRunningEntry` + refresh; renders RunningPanel.
- `TerminalPanel.tsx`: passes title as open label; `commitRename` also calls
  terminalSetLabel(sessionId, cwd, title).
- `styles.css`: `.running-panel/.running-badge/.running-row/...`.

## Verification
- `pnpm typecheck` clean; `pnpm test` 1015 passed; `cargo fmt --check` clean;
  `cargo check --workspace --all-targets` clean; `cargo test -p cb-core running::` 18 passed.
  Full `cargo test -p cb-core` exited 0 (zero failures). docs:index regenerated,
  docs:check passed, commands.md updated.
- Manual (pnpm tauri dev, Windows) still to do by user: live list across codebases, kill
  routing, orphan detection after killing the app with a terminal child alive.
