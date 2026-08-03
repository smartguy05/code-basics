# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`code-basics` is a lightweight Rider/IDE replacement: a Tauri 2 desktop app for running projects, running tests, and doing git work (staging, line-level revert, history) across .NET and JS/TS workspaces.

## Commands

pnpm is the package manager (`pnpm-lock.yaml` is the tracked lockfile; `tauri.conf.json` runs `pnpm dev`/`pnpm build`). Don't introduce `package-lock.json`.

- `pnpm tauri dev` — run the full app (starts Vite on port 1420 + the Rust shell)
- `pnpm dev` — frontend only in a browser (Tauri `invoke` calls will fail)
- `pnpm typecheck` — TypeScript check (`tsc --noEmit`); `pnpm build` typechecks then builds
- `cargo test -p cb-core` — all core Rust tests (unit tests + `crates/core/tests/git_operations.rs`)
- `cargo test -p cb-core <name>` — a single test by substring
- `cargo build` / `cargo clippy` — workspace-wide (both crates)

There is no JS test suite; all logic tests are Rust.

## Architecture

Three layers with a strict dependency rule:

1. **`crates/core` (`cb-core`)** — all decision-making logic, deliberately with **no Tauri dependency** so everything is unit-testable headlessly:
   - `workspace.rs` — scans an opened directory for projects. Filesystem-only detection (no MSBuild/npm invocation); skips `SKIP_DIRS`, max depth 10.
   - `adapters/` — per-ecosystem knowledge (`dotnet.rs`, `node.rs`): how to detect a project and build the command line to run/test it. `manifest.rs` adds declarative TOML adapters loaded from `.code-basics/adapters/*.toml` in a workspace (see `examples/adapters/` for pytest and cargo-nextest); any runner that emits JUnit XML can be added without Rust code.
   - `testing/` — parses test report files (`trx`, `junitXml`, `jestLike` formats). The core design: runners stream raw output live to the console, then the test tree is built from the structured report file written at exit.
   - `git/` — libgit2 (`git2`) operations including partial staging and line-level revert via patch manipulation (`patch.rs`).
   - `importers/rider.rs` — converts Rider `.run/*.xml` configurations.
   - `config.rs` — `.code-basics/config.json` per workspace: only user-created/imported configs are saved; detected ones are re-derived on every scan.
   - `process/` — process spawning, output chunking, cross-platform kill.
2. **`src-tauri`** — thin bridge only: app state (`state.rs`), dispatch of a config to the right adapter (`invocation.rs`), and the `#[tauri::command]` surface in `commands/{workspace,run,git}.rs`, registered in `lib.rs`. New backend functionality goes in `cb-core` first; commands here should stay small.
3. **`src/` (React 19 + Vite)** — four tab views (`views/`: Tests, Run, Changes, History), CodeMirror-based diff/config editors (`components/`), and typed IPC wrappers in `ipc/api.ts`.

### The Rust↔TS type contract

`src/ipc/types.ts` hand-mirrors the Rust model types (`crates/core/src/model.rs`, `workspace.rs`, `git/`) — there is no codegen wired up (specta derives exist but are unused for export). Rust structs use `serde(rename_all = "camelCase")`, and the `tests` module at the bottom of `model.rs` pins the exact JSON keys each type produces (the `types.ts` comment calls this `serialisation_shape`; the tests exist under different names). **When changing a Rust type that crosses IPC, update the key-pinning test in `model.rs` and `types.ts` together.** Full rules: `docs/architecture/ipc-contract.md`.

## Documentation and the code index

- `docs/INDEX.md` is a **generated** map of every source file (with one-line purpose), the full Tauri command surface, the `ipc/api.ts` wrappers, and each `cb-core` module's public API. **Consult it first when locating code** — it is usually faster than searching. Never edit it by hand.
- `pnpm docs:index` regenerates it (`scripts/generate-index.mjs`). Run it after adding/removing source files, Tauri commands, or public core APIs.
- `pnpm docs:check` (`scripts/check-docs.mjs`) enforces the docs rules: every markdown file in `docs/` (plus README.md and this file) stays under 500 lines, and all relative links resolve. Run it after editing docs.
- Human docs live in `docs/` grouped by directory (`getting-started/`, `architecture/`, `guides/`, `reference/`), entry point `docs/README.md`. When changing behavior documented there (commands, config format, adapter schema), update the matching doc — `docs/reference/commands.md` in particular must track `generate_handler!` in `src-tauri/src/lib.rs`.

### Other conventions

- Rust tests for adapters live in sibling `*_tests.rs` files included via `#[path]` under `#[cfg(test)]`.
- Streaming process output uses Tauri `Channel<ProcessEvent>` (see `startRun` in `ipc/api.ts` and `commands/run.rs`).
- Per-workspace state (config, declarative adapters, test report output) all lives under `.code-basics/` in the opened workspace.
- The app accepts a directory argument (`code-basics .`); a bad path logs and still opens the welcome screen.
