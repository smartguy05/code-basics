# Completed

## Item 1 — Restart stops the original process (DONE)
- `crates/core/src/process/mod.rs`: added `token: u64` to `Running` + `next_token: Arc<AtomicU64>` on `Supervisor`. `run_inner` now (a) `self.cancel(id).await` before spawn — kills any prior run under the id and awaits the tree-kill so the port frees; (b) tags each run with a token; (c) removes the map/store entry on exit **only if the token still owns the id**, so a superseded run's late exit can't evict its replacement. Added `kill_on_drop(true)` safety net.
- Test: `restarting_an_id_stops_the_old_process_and_keeps_the_new_cancellable` (inline in mod.rs). Reproduced fail, now passes.
- `src/views/RunView.tsx`: `start()` claims a per-id generation (`runGenRef`); its terminal `catch`/`finally` bookkeeping only fires while it's still the current generation, so a restart's old promise doesn't clear the new run's "running" state. Restart button already calls `start()`, which is now restart-safe end to end.

## Item 2 — Kill spawned processes on app close/quit (DONE)
- `src-tauri/src/lib.rs`: switched `.run(generate_context!())` → `.build(...)?.run(|app, event| ...)`; on `RunEvent::ExitRequested | Exit`, sweeps `state.running.live()` and `kill_tree(pid)` each. Every spawning handle records into the one shared `RunningStore`, so live() is the complete pid set. True crash (panic=abort) still relies on next-launch orphan detection (already existed).

## Item 3 — Stop All (runs only) (DONE)
- `src/views/runControlLogic.ts` (new) + `.test.ts`: `runningConfigIdsOfKind(configs, runningIds, kind)` — filters supervisor live ids to configs of a given RunKind; `:build` keys excluded automatically.
- `RunView.tsx`: "Stop All" button after Stop; `stopAllRuns()` cancels every running "app" config.

## Item 4 — Stop Tests (DONE)
- `src/views/TestsView.tsx`: relabeled the existing "Stop" → "Stop Tests"; `cancel()` now stops ALL running test-kind configs (via runningConfigIdsOfKind + runningIds), not just the selected.

## Item 6 — First-open setup prompt now appears (DONE)
- Root cause: `needsSetup` treated user-scope (global) install as installed; Anthony has hooks globally → never prompted.
- `src/components/setupPromptLogic.ts`: intent capture now counts as installed only at **project** scope (`p.capture === "project"`). Gate unchanged (any scope). Updated `setupPromptLogic.test.ts`. Modal already leads with the project-scope button.

## Item 7 — LSP "restart language server" (DONE)
- `src-tauri/src/commands/lsp.rs`: `lsp_restart` command — `take_lsp()` + `request_teardown()` then `ensure_session` (fresh), returns LspStatus. Registered in lib.rs.
- `src/ipc/api.ts`: `lspRestart()`. `lspStatusLogic.ts`: `restartable` flag on summary (true when a server `failed`) + `LSP_RESTART_CAVEAT`. `LspStatus.tsx`: Restart button + caveat in the dropdown. Test added.
- Answered user: restart fixes a crashed server; NOT a missing binary / bad handshake / misconfig (caveat says so).

## Item 5 — Build Solution (PARTIAL — core done, wiring pending)
- `crates/core/src/invocation.rs`: `plan_solution_build(workspace, solution, action) -> (Vec<SolutionBuildStep>, warnings)` — resolves solution members onto scanned projects (mirrors graph.rs), one `dotnet build` per resolved .NET project, warns on unresolved/non-dotnet members. Tests in invocation_tests.rs (2, passing).

## Verification run this session
- `cargo check -p cb-core -p cb-app` clean; `cargo fmt` applied.
- `pnpm typecheck` clean.
- New/changed tests pass: restart (rust), plan_solution_build x2 (rust), runControlLogic/setupPromptLogic/lspStatusLogic (44 vitest).
- NOT yet run: full `cargo test -p cb-core`, full `pnpm test`.
