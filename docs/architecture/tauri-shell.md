# The Tauri shell (`src-tauri`)

Deliberately thin. Every decision lives in [`cb-core`](core-crate.md); what remains here is state, dispatch, and bridging streamed output onto IPC. When adding functionality, put the logic in `cb-core` first and keep the command here small — that is the repo's core dependency rule.

## Files

| File | Role |
|------|------|
| `src/main.rs` / `src/lib.rs` | Entry point: builds `AppState`, opens a workspace named on the command line (`code-basics .`), registers every command in `tauri::generate_handler!` |
| `src/state.rs` | `AppState` |
| `src/invocation.rs` | Config → command dispatch |
| `src/commands/workspace.rs` | Workspace, config, favourites/ordering, launch-profile, and Rider-import commands |
| `src/commands/files.rs` | Workspace file commands: directory listing, file read/write for the editor, and create / rename / delete behind the tree's right-click menu — plus the symbol-index upkeep every one of them owes |
| `src/commands/run.rs` | Run/test/build execution commands |
| `src/commands/secrets.rs` | .NET user-secrets commands |
| `src/commands/git.rs` | Git commands |
| `src/commands/changelists.rs` | Change-group commands |
| `src/commands/intents.rs` | Agent intent capture and grouping commands |
| `src/commands/inspect.rs` | Object-inspection commands; resolves the bundled sidecar directory |
| `src/commands/terminal.rs` | Floating-terminal commands over `cb_core::pty` (open/write/resize/close/list) |
| `src/commands/notes.rs` | Global notes / scratchpad commands over `cb_core::notes` (read/write); no `AppState` |
| `src/commands/running.rs` | The Running panel: every live process plus crash-orphan candidates, and a kill routed by kind |
| `src/commands/launcher.rs` | [App-launcher](../getting-started/using-the-app.md#running-other-apps) commands over `cb_core::launcher` (list / launch / stop, pin / rename / forget); the store commands take no `AppState` |
| `src/recorder.rs` | The one non-window entry point: `record-intent`, re-invoked by an agent hook |

The complete command list with parameters is in the [command reference](../reference/commands.md); it must stay in sync with the `generate_handler!` block in `lib.rs`.

### Keeping the symbol index honest is a command's job

`reindex_saved_file` and its mirror `unindex_moved_path` both take an **absolute** path, because the two callers do not mean the same thing by a relative one — the Run tab's are workspace-relative, the Changes tab's come from git and are relative to the *repository*, which `Repo::open` may discover above the workspace. `symbols::index::relative_to_root` re-keys the absolute result, and a file that turns out not to be under the workspace is left alone rather than keyed against a root it does not sit under.

`fs_write_file` and `fs_create_file` re-index what they wrote; `fs_rename` does both (drop the old key, index the new); `fs_delete` only drops. The drop is `remove_file` rather than `replace_file` with an empty symbol list, because `replace_file` deliberately **keeps** the `files` entry when the path is missing on disk — an unreadable file is not a deleted one, and only the caller who performed the deletion knows the difference.

One case is knowingly left wrong. Deleting or renaming a **directory** removes every file under it, and `unindex_moved_path` drops only the key naming the directory itself — which the index never held, so it is a no-op, and the descendants keep their entries until the next rescan. Walking the deleted subtree is impossible (it is already gone), so the only alternative is a prefix sweep of `files`, and a prefix cannot distinguish `src/app` from `src/apple`: it would silently unindex a directory the user still has. A stale entry is visible the moment it is clicked and is self-corrected by the next rescan; a wrongly swept one is invisible. Failure on either path is silent for the same reason as everywhere else here — the file operation *did* happen, and reporting it as failed because a palette entry lingered would be a false report of the thing the user cares about.

## `AppState`

```rust
pub struct AppState {
    pub workspace: Mutex<Option<Workspace>>,          // the open workspace
    pub supervisor: Supervisor,                        // running processes
    pub last_test_run: Mutex<HashMap<String, TestRunResult>>, // per-config, for "re-run failed"
    pub last_inspect: Mutex<Option<InspectGraph>>,     // the most recent object capture
    pub pty: PtyManager,                               // the floating terminals' PTY sessions
    // (plus symbols + a build flag, and the LSP session handle)
}
```

The workspace survives a window reload because it lives here, not in the frontend. `last_test_run` is what lets `run_tests(only_failed: true)` know which test names to filter to. `last_inspect` is one slot rather than a map: there is nothing to key a capture by, and a capture is a copy of somebody's process memory — holding several would be holding more of it than anything needs, which is also why `inspect_clear` genuinely drops it. `pty` is a clone-cheap handle like `supervisor`, holding the open [terminal](../getting-started/using-the-app.md#terminals) sessions keyed by id; unlike the caches it is **not** per-workspace, so `set_workspace` does not clear it — a terminal is not tied to the open root.

Note which supervisor a spawn goes to, because it decides what a codebase switch stops. Configuration runs and
builds use the **per-workspace** `WorkspaceSlot::supervisor`, so closing that codebase can stop them. The
review agent, behavioral runs and **launched apps** use the app-level `supervisor` above, recorded as
`RunKind::External`: an app the user started themselves is not owned by whichever tab was in front, so closing
that tab must not kill it. `commands/running.rs` routes a kill the same way it was spawned.

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

`terminal_open` is the bidirectional variant: it forwards `TerminalEvent`s (one merged stream) the same way, but keystrokes and resizes flow **back** as ordinary commands (`terminal_write` / `terminal_resize`), the input path a supervised process never has.

`run_tests` additionally waits for exit, parses the report file, records the result in `last_test_run`, and returns a `TestRunOutcome` containing the flat result, the built tree, and any invocation warnings.

## Error convention

Every command returns `Result<T, String>` — errors cross IPC as plain human-readable strings (formatted with `{e:#}` so anyhow context chains survive). The frontend normalises them with `errorMessage()` in `src/ipc/api.ts`.

## Git commands

A `Repo` handle is opened per call rather than held in `AppState`: libgit2's `Repository` is not `Sync`, and opening is cheap next to any operation performed on it.

Related: [the frontend](frontend.md) · [the IPC type contract](ipc-contract.md).
