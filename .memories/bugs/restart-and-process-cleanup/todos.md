# Todos

## Item 5 — Build Solution: finish wiring (core helper already done + tested)
1. Backend command `build_solution` in `src-tauri/src/commands/run.rs`:
   - Args: solution path (or index) + `channel: Channel<ProcessEvent>`.
   - Resolve active slot + workspace; find the `Solution`; call `invocation::plan_solution_build(&ws, solution, BuildAction::Build)`.
   - Emit each warning as a `[code-basics]` stderr line on the channel.
   - Run steps **sequentially** via `slot.supervisor.run_tracked(&format!("solution:build"), &step.invocation, tx, RunMeta{ kind: Build, ... })`, streaming combined output; print `[code-basics] building <name>` before each. Stop-on-first-failure or continue? Decide (suggest: continue, report per-project exit).
   - Register in `lib.rs` generate_handler; add to `docs/reference/commands.md`; run `pnpm docs:index`.
2. `src/ipc/api.ts`: `buildSolution(...)` wrapper (mirror `buildProject`).
3. `RunView.tsx`: "Build Solution" button near 🔨/⟳/🧹 (enabled when `workspace.solutions.length > 0`); `runBuildSolution()` opens a build console session and calls `api.buildSolution`.

## Final quality gate (before claiming done)
- Full `cargo test -p cb-core` (from Git Bash so `sh` on PATH).
- Full `pnpm test` + `pnpm typecheck`.
- `pnpm docs:index` + `pnpm docs:check` (after adding build_solution command).
- Consider `cargo clippy` (CB_GATE_FULL) — but app may hold cb-app.exe lock.

## Manual verification (pnpm tauri dev) — from plan file
- Restart frees port + new run cancellable; close app → no orphans; Stop All (runs only); Stop Tests; Build Solution per-project; setup prompt appears on a .NET repo lacking project hooks; LSP restart on a crashed C# server.
