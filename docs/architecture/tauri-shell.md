# The Tauri shell (`src-tauri`)

Deliberately thin. Every decision lives in [`cb-core`](core-crate.md); what remains here is state, dispatch, and bridging streamed output onto IPC. When adding functionality, put the logic in `cb-core` first and keep the command here small — that is the repo's core dependency rule.

## Files

| File | Role |
|------|------|
| `src/main.rs` / `src/lib.rs` | Entry point: builds `AppState`, opens a workspace named on the command line (`code-basics .`), registers every command in `tauri::generate_handler!` |
| `src/state.rs` | `AppState` |
| `src/invocation.rs` | Config → command dispatch |
| `src/commands/workspace.rs` | Workspace, config, favourites/ordering, launch-profile, and Rider-import commands |
| `src/commands/run.rs` | Run/test/build execution commands |
| `src/commands/secrets.rs` | .NET user-secrets commands |
| `src/commands/git.rs` | Git commands |

The complete command list with parameters is in the [command reference](../reference/commands.md); it must stay in sync with the `generate_handler!` block in `lib.rs`.

## `AppState`

```rust
pub struct AppState {
    pub workspace: Mutex<Option<Workspace>>,          // the open workspace
    pub supervisor: Supervisor,                        // running processes
    pub last_test_run: Mutex<HashMap<String, TestRunResult>>, // per-config, for "re-run failed"
}
```

The workspace survives a window reload because it lives here, not in the frontend. `last_test_run` is what lets `run_tests(only_failed: true)` know which test names to filter to.

## `invocation::build`

The **only** place that knows which adapter owns which ecosystem. Given a `RunConfig` and an optional failed-test filter it:

1. Resolves the project directory and ensures `.code-basics/results/` exists (runners write reports there).
2. Dispatches on `config.ecosystem`: `"dotnet"` → `adapters::dotnet`, `"node"` → `adapters::node`, anything else → a matching declarative manifest from `.code-basics/adapters/`.
3. Returns the fully resolved `Invocation` for the supervisor.

## Streaming output to the UI

Commands that run processes (`start_run`, `run_tests`, `git_network`) accept a Tauri `Channel<ProcessEvent>`. A forwarding task pumps supervisor events onto the channel as they arrive, so console output reaches the UI live rather than in one burst at exit. A closed channel (window went away) just stops forwarding — the process itself is left to finish.

`run_tests` additionally waits for exit, parses the report file, records the result in `last_test_run`, and returns a `TestRunOutcome` containing the flat result, the built tree, and any invocation warnings.

## Error convention

Every command returns `Result<T, String>` — errors cross IPC as plain human-readable strings (formatted with `{e:#}` so anyhow context chains survive). The frontend normalises them with `errorMessage()` in `src/ipc/api.ts`.

## Git commands

A `Repo` handle is opened per call rather than held in `AppState`: libgit2's `Repository` is not `Sync`, and opening is cheap next to any operation performed on it.

Related: [the frontend](frontend.md) · [the IPC type contract](ipc-contract.md).
