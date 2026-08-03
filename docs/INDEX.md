# Code index

> **Generated** by [`scripts/generate-index.mjs`](../scripts/generate-index.mjs) — do not edit by hand.
> Regenerate with `pnpm docs:index` after adding files, commands, or public APIs.

Use this file to locate things fast: every first-party source file with its one-line purpose, the full Tauri command surface, the frontend IPC wrappers, and the public API of each `cb-core` module.

## Source files

| File | Lines | Purpose |
|------|------:|---------|
| `crates/core/Cargo.toml` | 28 |  |
| `crates/core/src/adapters/dotnet.rs` | 674 | The .NET ecosystem adapter. |
| `crates/core/src/adapters/dotnet_tests.rs` | 639 | Tests for the .NET adapter. |
| `crates/core/src/adapters/manifest.rs` | 465 | Declarative adapters: adding an ecosystem without writing Rust. |
| `crates/core/src/adapters/mod.rs` | 13 | Ecosystem adapters. |
| `crates/core/src/adapters/node.rs` | 339 | The JavaScript / TypeScript ecosystem adapter. |
| `crates/core/src/adapters/node_tests.rs` | 299 | Tests for the JS/TS adapter. |
| `crates/core/src/config.rs` | 255 | The workspace configuration file, `.code-basics/config.json`. |
| `crates/core/src/git/mod.rs` | 25 | Git operations. |
| `crates/core/src/git/patch.rs` | 457 | Building unified diff patches restricted to a selection of lines. |
| `crates/core/src/git/repo.rs` | 861 | Repository reads and mutations. |
| `crates/core/src/importers/mod.rs` | 7 | Importing configurations from other tools. |
| `crates/core/src/importers/rider.rs` | 356 | Importing JetBrains Rider run configurations. |
| `crates/core/src/importers/rider_tests.rs` | 352 | Tests for the Rider importer. |
| `crates/core/src/lib.rs` | 21 | Core logic for `code-basics`. |
| `crates/core/src/model.rs` | 402 | Types shared between the Rust core and the TypeScript frontend. |
| `crates/core/src/process/chunker.rs` | 185 | Incremental UTF-8 decoding for streamed process output. |
| `crates/core/src/process/kill.rs` | 87 | Platform-specific process *tree* termination. |
| `crates/core/src/process/mod.rs` | 423 | Process supervision: spawn, stream, cancel. |
| `crates/core/src/testing/jest_like.rs` | 317 | Parser for the JSON report shared by Jest and Vitest. |
| `crates/core/src/testing/junit.rs` | 331 | Parser for JUnit-style XML test reports. |
| `crates/core/src/testing/mod.rs` | 103 | Test report parsing and result shaping. |
| `crates/core/src/testing/tree.rs` | 265 | Turning a flat list of test cases into the hierarchy the UI renders. |
| `crates/core/src/testing/trx.rs` | 520 | Parser for Visual Studio `.trx` test reports. |
| `crates/core/src/workspace.rs` | 589 | Scanning a workspace for projects and building the configurations that can |
| `crates/core/tests/git_operations.rs` | 615 | End-to-end git tests against real repositories on disk. |
| `src/App.tsx` | 160 |  |
| `src/components/ConfigEditor.tsx` | 216 |  |
| `src/components/DiffView.tsx` | 310 |  |
| `src/components/OutputConsole.tsx` | 110 |  |
| `src/components/RiderImportDialog.tsx` | 135 |  |
| `src/components/TestTree.tsx` | 150 |  |
| `src/ipc/api.ts` | 155 | Typed wrappers over the Tauri command surface. |
| `src/ipc/types.ts` | 222 |  |
| `src/main.tsx` | 11 |  |
| `src/views/ChangesView.tsx` | 291 |  |
| `src/views/HistoryView.tsx` | 233 |  |
| `src/views/RunView.tsx` | 212 |  |
| `src/views/TestsView.tsx` | 224 |  |
| `src-tauri/src/commands/git.rs` | 250 | Git commands. |
| `src-tauri/src/commands/run.rs` | 170 | Running applications and tests. |
| `src-tauri/src/commands/workspace.rs` | 115 | Workspace and configuration commands. |
| `src-tauri/src/invocation.rs` | 153 | Turning a run configuration into a command line. |
| `src-tauri/src/lib.rs` | 89 | The Tauri shell. |
| `src-tauri/src/main.rs` | 6 | Suppress the extra console window on Windows in release builds. |
| `src-tauri/src/state.rs` | 52 | Shared application state. |
| `scripts/check-docs.mjs` | 66 |  |
| `scripts/generate-index.mjs` | 162 |  |
| `examples/adapters/cargo-nextest.toml` | 34 | Rust tests via cargo-nextest, which can emit JUnit XML. |
| `examples/adapters/pytest.toml` | 35 | A worked example of a declarative adapter. |

## Tauri command surface

Registered in `src-tauri/src/lib.rs`; documented with parameters in [reference/commands.md](reference/commands.md).

- **workspace** (`src-tauri/src/commands/workspace.rs`): `open_workspace`, `current_workspace`, `rescan_workspace`, `save_config`, `delete_config`, `preview_rider_import`, `apply_rider_import`
- **run** (`src-tauri/src/commands/run.rs`): `start_run`, `cancel_run`, `running_ids`, `run_tests`, `last_test_run`
- **git** (`src-tauri/src/commands/git.rs`): `git_status`, `git_file_diff`, `git_file_contents`, `git_write_file`, `git_stage_file`, `git_unstage_file`, `git_stage_lines`, `git_unstage_lines`, `git_revert_lines`, `git_discard_file`, `git_commit`, `git_branches`, `git_create_branch`, `git_checkout_branch`, `git_delete_branch`, `git_history`, `git_commit_diff`, `git_stash_save`, `git_stash_pop`, `git_network`

## Frontend IPC wrappers (`src/ipc/api.ts`)

`openWorkspace`, `currentWorkspace`, `rescanWorkspace`, `saveConfig`, `deleteConfig`, `previewRiderImport`, `applyRiderImport`, `startRun`, `cancelRun`, `runningIds`, `runTests`, `lastTestRun`, `gitStatus`, `gitFileDiff`, `gitFileContents`, `gitWriteFile`, `gitStageFile`, `gitUnstageFile`, `gitStageLines`, `gitUnstageLines`, `gitRevertLines`, `gitDiscardFile`, `gitCommit`, `gitBranches`, `gitCreateBranch`, `gitCheckoutBranch`, `gitDeleteBranch`, `gitHistory`, `gitCommitDiff`, `gitStashSave`, `gitStashPop`, `gitNetwork`, `errorMessage`

## Public core API (`cb-core`)

- `crates/core/src/adapters/dotnet.rs`: `ProjectFile`, `references()`, `references_prefix()`, `parse_project_file()`, `ConfiguredRunner`, `parse_dotnet_config()`, `is_test_project()`, `classify_runner()`, `has_trx_extension()`, `project_kind()`, `LaunchProfile`, `parse_launch_settings()`, `split_args()`, `BuildContext`, `test_invocation()`, `run_invocation()`, `configs_for_project()`
- `crates/core/src/adapters/manifest.rs`: `AdapterManifest`, `CommandTemplate`, `parse()`, `load_dir()`, `matches()`, `build_invocation()`, `configs_for_project()`, `manifest_dir()`
- `crates/core/src/adapters/node.rs`: `PackageJson`, `depends_on()`, `parse_package_json()`, `PackageManager`, `program()`, `run_script_args()`, `exec_args()`, `script_arg_separator()`, `detect_package_manager()`, `detect_runner()`, `is_workspace_root()`, `project_kind()`, `test_invocation()`, `script_invocation()`, `configs_for_project()`, `project_dir()`
- `crates/core/src/config.rs`: `WorkspaceConfig`, `config_dir()`, `config_path()`, `results_dir()`, `load()`, `save()`, `merge()`, `upsert()`, `remove()`
- `crates/core/src/git/patch.rs`: `LineOrigin`, `DiffLine`, `Hunk`, `FileDiff`, `changed_line_indices()`, `hunk_line_indices()`, `Direction`, `build_patch()`
- `crates/core/src/git/repo.rs`: `ComparisonMode`, `ChangeKind`, `FileChange`, `is_conflicted()`, `WorkingStatus`, `Branch`, `Commit`, `StageTarget`, `Repo`, `open()`, `workdir()`, `status()`, `file_diff()`, `diff_all()`, `baseline_content()`, `working_content()`, `write_working_file()`, `stage_file()`, `unstage_file()`, `stage_lines()`, `unstage_lines()`, `revert_lines()`, `discard_file()`, `commit()`, `branches()`, `create_branch()`, `checkout_branch()`, `delete_branch()`, `history()`, `commit_diff()`, `stash_save()`, `stash_pop()`, `network_command()`, `NetworkOperation`
- `crates/core/src/importers/rider.rs`: `RiderConfiguration`, `parse()`, `expand_macros()`, `convert()`, `ImportResult`, `import()`
- `crates/core/src/model.rs`: `Project`, `ProjectKind`, `TestRunner`, `RunKind`, `ConfigSource`, `RunConfig`, `new()`, `ReportSpec`, `ReportFormat`, `Invocation`, `TestOutcome`, `TestCase`, `TestSummary`, `from_cases()`, `TestRunResult`, `TestNode`
- `crates/core/src/process/chunker.rs`: `Utf8Chunker`, `new()`, `push()`, `finish()`, `LineSplitter`, `new()`, `push()`, `finish()`
- `crates/core/src/process/kill.rs`: `configure_process_group()`, `kill_tree()`
- `crates/core/src/process/mod.rs`: `Stream`, `ProcessEvent`, `Supervisor`, `new()`, `run()`, `cancel()`, `running_ids()`, `is_running()`
- `crates/core/src/testing/jest_like.rs`: `parse()`
- `crates/core/src/testing/junit.rs`: `parse()`
- `crates/core/src/testing/mod.rs`: `parse()`, `parse_file()`
- `crates/core/src/testing/tree.rs`: `build()`, `failed_names()`
- `crates/core/src/testing/trx.rs`: `parse()`
- `crates/core/src/workspace.rs`: `Workspace`, `scan()`, `find_project()`, `dotnet_test_context()`, `configs_by_project()`
