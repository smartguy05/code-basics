# The core crate (`cb-core`)

`crates/core` holds every decision the application makes, with **no Tauri dependency**, so all of it runs under `cargo test -p cb-core`. The [Tauri shell](tauri-shell.md) is only a bridge to it.

Modules, in the order data flows through them:

## `workspace`

Scans an opened directory for projects (`workspace::scan`). Detection is intentionally filesystem-only — no MSBuild evaluation, no `npm ls`, no shelling out — so opening a workspace feels instant. Skips well-known output/metadata directories (`bin`, `obj`, `node_modules`, `target`, ...; `bin`/`obj` matter because they contain copies of project files that would otherwise be detected as projects) and descends at most 10 levels.

Produces a `Workspace { root, name, projects, configs }` where `configs` are the auto-detected run configurations. Saved user configurations are merged on top by the caller ([`config::merge`](#config)). Project ids are workspace-relative paths, so they stay stable across machines.

## `adapters`

Per-ecosystem knowledge: how to recognise a project and how to build the exact command line that runs or tests it. Each adapter returns an `Invocation { program, args, cwd, env, report, warnings }` — a fully resolved command for the process supervisor.

### `adapters::dotnet` (674 lines)

The most intricate adapter, because `dotnet test` has **two entirely separate execution paths**:

- **VSTest** (classic) takes `--logger "trx;LogFileName=..."`.
- **Microsoft.Testing.Platform** (MTP) takes `-- --report-trx --report-trx-filename ...` and **silently ignores** VSTest's `--logger`.

Because MTP ignores rather than rejects the wrong flags, a misclassification exits zero and simply leaves no report — surfacing as "the tests ran but nothing appeared". Classification (`classify_runner`, reading `.csproj` package references, `Directory.Build.props` inheritance, and `dotnet.config`) therefore has extensive tests, and a missing report is reported with a message naming this as the likely cause.

Also parses `launchSettings.json` profiles for application launches and handles multi-target frameworks and build configurations. Both paths emit TRX, so one parser serves both.

Application launches follow `dotnet run`'s own defaults: no named profile means the first `Project` profile applies (environment and `applicationUrl` included); `RunConfig.ignore_launch_settings` is the explicit `--no-launch-profile` opt-out, and warns when it strips `ASPNETCORE_ENVIRONMENT` (which silently disables user secrets). Detected test configurations are Debug-only — `#if !DEBUG` code paths make an auto-offered Release test run a trap. `BuildAction` / `build_action_invocation` produce the `dotnet build` / `build --no-incremental` / `clean` command lines behind the UI's build buttons.

### `adapters::node`

Detects Vitest/Jest from `package.json` dependencies, the package manager from lockfiles, and workspace roots. The detail worth knowing: **Vitest's JSON reporter replaces the console reporter**, silencing live output — so both reporters are requested together, which is also why the report file must be passed as `--outputFile.json=` rather than plain `--outputFile=`. Jest's default reporter writes to stderr and has no such problem; the JSON shape is the same, so one parser serves both.

### `adapters::manifest`

Declarative adapters: a TOML file supplies a detection file list, a test command template, and named run commands — no Rust involved. Templates substitute `{report}`, `{project}`, and `{root}`. A manifest that cannot express a test filter still runs, but attaches a warning so "re-run failed" never *silently* runs everything. Malformed manifests are skipped with errors reported alongside the ones that loaded. Full schema and walkthrough: [Adding an ecosystem](../guides/adding-an-ecosystem.md).

## `testing`

Report parsing. Every parser emits a **flat** `Vec<TestCase>` — the shape every format naturally produces — and `testing::tree::build` does the grouping into project → suite → test exactly once, so all runners yield an identical tree. Outcome roll-up uses worst-wins precedence (Failed > Other > Passed > Skipped) so one failing leaf colours every ancestor. `tree::failed_names` extracts fully-qualified names for "re-run failed".

Three parsers ([format details](../reference/test-reports.md)):

- `trx` — Visual Studio TRX. Joins `<Results>` (outcomes, timings) with `<TestDefinitions>` (class, assembly) on `testId`, because neither section alone is enough.
- `jest_like` — the `--json` shape shared by Jest and Vitest, accepting both spellings where they diverge.
- `junit` — JUnit XML, deliberately permissive because the format is a convention, not a standard. This parser is what makes declarative adapters practical.

`parse_file` treats a *missing* report as its own error with a message pointing at the usual cause (VSTest flags handed to MTP).

## `git`

Two implementations by design:

- **libgit2 (`git2`)** for reads and local mutations: status, structured diffs, staging, commits, branches, history. Fast, in-process, no porcelain parsing.
- **System `git`** for anything touching the network (push/pull/fetch): libgit2 would require reimplementing SSH agent discovery, macOS Keychain, Windows Credential Manager, and Git Credential Manager on three platforms; the user's `git` already has all of that configured. `git apply` is likewise delegated as the only correct implementation of partial patch application.

`git::patch` is the heart of line-level operations: it builds a unified diff containing only the selected lines and hands it to `git apply` (forward to stage, reversed to revert). The subtlety is that **unselected** lines must be treated differently per direction — an unselected addition is *dropped* going forward but becomes *context* in reverse, and vice versa for deletions. The module doc has the full truth table; getting it backwards produces patches that are rejected, or worse, apply and revert lines the user never selected.

`git::repo::Repo` exposes the full operation set: three comparison modes (working↔HEAD, working↔index, index↔HEAD), file/line staging and unstaging, line revert, discard, commit (with amend), branches (create from HEAD or any revision, switch, delete, and `checkout_remote_branch` — the `git switch` behaviour of creating a local tracking branch from a remote one), history, per-commit diffs, and stash. Integration tests live in `crates/core/tests/git_operations.rs`.

## `process`

The single place that spawns anything: test runs, app launches, git network calls. `Supervisor` spawns with layered environment, streams `ProcessEvent`s (stdout/stderr chunks, exit), and tracks running processes for cancellation.

- `chunker::Utf8Chunker` decodes raw byte chunks incrementally, holding back partial multi-byte characters across reads — output is read as bytes, not lines, because runners draw progress with bare `\r` and ANSI escapes that line-buffering would swallow.
- `kill` terminates the process **tree**: a new process group at spawn time (Unix `setpgid`, Windows process groups), because killing only the wrapper leaves `dotnet run`'s assembly or a dev-server holding its port.
- Colour-friendly defaults are layered under the config's own env: `FORCE_COLOR=1`, and the configuration key that re-enables .NET's console-logger colours under redirection (`Logging__Console__FormatterOptions__ColorBehavior=Enabled`).

## `importers::rider`

Best-effort conversion of JetBrains Rider `.run/*.xml` files. JetBrains publishes no stable schema, so the importer **never silently converts**: everything untranslatable lands in `RunConfig::warnings` and the UI shows a review step before saving. See [Rider import](../guides/rider-import.md).

## `config`

`.code-basics/config.json` — saved run configurations, versioned for future migration, meant to be checked in like Rider's `.run/`. Only user-created/imported configs are written; detected ones are re-derived every scan so the file stays small and detection keeps working as projects change. Also holds `favorites` and `order` (config ids; `sort_configs` puts favourites first, then the saved order, then names) and defines the `.code-basics/results/` report directory. `config::apply` layers a saved file onto a scanned workspace in one call. See [Configuration](../reference/configuration.md).

## `files`

Workspace file access for the Run tab's directory tree and editor. `list_dir` lists one directory per call (the tree expands lazily), directories first, filtered by the same `SKIP_DIRS` the scanner uses; `read_file`/`write_file` move file contents in and out of the editor. All paths are workspace-relative, and anything that could escape the root (absolute paths, `..`) is rejected. Reads refuse binary content and files over 5 MB with a clear error rather than garbage.

## `secrets`

.NET user secrets, the way `dotnet user-secrets` and Rider manage them: a project's `<UserSecretsId>` names a `secrets.json` under the user profile (`%APPDATA%\Microsoft\UserSecrets\<id>\` on Windows, `~/.microsoft/usersecrets/<id>/` elsewhere) — secrets never touch the workspace. `read` returns id/path/content; `write` validates against the same JSON dialect .NET's configuration loader accepts (comments and trailing commas included) and adds a `<UserSecretsId>` to the project file first when missing, like `dotnet user-secrets init`.

## `model`

The types shared with the frontend: `Project`, `RunConfig`, `Invocation`, `TestCase`/`TestNode`/`TestRunResult`, and their enums. Serialised camelCase; the JSON key names are pinned by tests. See [the IPC contract](ipc-contract.md).

## Testing conventions

Unit tests live with their module; larger adapter test suites sit in sibling files (`dotnet_tests.rs`, `node_tests.rs`, `rider_tests.rs`) included via `#[path]` under `#[cfg(test)]`. Fixture reports live in `crates/core/fixtures/reports/`. Run everything with `cargo test -p cb-core`, one test with `cargo test -p cb-core <name>`.
