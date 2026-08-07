# Tauri command reference

Every IPC command, as registered in `src-tauri/src/lib.rs`. All commands return `Result<T, String>` — errors arrive as human-readable strings. The frontend calls these only through the typed wrappers in `src/ipc/api.ts` (wrapper names are the camelCase of the command names; parameters cross as camelCase, e.g. `config_id` → `configId`).

Types referenced below are documented in [the IPC contract](../architecture/ipc-contract.md) and mirrored in `src/ipc/types.ts`. This list must stay in sync with `generate_handler!`; `pnpm docs:check` will not catch drift here, but the generated [INDEX.md](../INDEX.md) lists the registered commands for comparison.

## Workspace & configuration

`src-tauri/src/commands/workspace.rs`

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `open_workspace` | `path: String` | `Workspace` | Scans, merges saved configs, stores as current |
| `current_workspace` | – | `Workspace \| null` | Survives window reload |
| `rescan_workspace` | – | `Workspace` | Re-detects; keeps saved configs layered on top |
| `save_config` | `config: RunConfig` | `Workspace` | Persists to `.code-basics/config.json` |
| `delete_config` | `id: String` | `Workspace` | |
| `preview_rider_import` | – | `RiderImportPreview` | Parses `.run/*.xml`; writes nothing |
| `apply_rider_import` | `configs: RunConfig[]` | `Workspace` | Saves the reviewed selection |
| `launch_profiles` | `project: String` | `LaunchProfile[]` | Launch profiles a .NET project defines, with their environment, arguments and application URL. Profiles `dotnet run` cannot apply (IIS Express, Docker) come back with `launchable: false` rather than being omitted |
| `set_favorite` | `id: String`, `favorite: bool` | `Workspace` | Starred configs sort first; persisted in `config.json` |
| `set_config_order` | `order: String[]` | `Workspace` | Preferred ordering as config ids; unlisted ids keep name order after them |

## Workspace files

`src-tauri/src/commands/files.rs` — backs the Run tab's directory tree and file editor. All paths are workspace-relative; paths that would escape the root (absolute, `..`) are rejected.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `fs_list_dir` | `path: String` | `DirEntry[]` | One directory per call (the tree expands lazily); directories first, sorted case-insensitively, `SKIP_DIRS` (`node_modules`, `bin`, `obj`, …) hidden |
| `fs_read_file` | `path: String` | `String` | UTF-8 text only; binary or >5 MB files are a clear error |
| `fs_write_file` | `path: String`, `content: String` | `()` | Saves the file editor's contents (Ctrl+S) |

## .NET user secrets

`src-tauri/src/commands/secrets.rs` — `project` is the workspace-relative `.csproj` path a .NET `RunConfig.project` holds. Secrets live in `secrets.json` under the user profile (`%APPDATA%\Microsoft\UserSecrets\<id>\` on Windows, `~/.microsoft/usersecrets/<id>/` elsewhere), never in the repository.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `read_project_secrets` | `project: String` | `ProjectSecrets` | Id, secrets file path, and contents when the file exists |
| `write_project_secrets` | `project: String`, `content: String` | `ProjectSecrets` | Validates JSON; adds a `<UserSecretsId>` to the project first when missing, like `dotnet user-secrets init` |

## Running & tests

`src-tauri/src/commands/run.rs`

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `start_run` | `config_id: String`, `channel: Channel<ProcessEvent>`, `env: Map?` | `()` | Streams output; resolves on exit. `env` is layered over the config's own for this run only (the Run tab's environment picker) |
| `build_project` | `config_id: String`, `action: "build" \| "rebuild" \| "clean"`, `channel: Channel<ProcessEvent>` | `()` | .NET only; runs `dotnet build` / `build --no-incremental` / `clean`, registered as `<config_id>:build` |
| `cancel_run` | `config_id: String` | `bool` | Kills the process **tree** |
| `running_ids` | – | `String[]` | Config ids currently running |
| `run_tests` | `config_id: String`, `only_failed: bool`, `channel: Channel<ProcessEvent>` | `TestRunOutcome` | Streams output, then parses the report; `only_failed` filters to the previous run's failures |
| `last_test_run` | `config_id: String` | `TestRunOutcome \| null` | Most recent result for this config |

## Git

`src-tauri/src/commands/git.rs` — a `Repo` handle is opened per call (libgit2's `Repository` is not `Sync`).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `git_status` | – | `WorkingStatus` | |
| `git_file_diff` | `path: String`, `mode: ComparisonMode` | `FileDiff` | Modes: `workingToHead`, `workingToIndex`, `indexToHead` |
| `git_file_contents` | `path: String`, `mode: ComparisonMode` | `FileContents` | Baseline + working content for the merge view |
| `git_write_file` | `path: String`, `content: String` | `()` | Saves edits made in the diff editor |
| `git_stage_file` | `path: String` | `()` | |
| `git_unstage_file` | `path: String` | `()` | |
| `git_stage_lines` | `path: String`, `lines: u32[]` | `bool` | Patch-based partial staging |
| `git_unstage_lines` | `path: String`, `lines: u32[]` | `bool` | |
| `git_revert_lines` | `path: String`, `mode: ComparisonMode`, `lines: u32[]` | `bool` | Reverse-applies just the selection |
| `git_discard_file` | `path: String` | `()` | |
| `git_commit` | `message: String`, `amend: bool` | `String` | Returns the new commit id |
| `git_branches` | – | `Branch[]` | |
| `git_create_branch` | `name: String`, `checkout: bool`, `from: String?` | `()` | `from` is the revision to branch from; absent means HEAD |
| `git_checkout_branch` | `name: String` | `()` | |
| `git_checkout_remote_branch` | `name: String` | `()` | Like `git switch`: creates the local tracking branch (or reuses it), then switches |
| `git_delete_branch` | `name: String` | `()` | |
| `git_merge_branch` | `name: String` | `MergeReport` | Merge a branch into the current one. Refuses to start with modified tracked files or another operation in progress. Conflicts do not error: they come back as `outcome: "conflicted"` with the paths, and the merge is **left in progress** to resolve in the Changes tab |
| `git_abort_merge` | | `()` | Discard an in-progress merge and return to the pre-merge commit |
| `git_changelists` | | `Changelists` | The workspace's change groups |
| `git_create_changelist` | `name: String` | `Changelists` | Add an empty group; rejects a duplicate or blank name |
| `git_delete_changelist` | `name: String` | `Changelists` | Delete a group; its files become ungrouped |
| `git_rename_changelist` | `from: String, to: String` | `Changelists` | Rename a group, keeping its members |
| `git_assign_to_changelist` | `paths: String[], group: String?` | `Changelists` | Move files into a group, or out of every group when `group` is null |
| `git_history` | `limit: u32` | `Commit[]` | |
| `git_commit_diff` | `id: String` | `FileDiff[]` | |
| `git_stash_save` | `message: String` | `()` | |
| `git_stash_pop` | – | `()` | |
| `git_network` | `kind: NetworkKind`, `channel: Channel<ProcessEvent>` | `i32 \| null` | Push/pull/fetch via system `git`; returns the exit code |

## Agent intent

`src-tauri/src/commands/intents.rs` — collapsing hunks into the decisions behind them. See [Agent intent capture](../guides/agent-intent-capture.md).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `intent_groups` | `mode: ComparisonMode` | `IntentGroup[]` | The cards for the whole working tree. Recomputed on every call rather than cached: a stale group would offer to stage lines that have moved |
| `stage_intent_group` | `group: String` | `usize` | Stage every line in one card; returns how many files changed. Takes the group **id**, not its lines — indices are only valid for one comparison mode, and staging uses a different one from the view, so they are re-derived here |
| `revert_intent_group` | `group: String, mode: ComparisonMode` | `usize` | Revert every line in one card, in the displayed mode |
| `intent_capture_status` | – | `ProviderStatus[]` | Per agent: detected, where hooks are installed, how many past sessions match this workspace, and anything blocking capture |
| `intent_install_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact final contents of every file an install would write. **Touches nothing** — this is what the confirmation dialog renders |
| `enable_intent_capture` | `provider: ProviderId, scope: InstallScope` | `ProviderStatus[]` | Perform a confirmed install. Additive: existing hooks are preserved and the file is backed up first |
| `import_intent_history` | – | `usize` | Read what the agents already recorded, with no setup; returns the total record count afterwards |
| `clear_intent_history` | – | `()` | Forget everything recorded for this workspace |

## Object inspection

`src-tauri/src/commands/inspect.rs` — reading the objects out of a running .NET process or a crash dump, via the `cb-inspector` sidecar.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `inspect_status` | – | `InspectStatus` | Whether a sidecar is installed (and if not, how to install it), whether this workspace opted into crash dumps, the dumps on disk newest first, and the caveats to show before any of it is relied on |
| `inspect_attachable` | – | `AttachableList` | Enumerated by `cb-inspector --list-processes`, then attributed in `cb-core`. Every .NET process on the machine that has published a diagnostics channel, each carrying `attribution` (`launched` \| `descendant` \| `unrelated`), a `configId`/`configName` **only** for the first two, and `isApplication` — whether there is evidence this is the process holding the user's objects. An empty `processes` is a normal answer; a **rejection** means the list could not be read, which is a different statement and is shown as one; `warnings` carries a list that came back degraded, such as a host that would not report any process's parent. Only supervisor ids that *are* a configuration's id qualify as ours, so a build (`<id>:build`), a git fetch (`git:network`) and the inspector's own sidecar (`inspect:<session>`) — and anything they started — never appear. `launcherCaveat` is set when the pid is a launcher rather than the application: `dotnet run` starts the app as a child process, so its pid is the .NET CLI and a capture of it contains none of the user's objects |
| `inspect_run_dump` | `pid: number \| null, startedAt: number` | `RunDump \| null` | The dump a finished run may have written, and whether it is *certainly* that run's. Only a matching pid is evidence; anything else comes back `certain: false` and must be described as a candidate, because the dump environment is inherited by every child process and applies to every other configuration running at the same time |
| `inspect_capture` | `target: InspectTarget, root: RootSpec, widen: ElidedReason \| null, channel: Channel<ProcessEvent>` | `InspectGraph` | One capture. Streams the sidecar's output to the console, then parses `result.json`. A reported bitness mismatch is retried once with the x86 build. Registered with the supervisor as `inspect:<session>`, so cancelling works. A combination that cannot work — a crash exception asked of a live process — is refused before anything is spawned, because a live attach copies the target's memory image. A `Live` pid is re-checked immediately before spawning against a **fresh enumeration of the machine's .NET processes** — not against the supervisor, which never knew the `dotnet run` child in the first place. The picker is refreshed on demand, so a pid chosen earlier may belong to a process that has exited and had its number reused. An enumeration that *failed* is refused with its own reason rather than being read as an empty list |
| `inspect_last` | – | `InspectGraph \| null` | The last capture, so switching views does not re-read a process that may have moved on |
| `inspect_clear` | – | `()` | Drop the held capture. A graph is a copy of somebody's process memory, so "close it" actually releases it |

Expanding a node past a cap is `inspect_capture` with `RootSpec::Address` and `widen` set to the cap that stopped the previous read — the same operation, under limits raised by `Caps::widened` so the re-read can actually get past that cap. Re-reading with the caps that produced the elision returns the identical truncation, which is why `widen` is not optional in practice. The result carries a new `snapshotId`; the UI treats a differing snapshot as a staleness warning only for a live target, since a dump on disk cannot change between two reads. There is deliberately no separate expand command.

## Streaming commands

`start_run`, `run_tests`, `git_network`, and `inspect_capture` take a `Channel<ProcessEvent>` and push output events (stdout/stderr chunks, exit) while their promise stays pending. The `api.ts` wrappers hide the channel behind an `onEvent` callback.

Related: [Tauri shell](../architecture/tauri-shell.md) · [adding a command end-to-end](../guides/development.md#adding-a-tauri-command-end-to-end).
