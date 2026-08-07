# Bug: Node configs fail on Windows — "failed to start `pnpm`: program not found"

**Fixed 2026-08-07**, commit `5180d46`.

## Symptom
Running any Node run/test configuration from the app on Windows failed instantly with `failed to start \`pnpm\` in <dir>: program not found`. Surfaced the day the repo gained its own vitest `test` script (the app then detected its own frontend as runnable), but the bug affected every pnpm/npm/yarn config on Windows since the node adapter existed.

## Root cause
`adapters/node.rs` emits bare program names (`pnpm`, `npm`, `yarn`); `process/mod.rs` spawned them with `tokio::process::Command::new(name)`. Windows `CreateProcess` resolves a bare name against PATH by appending `.exe` only — and these tools install as `.cmd` batch shims with no `.exe` (verified: `where pnpm` → only `pnpm.cmd`). The shell's PATHEXT handling is what makes them work in a terminal; a direct spawn gets none of that. `dotnet`/`git` were unaffected (real `.exe`s).

## Fix
`crates/core/src/process/resolve.rs` — a PATHEXT walk applied at the single spawn point in `Supervisor::run`: bare extension-less names resolve to the first PATH hit trying each PATHEXT extension in order (Rust's `Command` launches a `.cmd` by full path itself, wrapping in cmd.exe). Names with extensions/separators and unresolvable names pass through unchanged, so error messages still name what the config asked for. Non-Windows: identity function — zero behaviour change on Linux/macOS (`search()` itself is pure and tested cross-platform).

## Proof
Repro test `process::tests::a_package_manager_cmd_shim_spawns_by_bare_name` (Windows-only, self-skips without pnpm) failed with the exact user-reported message before wiring, passes after. 11 pure resolver tests + full suite (973 lib) green; fmt/clippy/docs:index/docs:check clean.
