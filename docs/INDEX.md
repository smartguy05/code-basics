# Code index

> **Generated** by [`scripts/generate-index.mjs`](../scripts/generate-index.mjs) — do not edit by hand.
> Regenerate with `pnpm docs:index` after adding files, commands, or public APIs.

Use this file to locate things fast: every first-party source file with its one-line purpose, the full Tauri command surface, the frontend IPC wrappers, and the public API of each `cb-core` module.

## Source files

| File | Lines | Purpose |
|------|------:|---------|
| `crates/core/Cargo.toml` | 30 |  |
| `crates/core/build.rs` | 9 |  |
| `crates/core/src/adapters/dotnet.rs` | 937 | The .NET ecosystem adapter. |
| `crates/core/src/adapters/dotnet_tests.rs` | 891 | Tests for the .NET adapter. |
| `crates/core/src/adapters/manifest.rs` | 478 | Declarative adapters: adding an ecosystem without writing Rust. |
| `crates/core/src/adapters/mod.rs` | 23 | Ecosystem adapters. |
| `crates/core/src/adapters/msbuild.rs` | 154 | Optional, accurate MSBuild evaluation. |
| `crates/core/src/adapters/msbuild_tests.rs` | 194 | Tests for optional MSBuild evaluation. |
| `crates/core/src/adapters/node.rs` | 339 | The JavaScript / TypeScript ecosystem adapter. |
| `crates/core/src/adapters/node_tests.rs` | 299 | Tests for the JS/TS adapter. |
| `crates/core/src/adapters/solution.rs` | 312 | Reading .NET solution files. |
| `crates/core/src/adapters/solution_tests.rs` | 129 | Tests for solution parsing. Included by `solution.rs` under `#[cfg(test)]`. |
| `crates/core/src/changelists.rs` | 204 | Change groups: named buckets for the files in a working tree. |
| `crates/core/src/changelists_tests.rs` | 200 | Tests for change groups. Included by `changelists.rs` under `#[cfg(test)]`. |
| `crates/core/src/config.rs` | 444 | The workspace configuration file, `.code-basics/config.json`. |
| `crates/core/src/files.rs` | 218 | Workspace file access for the directory tree and file editor. |
| `crates/core/src/git/attribution.rs` | 792 | Deciding which recorded edit produced which line of a diff. |
| `crates/core/src/git/attribution_tests.rs` | 951 | Tests for attributing diff lines to recorded edits. |
| `crates/core/src/git/grouping.rs` | 457 | Turning hunks into a handful of decisions. |
| `crates/core/src/git/grouping_tests.rs` | 609 | Tests for collapsing hunks into cards. |
| `crates/core/src/git/mod.rs` | 29 | Git operations. |
| `crates/core/src/git/patch.rs` | 457 | Building unified diff patches restricted to a selection of lines. |
| `crates/core/src/git/repo.rs` | 1306 | Repository reads and mutations. |
| `crates/core/src/importers/mod.rs` | 7 | Importing configurations from other tools. |
| `crates/core/src/importers/rider.rs` | 501 | Importing JetBrains Rider run configurations. |
| `crates/core/src/importers/rider_tests.rs` | 573 | Tests for the Rider importer. |
| `crates/core/src/intents/hook.rs` | 356 | Turning a hook payload into a record. |
| `crates/core/src/intents/hook_tests.rs` | 394 | Tests for ingesting hook payloads. Included by `hook.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/intents_tests.rs` | 284 | Tests for recorded agent intent. Included by `mod.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/mod.rs` | 375 | What a coding agent said it was doing, and where it wrote it down. |
| `crates/core/src/intents/patchfmt.rs` | 224 | Reading Codex's patch format. |
| `crates/core/src/intents/patchfmt_tests.rs` | 225 | Tests for Codex patch parsing. Included by `patchfmt.rs` under `#[cfg(test)]`. |
| `crates/core/src/intents/providers/claude_code.rs` | 442 | Claude Code: hooks in `settings.json`, history in per-project transcripts. |
| `crates/core/src/intents/providers/codex.rs` | 491 | Codex: hooks in `hooks.json`, history in dated rollout files. |
| `crates/core/src/intents/providers/hooks_json.rs` | 219 | Merging our hooks into a configuration file the user already owns. |
| `crates/core/src/intents/providers/instructions.rs` | 98 | Asking the agent for a reason. |
| `crates/core/src/intents/providers/instructions_tests.rs` | 130 | Tests for the label request appended to an agent's instruction file. |
| `crates/core/src/intents/providers/mod.rs` | 213 | Per-agent knowledge: where it keeps its history, and how to ask it to |
| `crates/core/src/intents/providers/providers_tests.rs` | 497 | Tests for provider detection and hook installation. |
| `crates/core/src/lib.rs` | 25 | Core logic for `code-basics`. |
| `crates/core/src/model.rs` | 444 | Types shared between the Rust core and the TypeScript frontend. |
| `crates/core/src/process/chunker.rs` | 185 | Incremental UTF-8 decoding for streamed process output. |
| `crates/core/src/process/kill.rs` | 87 | Platform-specific process *tree* termination. |
| `crates/core/src/process/mod.rs` | 458 | Process supervision: spawn, stream, cancel. |
| `crates/core/src/secrets.rs` | 390 | .NET user secrets: per-project secrets stored *outside* the repository. |
| `crates/core/src/testing/jest_like.rs` | 317 | Parser for the JSON report shared by Jest and Vitest. |
| `crates/core/src/testing/junit.rs` | 331 | Parser for JUnit-style XML test reports. |
| `crates/core/src/testing/mod.rs` | 103 | Test report parsing and result shaping. |
| `crates/core/src/testing/tree.rs` | 265 | Turning a flat list of test cases into the hierarchy the UI renders. |
| `crates/core/src/testing/trx.rs` | 541 | Parser for Visual Studio `.trx` test reports. |
| `crates/core/src/workspace.rs` | 949 | Scanning a workspace for projects and building the configurations that can |
| `crates/core/tests/git_operations.rs` | 846 | End-to-end git tests against real repositories on disk. |
| `crates/core/tests/intent_attribution.rs` | 187 | Attribution measured against a real repository, rather than a fixture. |
| `src/App.tsx` | 197 |  |
| `src/components/BranchMenu.tsx` | 423 |  |
| `src/components/ConfigEditor.tsx` | 359 |  |
| `src/components/DiffView.tsx` | 337 |  |
| `src/components/EnvironmentPicker.tsx` | 107 |  |
| `src/components/ErrorBoundary.tsx` | 65 |  |
| `src/components/FileEditor.tsx` | 112 |  |
| `src/components/FileTree.tsx` | 118 |  |
| `src/components/IntentPanel.tsx` | 364 |  |
| `src/components/OutputConsole.tsx` | 513 |  |
| `src/components/RiderImportDialog.tsx` | 135 |  |
| `src/components/RunConfigMenu.tsx` | 172 |  |
| `src/components/SecretsEditor.tsx` | 99 |  |
| `src/components/Sidebar.tsx` | 50 |  |
| `src/components/TestTree.tsx` | 150 |  |
| `src/components/language.ts` | 113 |  |
| `src/ipc/api.ts` | 294 | Typed wrappers over the Tauri command surface. |
| `src/ipc/types.ts` | 417 |  |
| `src/main.tsx` | 24 |  |
| `src/views/ChangesView.tsx` | 747 |  |
| `src/views/HistoryView.tsx` | 234 |  |
| `src/views/RunView.tsx` | 723 |  |
| `src/views/TestsView.tsx` | 402 |  |
| `src-tauri/src/commands/changelists.rs` | 57 | Change-group commands. |
| `src-tauri/src/commands/files.rs` | 36 | Workspace file commands, for the Run tab's directory tree and file editor. |
| `src-tauri/src/commands/git.rs` | 285 | Git commands. |
| `src-tauri/src/commands/intents.rs` | 209 | Agent-intent commands. |
| `src-tauri/src/commands/run.rs` | 311 | Running applications and tests. |
| `src-tauri/src/commands/secrets.rs` | 49 | .NET user secrets commands. |
| `src-tauri/src/commands/workspace.rs` | 174 | Workspace and configuration commands. |
| `src-tauri/src/invocation.rs` | 166 | Turning a run configuration into a command line. |
| `src-tauri/src/lib.rs` | 128 | The Tauri shell. |
| `src-tauri/src/main.rs` | 6 | Suppress the extra console window on Windows in release builds. |
| `src-tauri/src/recorder.rs` | 75 | The `record-intent` mode, which is what the installed hooks actually run. |
| `src-tauri/src/state.rs` | 52 | Shared application state. |
| `scripts/check-docs.mjs` | 66 |  |
| `scripts/generate-index.mjs` | 162 |  |
| `examples/adapters/cargo-nextest.toml` | 34 | Rust tests via cargo-nextest, which can emit JUnit XML. |
| `examples/adapters/pytest.toml` | 35 | A worked example of a declarative adapter. |

## Tauri command surface

Registered in `src-tauri/src/lib.rs`; documented with parameters in [reference/commands.md](reference/commands.md).

- **workspace** (`src-tauri/src/commands/workspace.rs`): `open_workspace`, `current_workspace`, `rescan_workspace`, `save_config`, `delete_config`, `preview_rider_import`, `apply_rider_import`, `launch_profiles`, `set_favorite`, `set_config_order`
- **files** (`src-tauri/src/commands/files.rs`): `fs_list_dir`, `fs_read_file`, `fs_write_file`
- **secrets** (`src-tauri/src/commands/secrets.rs`): `read_project_secrets`, `write_project_secrets`
- **run** (`src-tauri/src/commands/run.rs`): `start_run`, `build_project`, `cancel_run`, `running_ids`, `run_tests`, `last_test_run`
- **git** (`src-tauri/src/commands/git.rs`): `git_status`, `git_file_diff`, `git_file_contents`, `git_write_file`, `git_stage_file`, `git_unstage_file`, `git_stage_lines`, `git_unstage_lines`, `git_revert_lines`, `git_discard_file`, `git_commit`, `git_branches`, `git_create_branch`, `git_checkout_branch`, `git_checkout_remote_branch`, `git_delete_branch`, `git_merge_branch`, `git_abort_merge`, `git_history`, `git_commit_diff`, `git_stash_save`, `git_stash_pop`, `git_network`
- **changelists** (`src-tauri/src/commands/changelists.rs`): `git_changelists`, `git_create_changelist`, `git_delete_changelist`, `git_rename_changelist`, `git_assign_to_changelist`
- **intents** (`src-tauri/src/commands/intents.rs`): `intent_groups`, `stage_intent_group`, `revert_intent_group`, `intent_capture_status`, `intent_install_plan`, `enable_intent_capture`, `import_intent_history`, `clear_intent_history`

## Frontend IPC wrappers (`src/ipc/api.ts`)

`openWorkspace`, `currentWorkspace`, `rescanWorkspace`, `saveConfig`, `deleteConfig`, `launchProfiles`, `setFavorite`, `setConfigOrder`, `readProjectSecrets`, `writeProjectSecrets`, `previewRiderImport`, `applyRiderImport`, `fsListDir`, `fsReadFile`, `fsWriteFile`, `startRun`, `buildProject`, `cancelRun`, `runningIds`, `runTests`, `lastTestRun`, `gitStatus`, `gitFileDiff`, `gitFileContents`, `gitWriteFile`, `gitStageFile`, `gitUnstageFile`, `gitStageLines`, `gitUnstageLines`, `gitRevertLines`, `gitDiscardFile`, `gitCommit`, `gitBranches`, `gitCreateBranch`, `gitCheckoutBranch`, `gitCheckoutRemoteBranch`, `gitDeleteBranch`, `gitMergeBranch`, `gitAbortMerge`, `gitChangelists`, `gitCreateChangelist`, `gitDeleteChangelist`, `gitRenameChangelist`, `gitAssignToChangelist`, `gitHistory`, `gitCommitDiff`, `gitStashSave`, `gitStashPop`, `gitNetwork`, `intentGroups`, `stageIntentGroup`, `revertIntentGroup`, `intentCaptureStatus`, `intentInstallPlan`, `enableIntentCapture`, `importIntentHistory`, `clearIntentHistory`, `errorMessage`

## Public core API (`cb-core`)

- `crates/core/src/adapters/dotnet.rs`: `ProjectFile`, `references()`, `references_prefix()`, `parse_project_file()`, `ConfiguredRunner`, `parse_dotnet_config()`, `is_test_project()`, `classify_runner()`, `has_trx_extension()`, `project_kind()`, `configurations()`, `LaunchProfile`, `is_launchable()`, `parse_launch_settings()`, `split_args()`, `BuildContext`, `test_invocation()`, `run_invocation()`, `BuildAction`, `build_action_invocation()`, `configs_for_project()`
- `crates/core/src/adapters/manifest.rs`: `AdapterManifest`, `CommandTemplate`, `parse()`, `load_dir()`, `matches()`, `matched_file()`, `build_invocation()`, `configs_for_project()`, `manifest_dir()`
- `crates/core/src/adapters/msbuild.rs`: `command_args()`, `parse_output()`, `apply()`, `evaluate()`
- `crates/core/src/adapters/node.rs`: `PackageJson`, `depends_on()`, `parse_package_json()`, `PackageManager`, `program()`, `run_script_args()`, `exec_args()`, `script_arg_separator()`, `detect_package_manager()`, `detect_runner()`, `is_workspace_root()`, `project_kind()`, `test_invocation()`, `script_invocation()`, `configs_for_project()`, `project_dir()`
- `crates/core/src/adapters/solution.rs`: `Solution`, `SolutionProject`, `is_solution_file()`, `parse()`
- `crates/core/src/changelists.rs`: `Changelist`, `Changelists`, `group_of()`, `changelists_path()`, `load()`, `save()`, `create()`, `remove()`, `rename()`, `assign()`
- `crates/core/src/config.rs`: `WorkspaceConfig`, `config_dir()`, `config_path()`, `results_dir()`, `load()`, `ensure_gitignore()`, `save()`, `merge()`, `apply()`, `sort_configs()`, `set_favorite()`, `set_order()`, `upsert()`, `remove()`
- `crates/core/src/files.rs`: `DirEntry`, `list_dir()`, `read_file()`, `write_file()`
- `crates/core/src/git/attribution.rs`: `MatchLevel`, `Confidence`, `AttributedSpan`, `HunkAttribution`, `FileAttribution`, `is_empty()`, `Options`, `attribute_file()`, `attribute()`
- `crates/core/src/git/grouping.rs`: `GroupKind`, `GroupFile`, `IntentGroup`, `hunk_count()`, `is_formatting_only()`, `enclosing_symbol()`, `group()`
- `crates/core/src/git/patch.rs`: `LineOrigin`, `DiffLine`, `Hunk`, `FileDiff`, `changed_line_indices()`, `hunk_line_indices()`, `Direction`, `build_patch()`
- `crates/core/src/git/repo.rs`: `ComparisonMode`, `ChangeKind`, `FileChange`, `is_conflicted()`, `WorkingStatus`, `Branch`, `Commit`, `MergeOutcome`, `MergeReport`, `StageTarget`, `Repo`, `open()`, `workdir()`, `status()`, `file_diff()`, `diff_all()`, `baseline_content()`, `working_content()`, `write_working_file()`, `stage_file()`, `unstage_file()`, `stage_lines()`, `unstage_lines()`, `revert_lines()`, `discard_file()`, `commit()`, `merge_branch()`, `abort_merge()`, `branches()`, `create_branch()`, `checkout_branch()`, `create_branch_from()`, `checkout_remote_branch()`, `delete_branch()`, `history()`, `commit_diff()`, `stash_save()`, `stash_pop()`, `network_command()`, `NetworkOperation`
- `crates/core/src/importers/rider.rs`: `RiderConfiguration`, `parse()`, `expand_macros()`, `convert()`, `resolve_compounds()`, `ImportResult`, `import()`
- `crates/core/src/intents/hook.rs`: `HookEvent`, `parse()`, `ingest()`, `parse_labels()`, `is_enabled()`, `resolve_root()`
- `crates/core/src/intents/mod.rs`: `ProviderId`, `as_str()`, `IntentEdit`, `is_empty()`, `IntentRecord`, `IntentLabel`, `Intents`, `is_empty()`, `label_for()`, `for_path()`, `normalise_path()`, `relative_to()`, `intents_dir()`, `edits_path()`, `labels_path()`, `LoadOptions`, `load()`, `append_edit()`, `append_label()`, `next_seq()`, `clear()`
- `crates/core/src/intents/patchfmt.rs`: `PatchedFile`, `parse_envelope()`, `parse_unified_diff()`, `envelope_from_value()`
- `crates/core/src/intents/providers/claude_code.rs`: `ClaudeCode`, `new()`, `encode_project_dir()`
- `crates/core/src/intents/providers/codex.rs`: `Codex`, `new()`, `codex_home()`, `planned_entries()`
- `crates/core/src/intents/providers/hooks_json.rs`: `is_installed()`, `commands_for()`, `plan_merge()`, `plan_removal()`
- `crates/core/src/intents/providers/instructions.rs`: `path_for()`, `is_present()`, `planned_write()`
- `crates/core/src/intents/providers/mod.rs`: `InstallScope`, `ProviderStatus`, `absent()`, `PlannedWrite`, `InstallPlan`, `SessionFile`, `apply_plan()`, `all()`, `statuses()`, `history()`, `home_dir()`
- `crates/core/src/model.rs`: `Project`, `ProjectKind`, `TestRunner`, `RunKind`, `ConfigSource`, `RunConfig`, `new()`, `ReportSpec`, `ReportFormat`, `Invocation`, `TestOutcome`, `TestCase`, `TestSummary`, `from_cases()`, `TestRunResult`, `TestNode`
- `crates/core/src/process/chunker.rs`: `Utf8Chunker`, `new()`, `push()`, `finish()`, `LineSplitter`, `new()`, `push()`, `finish()`
- `crates/core/src/process/kill.rs`: `configure_process_group()`, `kill_tree()`
- `crates/core/src/process/mod.rs`: `Stream`, `ProcessEvent`, `Supervisor`, `new()`, `run()`, `cancel()`, `running_ids()`, `is_running()`
- `crates/core/src/secrets.rs`: `ProjectSecrets`, `secrets_path()`, `user_secrets_id()`, `read()`, `ensure_id()`, `write()`
- `crates/core/src/testing/jest_like.rs`: `parse()`
- `crates/core/src/testing/junit.rs`: `parse()`
- `crates/core/src/testing/mod.rs`: `parse()`, `parse_file()`
- `crates/core/src/testing/tree.rs`: `build()`, `failed_names()`
- `crates/core/src/testing/trx.rs`: `parse()`
- `crates/core/src/workspace.rs`: `Workspace`, `should_skip()`, `launch_profiles()`, `ScanOptions`, `scan()`, `scan_with()`, `find_project()`, `dotnet_test_context()`, `configs_by_project()`
