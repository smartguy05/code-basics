# Architecture overview

code-basics is three layers with one strict dependency rule: **all decision-making lives in `cb-core`, which has no Tauri dependency**. The shell dispatches; the frontend renders.

```
┌─────────────────────────────────────────────────────────┐
│  src/            React 19 + Vite + CodeMirror           │
│                  views, components, typed IPC wrappers  │
└───────────────▲─────────────────────────────────────────┘
                │  Tauri invoke() + Channel<ProcessEvent>
┌───────────────┴─────────────────────────────────────────┐
│  src-tauri/      the Tauri shell (thin, no logic)       │
│                  AppState, adapter dispatch, commands   │
└───────────────▲─────────────────────────────────────────┘
                │  plain function calls
┌───────────────┴─────────────────────────────────────────┐
│  crates/core/    cb-core (no Tauri dependency)          │
│                  workspace scan · adapters · testing    │
│                  git · process supervision · importers  │
└─────────────────────────────────────────────────────────┘
```

Why: everything that decides anything — project detection, command-line construction, report parsing, git patch manipulation — is unit-testable without a windowing system. `cargo test -p cb-core` exercises the entire brain of the application headlessly.

Per-layer detail: [core crate](core-crate.md) · [Tauri shell](tauri-shell.md) · [frontend](frontend.md) · [IPC contract](ipc-contract.md).

## The central design: run a command, read a report

Every supported test runner shares one property: it streams human-readable output live **and** writes a structured report file when it finishes. The app leans on that single observation everywhere:

1. The console shows raw output as it arrives (chunked, UTF-8-safe, `\r`-preserving).
2. When the process exits, the report file is parsed into a flat list of test cases.
3. The flat list is grouped once, in one place, into the project → suite → test tree the UI renders — so every runner produces an identical tree.

This is what makes adapters cheap. Supporting an ecosystem means knowing *which command to run* and *which report format it leaves behind* — three formats ([TRX, Jest-like JSON, JUnit XML](../reference/test-reports.md)) cover everything, and JUnit XML alone covers most of the long tail, which is why [new ecosystems can be added with a TOML file](../guides/adding-an-ecosystem.md) and no code.

## Data flow for the two main operations

**Running tests** (`run_tests` command):

```
RunConfig ──▶ invocation::build (dispatch by ecosystem)
          ──▶ adapter builds Invocation {program, args, cwd, env, report spec}
          ──▶ Supervisor spawns, streams ProcessEvents over a Channel to the UI
          ──▶ on exit: testing::parse_file(report) ──▶ flat Vec<TestCase>
          ──▶ testing::tree::build ──▶ Vec<TestNode> ──▶ UI
```

**Line-level git operations** (`git_stage_lines` / `git_revert_lines`):

```
FileDiff (libgit2) ──▶ user selects line indices in the UI
                   ──▶ patch::build_patch(file, selection, direction)
                   ──▶ `git apply` (forward = stage, reversed = revert)
```

## Deliberate choices worth knowing

- **Filesystem-only project detection, by default.** No MSBuild evaluation, no `npm ls`, no shelling out during a scan. Opening a workspace must feel instant. The single exception is opt-in per workspace: `msbuildEvaluation` in `config.json` trades scan speed for MSBuild's real answers (see [`adapters::msbuild`](core-crate.md)), and falls back to the filesystem scan whenever it cannot run.
- **Two git implementations on purpose.** Reads and local mutations use libgit2 (fast, structured, in-process). Network operations (push/pull/fetch) shell out to system `git` so the user's existing credential setup just works. `git apply` is likewise delegated as the only correct implementation of partial patch application.
- **Process-tree kill.** Cancelling kills the spawned process's whole group/tree; killing only the wrapper leaves `dotnet run`'s built assembly or a bundler alive and holding its port.
- **The VSTest / Microsoft.Testing.Platform split.** The two `dotnet test` paths take different, *mutually ignored* flags; getting it wrong produces a clean-exit run with no report. Telling them apart is the single most important job of the .NET adapter — see [core crate](core-crate.md#adapters).
- **Hand-mirrored IPC types, pinned by tests.** See [IPC contract](ipc-contract.md).
