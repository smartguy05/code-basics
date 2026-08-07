# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## CRITICAL: Memory Files

**ALWAYS update the per-work-item memory files when relevant.** Memory is scoped **per feature/bug** under `.memories/features/{feature-name}/` or `.memories/bugs/{bug-name}/`, not at the `.memories/` root. These files track work item state across sessions:

| File | Path | Purpose | When to Update |
|------|------|---------|----------------|
| `work-item.md` | `.memories/features/{feature-name}/work-item.md` or `.memories/bugs/{bug-name}/work-item.md` | The feature work item details, ACs, description | When loading or refreshing work item context |
| `plan.md` | `.memories/features/{feature-name}/plan.md` or `.memories/bugs/{bug-name}/plan.md` | Implementation plan for the feature or bug fix | When planning or revising the approach |
| `related-docs.md` | `.memories/features/{feature-name}/related-docs.md` or `.memories/bugs/{bug-name}/related-docs.md` | Pointers to relevant documentation | When discovering docs that inform the work |
| `notes.md` | `.memories/features/{feature-name}/notes.md` or `.memories/bugs/{bug-name}/notes.md` | Issues, gotchas, lessons learned **for this work item** | When debugging/solving something others might hit on this WI |
| `todos.md` | `.memories/features/{feature-name}/todos.md` or `.memories/bugs/{bug-name}/todos.md` | Remaining tasks and tech debt **for this work item** | When adding, completing, or deprioritizing tasks |
| `completed.md` | `.memories/features/{feature-name}/completed.md` or `.memories/bugs/{bug-name}/completed.md` | Completed work record (files touched, root cause, fix) | When finishing the work item (or a major phase of it) |

**Rules:**
1. Update these files **AT ALL TIMES** under the active work item folder — they are that work item's memory.
2. Update `completed.md` immediately after finishing a task (not at end of session).
3. Update `todos.md` to check off completed items and add new discovered tasks.
4. Update `notes.md` with any issue you debug/solve that others might hit.
5. Keep entries concise but descriptive — future you needs to understand.
6. Periodically prune `todos.md` to remove old completed items.
7. Periodically summarize and prune `completed.md` to keep the file size small.
8. **Cross-work-item patterns** (gotchas that recur across multiple work items) belong in `CLAUDE.md` (root or the relevant per-project `CLAUDE.md`), not in any single work item's `notes.md`.

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

### Two build failures that are not code failures

Both look like broken code, both have wasted real time, and the obvious reaction to each is wrong.

- **`cargo build` fails on `cb-app.exe` with `Access is denied. (os error 5)` and no compiler diagnostic.** The app is running and Windows will not let a running exe be replaced. Compilation is fine. Prove it with `cargo check --workspace --all-targets` or a build into a scratch `CARGO_TARGET_DIR`, and say the app is holding the file — do not kill it, since that discards whatever the user had open.
- **`cargo test -p cb-core` appears to hang in the `process::` tests.** Concurrent cargo invocations block on the shared `target/` lock, and those tests spawn real child processes late in the run, so the stall lands somewhere plausible. The suite finishes in about a minute. Give parallel agents their own `CARGO_TARGET_DIR`, and re-run before reporting the suite as broken — a subagent's build report is a claim, not evidence.

## Architecture

Three layers with a strict dependency rule:

1. **`crates/core` (`cb-core`)** — all decision-making logic, deliberately with **no Tauri dependency** so everything is unit-testable headlessly:
   - `workspace.rs` — scans an opened directory for projects. Filesystem-only detection (no MSBuild/npm invocation); skips `SKIP_DIRS`, max depth 10.
   - `adapters/` — per-ecosystem knowledge (`dotnet.rs`, `node.rs`): how to detect a project and build the command line to run/test/build it. `manifest.rs` adds declarative TOML adapters loaded from `.code-basics/adapters/*.toml` in a workspace (see `examples/adapters/` for pytest and cargo-nextest); any runner that emits JUnit XML can be added without Rust code. .NET: no launch profile means `dotnet run`'s default profile applies (`ignore_launch_settings` opts out); detected test configs are Debug-only.
   - `testing/` — parses test report files (`trx`, `junitXml`, `jestLike` formats). The core design: runners stream raw output live to the console, then the test tree is built from the structured report file written at exit.
   - `git/` — libgit2 (`git2`) operations including partial staging and line-level revert via patch manipulation (`patch.rs`). `attribution.rs` matches recorded agent edits onto the current diff **by content only** (recorded line numbers are deliberately discarded), and `grouping.rs` collapses hunks into intent cards: stated intent, then formatting-only, then enclosing symbol. Governing rule: a wrong label is much worse than no label, so every threshold abstains rather than guesses.
   - `intents/` — what a coding agent said it was doing, from Claude Code and Codex. Neither agent can attach a rationale to a tool call, so a `PostToolUse` hook records the geometry and a `Stop` hook records the reason, joined on the turn id both carry (`.code-basics/intents/`, gitignored). `providers/` also mines each agent's existing session files so the feature works retroactively with no setup. Hook installs are always additive and previewed before writing — they touch files the user shares with their team.
   - `inspect/` — reads the real managed heap of a crash dump or a live process via the `cb-inspector` sidecar: one-shot, request file in, `result.json` out, so `process/` needed no changes. `dumps.rs` arms the runtime's own `DOTNET_Dbg*` crash-dump capture (opt-in per workspace, off by default — a dump is a verbatim copy of process memory) and prunes by count and bytes. Same abstain rule: unreadable becomes `Unavailable`, a cap becomes `Elided`. No method is called and no property evaluated; addresses cross as hex strings. Live attach lists **every** .NET process on the machine (`cb-inspector --list-processes` → `DiagnosticsClient.GetPublishedProcesses()`), because `dotnet run` starts the application as a child and the supervisor's pid is the CLI launcher. `session::attribute` labels each row `launched`/`descendant`/`unrelated` from the parent chain — an unrelated row never carries a configuration name, a missing parent pid is *unknown* rather than "no parent", and a chain leaving the known set stops. Builds, git and the sidecar are dropped entirely. The pid is re-checked against a fresh enumeration immediately before spawning, because a recycled pid would render a stranger's heap under the user's label.
   - `importers/rider.rs` — converts Rider `.run/*.xml` configurations.
   - `config.rs` — `.code-basics/config.json` per workspace: only user-created/imported configs are saved; detected ones are re-derived on every scan. Also holds `favorites`/`order` (config ids; favourites sort first, then saved order, then names).
   - `secrets.rs` — .NET user secrets (`%APPDATA%\Microsoft\UserSecrets\<id>\secrets.json`); validates the comment-tolerant JSON dialect .NET accepts and can add a `<UserSecretsId>` to a project.
   - `process/` — process spawning, output chunking, cross-platform kill. Layers colour-enabling env defaults under the config's own. `pid(id)`/`running()` expose what the supervisor already knew, so the inspector can attach to a live process without a second lifecycle.
2. **`src-tauri`** — thin bridge only: app state (`state.rs`), dispatch of a config to the right adapter (`invocation.rs`), and the `#[tauri::command]` surface in `commands/{workspace,run,secrets,git,changelists,intents,files,inspect}.rs`, registered in `lib.rs`. New backend functionality goes in `cb-core` first; commands here should stay small. `recorder.rs` is the one non-window entry point: an agent hook re-invokes this binary as `record-intent`, which reads a payload from stdin and exits without ever creating a window.
3. **`src/` (React 19 + Vite)** — five tab views (`views/`: Run, Tests, Objects, Changes, History — Run, Tests and Objects stay mounted while hidden because they own running processes), a titlebar branch widget, CodeMirror-based diff/config editors and the xterm console (`components/`), and typed IPC wrappers in `ipc/api.ts`. Terminal-hosting panes must be `overflow: hidden` (a scrollbar fights xterm's fit addon).
4. **`sidecar/inspector/`** — the one component not written in Rust or TypeScript: a .NET one-shot process (`cb-inspector`) that walks a heap with ClrMD and writes `result.json`, plus a second mode (`--list-processes`) that enumerates the machine's attachable .NET processes. It makes no product decisions — the platform call is here, the attribution is in `cb-core`. `pnpm sidecar:build` publishes x64 and x86 (~4 MB each) into `src-tauri/resources/inspector/`, shipped via `bundle.resources` — the only bundled resource; everything else is found on `PATH`. Not built by `cargo build`, not covered by `cargo test`, and **absent from `docs/INDEX.md`** because `generate-index.mjs` does not read C#. Missing .NET is not a build failure: the feature reports itself unavailable.

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
