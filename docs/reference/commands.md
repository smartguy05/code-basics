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

## Enhancements (instructions + prompts)

`src-tauri/src/commands/enhancements.rs` — the menu-bar **Enhancements** menu, with two file-driven submenus. Both read plain `.md` files (front matter: `id`, `title`, and for instructions `placement`) from user-owned directories, auto-generated from whatever files are present; bundled defaults are seeded on first use without overwriting edits.

**Instructions** live in `%APPDATA%\code-basics\instructions\` (or `$XDG_CONFIG_HOME`/`~/.config` elsewhere; `CB_INSTRUCTIONS_PATH` overrides). Adding (after an inline confirmation) writes the section into **both** `CLAUDE.md` and `AGENTS.md`, bounded by an `<!-- code-basics: enhancement:<id> -->` marker so it is idempotent, refreshable and removable.

**Prompts** live in the sibling `prompts/` directory (`CB_PROMPTS_PATH` overrides). Nothing is written — the command returns each prompt's body and the frontend copies it to the clipboard.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `list_enhancements` | — | `EnhancementInfo[]` | Every instruction template on disk, each flagged `installed` when its section is present in either agent file |
| `add_enhancement` | `id: String` | `EnhancementInfo[]` | Splice the template's section into both agent files at its declared placement (backing up the originals); returns the refreshed list |
| `remove_enhancement` | `id: String` | `EnhancementInfo[]` | Cut the template's marked section out of both agent files; returns the refreshed list |
| `list_prompts` | — | `PromptInfo[]` | Every prompt on disk, each carrying the `body` the frontend copies to the clipboard |

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

## Adversarial review

`src-tauri/src/commands/review.rs` — runs a coding-agent CLI (Claude Code or Codex) read-only against the open workspace and streams its output. Every decision (which agents exist, allowed models, argument order) lives in `cb_core::review`.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `review_agents` | – | `ReviewAgentInfo[]` | The agents whose CLI is installed (`claude`/`codex`), preference order, each with its offered model aliases (empty ⇒ the agent's own default) |
| `start_review` | `prompt_id: String`, `agent_id: String`, `model: String?`, `channel: Channel<ProcessEvent>` | `()` | Runs a chosen prompt from the Prompts library, read-only: Claude via `claude -p … --permission-mode plan`, Codex via `codex exec --sandbox read-only …`. Registered as `review:current`; an unknown model is refused; a missing CLI surfaces as a `Failed` event |
| `cancel_review` | – | `bool` | Kills the review process **tree** |

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
| `git_commit` | `message: String`, `amend: bool` | `String` | Returns the new commit id. Also persists the change's content-keyed intent into a git note (`refs/notes/code-basics-intents`), best-effort — a note failure never fails the commit |
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
| `git_commit_file_contents` | `id: String, path: String` | `FileContents` | Both sides of one file as a commit changed it, for the History diff viewer. Either side is null when the file did not exist there (added, deleted, or a root commit) |
| `git_commit_file_why` | `id: String, path: String` | `LineIntent[]` | The recorded reason behind each line of a file as a past commit left it, resolved from the durable git note. Content-keyed, so it survives reformatting/rebase; empty when the commit has no note or no line matches (never a guessed reason) |
| `git_stash_save` | `message: String` | `()` | Stash the working tree (including untracked) under a message |
| `git_stash_list` | – | `StashEntry[]` | Every stash, newest first; `id` is the stash commit for previewing via `git_commit_diff` |
| `git_stash_pop` | `index: usize` | `()` | Apply `stash@{index}` and remove it |
| `git_stash_apply` | `index: usize` | `()` | Apply `stash@{index}`, keeping it in the list |
| `git_stash_drop` | `index: usize` | `()` | Remove `stash@{index}` without applying it |
| `git_stash_clear` | – | `()` | Drop every stash |
| `git_network` | `kind: NetworkKind`, `channel: Channel<ProcessEvent>` | `i32 \| null` | Push/pull/fetch via system `git`; returns the exit code |

## Agent intent

`src-tauri/src/commands/intents.rs` — collapsing hunks into the decisions behind them. See [Agent intent capture](../guides/agent-intent-capture.md).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `intent_groups` | `mode: ComparisonMode` | `IntentReview` | The cards for the whole working tree, plus the coverage audit: unexplained hunks (sorted to the top of `groups`), unfulfilled claims (declared intents no hunk evidences), and the per-turn `scorecard`. Recomputed on every call rather than cached |
| `stage_intent_group` | `group: String, path: Option<String>` | `usize` | Stage every line in one card; returns how many files changed. Takes the group **id**, not its lines — indices are only valid for one comparison mode, and staging uses a different one from the view, so they are re-derived here |
| `revert_intent_group` | `group: String, mode: ComparisonMode, path: Option<String>` | `usize` | Revert every line in one card, in the displayed mode; `path` limits either command to that file's share of the card |
| `reject_intent_group` | `group: String, mode: ComparisonMode, path: Option<String>, reason: String` | `RejectSummary` | Revert one card **and** leave the reason as a marker comment where the code was. Refused in `indexToHead` — a revert there changes the index, so the note would explain something the reviewer is not looking at — and refused without a reason. `unmarked` names files reverted without a note for want of line-comment syntax |
| `intent_capture_status` | – | `ProviderStatus[]` | Per agent: detected, where hooks are installed, how many past sessions match this workspace, and anything blocking capture |
| `intent_install_plan` | `provider: ProviderId, scope: InstallScope` | `InstallPlan` | The exact final contents of every file an install would write. **Touches nothing** — this is what the confirmation dialog renders |
| `enable_intent_capture` | `provider: ProviderId, scope: InstallScope` | `ProviderStatus[]` | Perform a confirmed install. Additive: existing hooks are preserved and the file is backed up first. Also installs the `pre-commit` guard and the durable-why `post-commit` hook, and makes both executable |
| `import_intent_history` | – | `usize` | Read what the agents already recorded, with no setup: edits, coarse labels, and the user prompts mined from session transcripts (keyed to the same turn id so they join). Returns the total record count afterwards |
| `clear_intent_history` | – | `()` | Forget everything recorded for this workspace |

## Behavioral before/after testing

`src-tauri/src/commands/behavioral.rs` — the runtime counterpart to the intent coverage audit: run the same config against `HEAD` and the working tree and diff the observable outcomes. Decisions live in `cb_core::behavioral`.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `behavioral_diff` | `configId: String, httpFiles: Option<Vec<String>>, channel: Channel<ProcessEvent>` | `BehavioralReport` | Build `HEAD` in an isolated `git worktree` and the working tree in place, run the config on both under distinct `:base`/`:work` supervisor ids (streaming both sides' output onto `channel`), then diff test results and console output and attribute each delta to the intent card whose files it points at. Every failure — a bad baseline checkout, a config absent at `HEAD`, a server that never became ready — is an abstain recorded in `warnings`, never an error. When `.http` scenarios (with an `@readiness` probe) and an `App` launch config are present it also replays those requests against a server started on each side — strictly sequential (base then work, same port) and never hanging on a server that will not exit — and diffs the responses; otherwise HTTP abstains with a warning |
| `behavioral_clear` | – | `String[]` | Remove the cached baseline checkouts under `.code-basics/behavioral/`; returns any teardown residue as warnings |

## Erosion detector

`src-tauri/src/commands/erosion.rs` — a rules-based, no-model scan over the diff for changes that quietly weaken the codebase (deleted assertions, skipped tests, widened catches, introduced panics, stubs left in production paths, removed safeguards and logs).

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `erosion_scan` | `mode: ComparisonMode` | `ErosionReport` | Every flag found across the working tree, plus `warnings` for any rule whose TOML would not parse or whose regex would not compile. Recomputed on every call |

Each rule is one regex against one **side** of the diff. The built-in set ships per ecosystem (.NET / TS-JS / Rust) and is **extended, never shadowed** by per-workspace TOML in `.code-basics/erosion/*.toml`:

```toml
[[rule]]
id = "no-fire-and-forget-task"
category = "widenedCatch"   # deletedAssertion | ignoredTest | widenedCatch | removedNullCheck | unsafeCast | leftoverStub | removedSafeguard | droppedLog
side = "added"              # added (things introduced) | removed (things taken away)
pattern = 'Task\.Run\('     # regex matched against a changed line's content; a context line is never matched
message = "Fire-and-forget Task.Run swallows failures."
extensions = [".cs"]        # optional; empty = every file
prodOnly = false            # optional; skip files that look like tests
```

See `examples/erosion/custom.toml` for a copyable starting point.

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

## Symbol palette

`src-tauri/src/commands/symbols.rs` — searching the workspace by name, over the index `cb_core::symbols` builds.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `search_everywhere` | `query: string, scope: SearchScope, limit: number \| null` | `SearchHit[]` | Files, symbols and run configurations ranked into one list. `query` is passed through verbatim — a trailing `:123` means "line 123" and is parsed in `cb-core`, so nothing else may re-implement it. `limit` defaults to 50 and bounds the ranking, not just the output. **Never errors for a missing index**: before the background build finishes the answer is an empty list, because "still indexing" is a normal state and not a failure |
| `symbol_index_status` | – | `SymbolIndexStatus` | `ready` (there is an index to search) and `building` (one is being built) are independent — a rebuild runs over a usable index — plus the file and symbol counts and whether a cap clipped them |
| `rebuild_symbol_index` | – | `()` | Discards `.code-basics/symbols.json` and re-reads the workspace from source, in the background. Returns as soon as the build starts; watch `symbol_index_status` for the finish. The old index stays in place until the new one lands |

The index is built on a background thread by `open_workspace` and `rescan_workspace`, and by the app's `setup` hook for a workspace named on the command line. Nothing waits for it: a cold build is ~20 ms on this repository but 637 ms on a 2,864-file .NET solution and 9.4 s against a cold filesystem cache. A build whose root is no longer the open workspace is discarded rather than stored (`AppState::record_symbols`), so opening A then B cannot serve A's paths under B. `fs_write_file` and `git_write_file` re-index the single file they wrote, which is microseconds and is what keeps the palette from going stale on every edit.

## Language servers

`src-tauri/src/commands/lsp.rs` — real "find usages", go-to-definition and inline usage counts, over LSP. A different question from the symbol palette above and **not** a fallback for it: the palette answers "what does this workspace declare?" with a text heuristic, which cannot tell `Order.Total` from `Invoice.Total`, and a usage count is a much stronger claim than a palette row.

Every one of these returns an `outcome: Availability` — `notConfigured`, `starting`, `loading`, `ready`, `failed`, `unsupported` — and **only `ready` licenses a count**. A timeout, a dead server, a server still loading its projects and a genuine zero are four different answers and never collapse into "0 usages"; the reason arrives in `message`, not as an error and not as an empty list.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `lsp_status` | – | `LspStatus` | What every configured server is doing, with `lookedFor` (everywhere that was searched) and `hint` (what to do about it). **Never errors**: no workspace and no session both give an empty `servers`, because "no server, and here is why" is an answer and this is where the user reads it. A language that resolved but has not been asked for anything yet has **no row at all** — there is no `Availability` for "found, idle", and `starting` said about an unspawned process would be a small lie |
| `lsp_open_document` | `path: String`, `text: String` | `()` | The editor opened a buffer, or replaced it wholesale. Enqueued, not confirmed — a `didOpen` has no reply. A file opened before its server is up is replayed to it when it comes up, with the *edited* text |
| `lsp_change_document` | `path: String`, `text: String` | `()` | Full-text sync. Send this before asking anything about the buffer: notifications and requests travel the same stdin stream in order, so enqueueing the change *is* the flush, and a request aimed at a buffer the server believes is two edits old answers confidently about a different symbol |
| `lsp_close_document` | `path: String` | `()` | The servers go back to reading disk |
| `lsp_find_usages` | `path: String`, `line: u32`, `character: u32` | `UsageResult` | Every use site of the symbol at that position, `includeDeclaration: false` — the inline row is drawn *on* the declaration, so counting it would report "1 usage" for a symbol nothing uses. `total` is the true count and is **not** capped even when `usages` is (500 rows); `truncated` says so. A row whose location is outside the workspace or in a `source-generated:`/metadata URI keeps `path: null` — still listed and still counted, just not openable |
| `lsp_goto_definition` | `path: String`, `line: u32`, `character: u32` | `DefinitionResult` | `declarations`, `implementations` and `typeDefinitions` as three lists, asked concurrently. An empty list plus a `message` is "nobody could be asked"; an empty list with `outcome: ready` and no note about that group is "there are none" |
| `lsp_declaration_anchors` | `path: String` | `AnchorResult` | Which declarations in one file deserve an inline "N usages" row. Each anchor's `character` aims at the **identifier**, not the start of the declaration, and `selectionLine` is the identifier's own line — attributes and doc comments push it below `line`. `id` is stable within a file so a widget can be keyed on it |

`line` is **1-based** in both directions, matching the editor gutter, `SymbolIndex::line` and the existing open-a-file-at-a-line chain. `character` is **0-based UTF-16 code units** in both directions, because that is what CodeMirror hands over and there is no 1-based column convention anywhere in this app. `Highlight.start`/`end` are UTF-16 offsets into `snippet` for the same reason. The asymmetry is deliberate; `cb_core::lsp::positions` is the only place either conversion happens.

Sessions are per workspace and are started by `open_workspace`, `rescan_workspace`, the app's `setup` hook (for `code-basics .`), and lazily by the first of these commands to arrive — whichever happens first, since a request can precede all three. Starting a *session* resolves where each language's server is and starts **no** process; a server is spawned by the first document or request for its language, and the request that triggers it returns `starting` immediately rather than waiting out Roslyn's project load. Opening a different workspace tears the session down inside `AppState`'s own guard (`AppState::record_lsp_session` hands a rejected session **back** rather than returning `false`, because a rejected session is a running process tree and a `bool` would leak it); a rescan keeps it, so an edit to the `lsp` block of `.code-basics/config.json` takes effect when the workspace is next opened rather than on the save.

## Architecture diagrams

`src-tauri/src/commands/architecture.rs` — the derived project graph, the derived component map, and the diagrams stored beside them.

| Command | Parameters | Returns | Notes |
|---------|-----------|---------|-------|
| `arch_project_graph` | – | `ArchGraph` | Nodes, edges, and the `warnings` naming every reference that could **not** be turned into an edge. Derived from `<ProjectReference>` items, `package.json` dependency names that match another project in this workspace, and `.sln`/npm-workspace grouping (`contains`, which is membership and never dependency). Recomputed on every call from the manifests as they are now — nothing is cached, because a cached arrow would assert a dependency the user has since deleted. A third-party npm package yields nothing at all: no edge, no node, no warning |
| `arch_render_graph` | – | `String` | The same graph as Mermaid `flowchart` source, deterministic byte-for-byte. Renders only — it does not store the result, so opening the tab cannot leave a regenerated file under the user's `.code-basics/` |
| `arch_component_graph` | – | `ArchGraph` | The **component map**, a different question from `arch_project_graph`: the services this workspace runs and the data stores they declare, inferred from manifests, configuration and filenames rather than read out of `<ProjectReference>` items. Only a HIGH signal — something the author wrote down — may create a node or an edge; a MEDIUM one may only enrich what a HIGH one created, and everything else lands in `warnings`. A workspace with no HIGH signals yields an **empty** map, never a fallback to the project map. Never errors for a missing symbol index: an index that has not finished building costs ASP.NET route *details* and nothing else, so the map is smaller, and a `warnings` entry says so |
| `arch_render_component_graph` | – | `String` | The component map as Mermaid `flowchart` source, through the same renderer — the component map is an ordinary `ArchGraph`. A separate command rather than a parameter on `arch_render_graph`. Warnings do not survive rendering, so a view that draws this should call `arch_component_graph` too |
| `arch_list_diagrams` | – | `DiagramFile[]` | Every stored diagram: committed ones from `.code-basics/diagrams/`, then the regenerated ones from `.code-basics/diagrams/derived/` (gitignored), each group alphabetical. The order is part of the contract so a list cannot reshuffle under the cursor. A file whose front matter cannot be read is still listed, presented as `user`, with `warning` set |
| `arch_read_diagram` | `name: String` | `String` | One diagram exactly as it is on disk, front matter included. The committed copy wins over a regenerated one of the same name. The name is validated as a single file name — a path, a drive prefix or `..` is refused |
| `arch_write_diagram` | `name: String, contents: String` | `ValidationError \| null` | Saves a person's edit and returns the problem the saved text carries, or `null`. **The file is written either way**: Mermaid passes through invalid states on the way to every valid one, so refusing a broken save would be a save the user cannot use while still drawing, and nothing downstream trusts a stored diagram anyway — the renderer validates what it is handed. Only an `Err` means nothing was written. Provenance comes from the copy already on disk, never from the text being saved, so typing `derivation: derived` into the editor cannot pass a drawing off as a fact. Editing a derived diagram **promotes** it to `user` and moves it out of the gitignored directory, so callers must re-list rather than reuse the path they had |
| `arch_validate` | `source: String` | `ValidationError \| null` | Checks Mermaid source without storing it; `null` means it will render. Invalid source is the answer, not an error — a half-typed diagram is an ordinary editing state. The diagram-type allowlist is a CSP guard as much as a syntax check: families outside it pull renderer bundles the spike never exercised under `default-src 'self'` |

`Derivation` and `DiagramDerivation` are Rust enums with data, so serde tags them **externally**: `{"derived":{"scanner":1}}` and `{"inferred":{"agent":"claude"}}` are objects while `"user"` (and `DiagramDerivation`'s `"derived"`) is a bare string. `src/ipc/types.ts` mirrors both shapes; the pinning tests are `an_arch_graph_serialises_with_the_keys_the_ui_reads` (`architecture/graph_tests.rs`) and `a_diagram_file_serialises_with_the_keys_the_frontend_reads` (`architecture/store_tests.rs`).

## Streaming commands

`start_run`, `run_tests`, `git_network`, and `inspect_capture` take a `Channel<ProcessEvent>` and push output events (stdout/stderr chunks, exit) while their promise stays pending. The `api.ts` wrappers hide the channel behind an `onEvent` callback.

Related: [Tauri shell](../architecture/tauri-shell.md) · [adding a command end-to-end](../guides/development.md#adding-a-tauri-command-end-to-end).
