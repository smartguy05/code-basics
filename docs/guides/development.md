# Development guide

## Commands

| Command | What it does |
|---------|--------------|
| `pnpm install` | Install frontend dependencies |
| `pnpm tauri dev` | Run the full app with hot reload (Vite on port 1420 + Rust shell) |
| `pnpm dev` | Frontend only, in a browser — `invoke` calls fail; layout work only |
| `pnpm typecheck` | `tsc --noEmit` |
| `pnpm build` | Typecheck + Vite production build into `dist/` |
| `pnpm tauri build` | Full release build: executable + installers. Chains `pnpm debuggers:fetch`, but does **not** build the sidecar — run `pnpm sidecar:build` first |
| `pnpm sidecar:build` | Publish the C# inspector (x64 + x86) into `src-tauri/resources/inspector/`. Skipped silently without the .NET SDK |
| `pnpm debuggers:fetch` | Vendor the NetCoreDbg and js-debug adapters into `src-tauri/resources/debuggers/`. Pinned versions, SHA-256 verified, idempotent, and chained into `pnpm tauri build`. Skipped with a warning when offline |
| `pnpm test` | Frontend unit tests (vitest, node environment) |
| `pnpm coverage` | Frontend tests with coverage; fails under 70% lines on the `*Logic.ts` modules |
| `cargo test -p cb-core` | All core tests (the entire logic layer) |
| `cargo test -p cb-core <name>` | Tests whose names contain `<name>` |
| `cargo test -p cb-core --test lsp_oracle -- --ignored --nocapture --test-threads=1` | The real-language-server oracle. Not part of the normal run: it starts whichever servers this machine has and skips the languages it does not. See [language servers](language-servers.md#verifying-against-a-real-server) |
| `cargo llvm-cov --workspace --summary-only --fail-under-lines 70 --ignore-filename-regex "src.tauri.src.main\.rs\|process.kill\.rs\|bin.fake_lsp\.rs"` | Rust coverage gate (install with `cargo install cargo-llvm-cov` + `rustup component add llvm-tools-preview`) |
| `cargo clippy` | Lint the Rust workspace |
| `cargo fmt` | Format the Rust workspace (stock rustfmt defaults — there is no `rustfmt.toml`) |
| `pnpm docs:index` | Regenerate [docs/INDEX.md](../INDEX.md) from the source tree |
| `pnpm docs:check` | Verify doc line limits and relative links |

Frontend tests use vitest (node environment, no jsdom) and cover only the pure `*Logic.ts` modules extracted from components — see the frontend conventions below. Decision-making logic still belongs in `cb-core` first — that is the point of the [architecture](../architecture/overview.md).

### Coverage

Two separate gates, both ≥70% lines:

- **Rust**: `cargo llvm-cov` over the whole workspace. Excluded (whole-file, via `--ignore-filename-regex`): `src-tauri/src/main.rs` (Tauri entry point) and `crates/core/src/process/kill.rs` (`taskkill`/libc platform forks, exercised indirectly by the `process::` tests) and `crates/core/src/bin/fake_lsp.rs` (the scripted language-server stand-in — test-only code that ships in no build, so its own coverage would only dilute the product figure). Run it from a shell with `sh` on PATH (Git Bash) — the `process::` tests spawn `sh` and fail without it.
- **Frontend**: `pnpm coverage`, thresholds configured in `vite.config.ts` over the `*Logic.ts` include list only — rendering components are deliberately outside the metric.
- **Reading the real per-file frontend list.** The vitest *text* reporter hides any file already at 100%, so the table `pnpm coverage` prints lists only the modules below 100% — it is **not** the full set of measured files. To confirm a specific module is actually included in the metric, read `coverage/coverage-final.json` or run with `--coverage.reporter=json-summary`; do not conclude from the printed table alone that a module is uncovered (that is how the `nodeTargets.ts` include gap once survived a gate round).

### Build output is enormous — check on it

A single target directory for this workspace runs to **2–6 GB**, and `target/debug/deps` reaches ~39 GB once it has accumulated across many builds. `cargo llvm-cov` keeps its own (`target/llvm-cov-target`, ~5 GB), so a repository that has been built, tested and covered is comfortably 50 GB before anything unusual happens.

Nothing prunes any of it. Worth knowing:

- `target/debug/incremental` is pure rebuild cache and can be deleted at any time.
- `cargo clean` reclaims everything at the cost of one full rebuild. There is no *build* state in `target/` worth preserving — but see the caveat below: the installed intent hooks run `target/release/cb-app.exe`, so deleting `target/release` (or `cargo clean`) silently disables intent capture until the next `cargo build --release`. Prefer `rm -rf target/debug target/llvm-cov-target` when reclaiming space, and leave `target/release` in place.
- **Never set `CARGO_TARGET_DIR` to a path inside the repository.** `/target` is gitignored wholesale, so anything parked under it is invisible to `git status` and survives `git clean`. Two sessions have each left tens of gigabytes of per-agent build directories behind this way — once as `target/wf-*`, once under the system temp directory.
- If you split builds across parallel processes to avoid the shared `target/` lock, reuse a small fixed set of paths outside the repo and delete them when the work is done. The lock serialises builds; it does not break them, and serialised cargo is usually cheaper than tens of gigabytes of parallel cargo.

## Local agent hooks

Two kinds of hook run automatically while an AI agent works in this repo. Both are enforcement, not suggestion — they run deterministic shell commands the model cannot skip.

### Quality-gate Stop hook

`.claude/settings.json` registers `cb-app.exe quality-gate` — a self-invoking subcommand (like `record-intent`), not a shipped script — which runs when an agent turn ends and inspects the working-tree change set (`git diff --name-only HEAD` plus untracked files). It:

- runs `pnpm typecheck` if any `*.ts`/`*.tsx` changed, and `cargo fmt --check` if any `*.rs` changed — **blocking the turn** (exit 2, feeding the failure back to the agent) until they pass;
- **blocks** on any changed file that still carries an unresolved `AI-REJECTED <date>` note — the same thing the git `pre-commit` hook refuses, surfaced at turn end instead of at commit time;
- prints a **non-blocking** reminder when a turn edited source but touched no `.memories/` file.

It short-circuits on the `stop_hook_active` payload flag so a blocked-then-fixed turn cannot loop. Heavier Rust checks (`cargo clippy --workspace --all-targets -D warnings`) are opt-in via `CB_GATE_FULL=1` — off by default because they relink and can hit the "app is running ⇒ Access denied" lock. `cargo fmt --check` never relinks, which is why it is the default Rust gate. A gate only runs where the change set *and* the repo's tooling call for it (no `typecheck` script / no `Cargo.toml` ⇒ that gate is skipped), so a user-scope install that fires in every repository stays quiet where it does not apply.

All decisions live in `crates/core/src/qgate/` and are unit-tested there (`cargo test -p cb-core qgate::`); the thin runner that reads the payload and spawns the checks is `src-tauri/src/qgate_run.rs`. The gate is **installable from the app** exactly like the intent hooks — Changes → Intent → *Set up agent intent capture* → **Quality gate** (Enable for this repo / for me), which previews the `settings.json` write and applies it through the same `InstallPlan`/`apply_writes` path (backup-then-write, a distinct `code-basics-qgate` marker so it coexists with the recorder's `Stop` entry). Because it runs `target/release/cb-app.exe`, rebuild release after changing `qgate/`. It complements, and does not replace, running the full command set yourself before claiming done.

### Intent-capture hooks

The `PostToolUse`/`Stop`/`post-commit` hooks that power [agent intent capture](agent-intent-capture.md) run `target/release/cb-app.exe record-intent`. Two consequences: never delete `target/release` (see the build-output caveat above), and **rebuild release** (`cargo build --release`) after changing anything under `crates/core/src/intents/` or `src-tauri/src/recorder.rs`, or the hooks keep running the old binary.

## Where code goes

The dependency rule, restated as a checklist:

- **Decides something** (what to run, how to parse, what a diff means)? → `crates/core`, with tests.
- **Wires a decision to the UI**? → a thin command in `src-tauri/src/commands/`.
- **Draws something**? → `src/`.

## Adding a Tauri command end-to-end

1. Implement the logic as a plain function in `cb-core`, unit-tested there.
2. Add a `#[tauri::command]` wrapper in the right file under `src-tauri/src/commands/` returning `Result<T, String>` (format errors with `{e:#}`).
3. Register it in the `generate_handler!` block in `src-tauri/src/lib.rs` — forgetting this compiles fine and fails at runtime.
4. If it returns a new type, follow the [IPC contract rules](../architecture/ipc-contract.md#rules-when-changing-a-crossing-type): derive the serde attributes, pin the JSON keys in `model.rs` tests, mirror in `src/ipc/types.ts`.
5. Add a typed wrapper in `src/ipc/api.ts` — views never call `invoke` directly.
6. Update the [command reference](../reference/commands.md) and run `pnpm docs:index`.

## Rust conventions

- Sibling test files for big suites: `dotnet_tests.rs` next to `dotnet.rs`, included with `#[cfg(test)] #[path = "..."]`.
- Errors use `anyhow` with `.context(...)` so messages explain what was being attempted; `thiserror` for typed errors where callers branch.
- Module-level `//!` docs explain *why* a module works the way it does (the VSTest/MTP split, the patch direction table, the two-git-implementations rationale). Keep that habit — the docs in this folder link to them rather than duplicating them.
- Report fixtures live in `crates/core/fixtures/reports/`.

## Frontend conventions

- All IPC goes through `src/ipc/api.ts`; components take data as props.
- Commands that return an updated `Workspace` flow it back up via `onWorkspaceChange`.
- Match the existing plain-React style: local `useState`, no state libraries, no router.
- Pure decision helpers (parsing, classification, formatting, index math) live in a co-located `*Logic.ts` module with a `.test.ts` beside it (`views/testsLogic.ts` + `views/testsLogic.test.ts` is the pattern). Components import from the logic module and stay untested rendering shells. Tests are plain vitest, node environment — no jsdom, no React imports.

## Documentation conventions

- Docs live in `docs/`, grouped by directory (`getting-started/`, `architecture/`, `guides/`, `reference/`).
- Keep every file under 500 lines — split logically when approaching the limit.
- Cross-link related docs with relative links; `pnpm docs:check` verifies they resolve.
- `docs/INDEX.md` is generated — never edit it by hand; run `pnpm docs:index`.

## Reclaiming disk space

Third incident, and the largest: the repository reached **62 GB**, of which `target/debug` was 53 GB and `target/debug/deps` alone was **45 GB**. A further **34 GB** sat in four private agent target dirs under the temp directory (`p4-gate`, `p5-a`, `p5-b`, `p5-c`) — created despite the rule above, which is why the rule now has a verification step.

**Why it grows.** `cargo` never garbage-collects `deps/`. Every compilation leaves its artifacts behind under a content hash, and nothing prunes the old ones. This workspace now has **eight** test targets (six integration files plus the lib and the `cb-fake-lsp` bin), each linking its own binary with debuginfo on every run. A session with thirty full `cargo test` runs therefore leaves thirty generations of them.

**Measured, on this workspace, by building all test binaries from a clean `target/debug`:**

| `[profile.dev] debug` | one clean build of every test binary |
|---|---|
| `true` (the cargo default) | **6.1 GB** |
| `"line-tables-only"` (now set) | **3.12 GB** |

So `Cargo.toml` sets `[profile.dev] debug = "line-tables-only"`, which halves the per-build cost. Verified that it keeps what this repository actually reads: a panic still reports `panicked at crates\core\src\lsp\framing_tests.rs:238:5` and the backtrace still carries symbol names. Only interactive-debugger information is lost.

**Halving it does not stop it.** The growth is accumulation, so the durable fix is periodic deletion, and that is a maintenance task somebody has to actually do:

```sh
# Reclaim everything reclaimable. Costs a rebuild and nothing else.
rm -rf target/debug target/llvm-cov-target target/tmp
```

Three things to know before running it:

1. **Do not use `cargo clean`, and do not delete `target/release`.** The `Stop` hook that records intents runs `target/release/cb-app.exe record-intent`. Wiping the release profile silently breaks intent capture until somebody happens to run a release build — a failure whose cause is nowhere near its symptom.
2. **`target/llvm-cov-target` is a second complete build tree** (6 GB when last measured), created by `pnpm coverage` / `cargo llvm-cov`. It is pure cache; delete it after a coverage run.
3. **Check for stray private target dirs, do not assume there are none.** They are outside the repository, so no amount of looking at `git status` or the repo's size will show them:

```sh
ls -d "$TEMP"/claude/*/target 2>/dev/null   # then delete what you find
ls -d target/wf-* target/agent-* 2>/dev/null
```

Do this check whenever a workflow that ran cargo finishes. Agents have been told not to create these and have created them anyway in three separate sessions; the instruction is not sufficient on its own.
- **`pnpm typecheck` / `pnpm test` fail with hundreds of `Cannot find module 'react'` (or `'vitest'`), in files nobody touched.** Every entry in `node_modules` is a pnpm **junction**, and an agent shell here often cannot traverse one: the give-away is `The path cannot be traversed because it contains an untrusted mount point` (os error 448 from cargo, `ERR_MODULE_NOT_FOUND` from node). Nothing is wrong with the code or the install — `node_modules/.pnpm/<pkg>` is intact, only the link into it is unreadable from that process. Disabling the tool sandbox does **not** help. Confirm with `Test-Path node_modules/react/package.json` (False) against `Test-Path node_modules/.pnpm/react@*/node_modules/react/package.json` (True), then say the frontend gate could not be run rather than "reporting" a green you never saw. **Cargo hits the same wall and has a way through**: the `~/.cargo/bin` shim gives `Permission denied` and cannot spawn `rustc`, so invoke the toolchain directly and pin `RUSTC` — point `TC` at `$USERPROFILE/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin`, then run `PATH="$TC:$PATH" RUSTC="$TC/rustc.exe" "$TC/cargo.exe" test -p cb-core`. `pnpm test` must resolve *through* the junctions and so has to be run by the user, but **`tsc` has a way through too**: generate a scratch tsconfig that extends the real one with `baseUrl` plus a `paths` entry pointing every package at its own `node_modules/.pnpm/<name>@<version>/node_modules/<name>` directory, then `npx tsc --noEmit -p` it. Three details are each worth hundreds of fake errors — prefer the package's **own** `.pnpm` directory (a first-match scan picks nested copies that are themselves junctions), point bare specifiers **straight at their `@types` package** (once `react` resolves to untyped JS, TypeScript's `@types` fallback walks the unreadable `node_modules/@types`, and `typeRoots` does *not* fix that — it governs global inclusion only), and add vite's `client.d.ts` to `include` for `?raw` imports. Measured on this repo: 1 error, and that one is a pre-existing `/// <reference types="vite/client" />` that `paths` cannot redirect. The `Stop` quality-gate hook still cannot be satisfied from such a session — it runs the literal `pnpm typecheck` — but since the `Unrunnable` change it **no longer blocks** on it: it prints that nothing was checked. Do not read that as a pass; a real type error still hides in that wall of noise, so read it rather than dismissing the whole run.
- **The Objects tab is dead in a fresh clone and nothing reports an error.** `pnpm tauri build` runs `beforeBuildCommand: "pnpm debuggers:fetch && pnpm build"` — which chains the *debug adapters* but **not** `pnpm sidecar:build` — and `src-tauri/resources/inspector/` is gitignored. With no sidecar present `inspect_status` reports the feature unavailable — by design, since missing .NET is not a build failure, but it means a clean checkout ships an inert tab. Run `pnpm sidecar:build` manually before bundling. Under `pnpm tauri dev`, `CB_INSPECTOR_PATH` (a directory or a single binary) overrides the bundled copy.
