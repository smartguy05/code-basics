# The Tauri shell (`src-tauri`)

Deliberately thin. Every decision lives in [`cb-core`](core-crate.md); what remains here is state, dispatch, and bridging streamed output onto IPC. When adding functionality, put the logic in `cb-core` first and keep the command here small — that is the repo's core dependency rule.

## Files

| File | Role |
|------|------|
| `src/main.rs` / `src/lib.rs` | Entry point: builds `AppState`, opens a workspace named on the command line (`code-basics .`), registers every command in `tauri::generate_handler!` |
| `src/state.rs` | `AppState` |
| `src/invocation.rs` | Config → command dispatch |
| `src/commands/workspace.rs` | Workspace, config, favourites/ordering, launch-profile, and Rider-import commands |
| `src/commands/files.rs` | Workspace file commands (directory tree listing, file read/write for the editor) |
| `src/commands/run.rs` | Run/test/build execution commands |
| `src/commands/secrets.rs` | .NET user-secrets commands |
| `src/commands/git.rs` | Git commands |
| `src/commands/changelists.rs` | Change-group commands |
| `src/commands/intents.rs` | Agent intent capture and grouping commands |
| `src/commands/inspect.rs` | Object-inspection commands; resolves the bundled sidecar directory |
| `src/recorder.rs` | The one non-window entry point: `record-intent`, re-invoked by an agent hook |

The complete command list with parameters is in the [command reference](../reference/commands.md); it must stay in sync with the `generate_handler!` block in `lib.rs`.

## `AppState`

```rust
pub struct AppState {
    pub workspace: Mutex<Option<Workspace>>,          // the open workspace
    pub supervisor: Supervisor,                        // running processes
    pub last_test_run: Mutex<HashMap<String, TestRunResult>>, // per-config, for "re-run failed"
    pub last_inspect: Mutex<Option<InspectGraph>>,     // the most recent object capture
}
```

The workspace survives a window reload because it lives here, not in the frontend. `last_test_run` is what lets `run_tests(only_failed: true)` know which test names to filter to. `last_inspect` is one slot rather than a map: there is nothing to key a capture by, and a capture is a copy of somebody's process memory — holding several would be holding more of it than anything needs, which is also why `inspect_clear` genuinely drops it.

## `invocation::build`

The **only** place that knows which adapter owns which ecosystem. Given a `RunConfig` and an optional failed-test filter it:

1. Resolves the project directory and ensures `.code-basics/results/` exists (runners write reports there).
2. Dispatches on `config.ecosystem`: `"dotnet"` → `adapters::dotnet`, `"node"` → `adapters::node`, anything else → a matching declarative manifest from `.code-basics/adapters/`.
3. Returns the fully resolved `Invocation` for the supervisor.

`build` also arms crash-dump capture when the workspace opted in (`session::arm_dumps`), which layers the `DOTNET_Dbg*` variables *under* the config's own environment and prunes what is already on disk. A workspace that has not opted in gets nothing, and a dumps directory that cannot be created is never a reason to refuse to start the user's application.

## Bundled resources

`tauri.conf.json` ships one thing the user is not expected to already have:

```json
"bundle": { "resources": { "resources/inspector/": "inspector/" } }
```

Everything else code-basics runs (`dotnet`, `node`, `git`) is found on `PATH`. The object-inspector sidecar cannot be, so it is bundled — `commands/inspect.rs` resolves it with `BaseDirectory::Resource`, and `None` is an ordinary answer because `cargo build` does not produce it. Why this departure was unavoidable: [live inspection](live-inspection.md#bundling-a-departure).

## Streaming output to the UI

Commands that run processes (`start_run`, `run_tests`, `git_network`, `inspect_capture`) accept a Tauri `Channel<ProcessEvent>`. A forwarding task pumps supervisor events onto the channel as they arrive, so console output reaches the UI live rather than in one burst at exit. A closed channel (window went away) just stops forwarding — the process itself is left to finish.

`run_tests` additionally waits for exit, parses the report file, records the result in `last_test_run`, and returns a `TestRunOutcome` containing the flat result, the built tree, and any invocation warnings.

## Error convention

Every command returns `Result<T, String>` — errors cross IPC as plain human-readable strings (formatted with `{e:#}` so anyhow context chains survive). The frontend normalises them with `errorMessage()` in `src/ipc/api.ts`.

## Git commands

A `Repo` handle is opened per call rather than held in `AppState`: libgit2's `Repository` is not `Sync`, and opening is cheap next to any operation performed on it.

Related: [the frontend](frontend.md) · [the IPC type contract](ipc-contract.md).
