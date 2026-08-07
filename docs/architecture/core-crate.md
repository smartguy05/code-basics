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

Both paths emit TRX, so one parser serves both — pinned by fixtures from each (`fixtures/reports/sample.trx`, `mtp.trx`). The two differ in one other way that matters: VSTest needs `--logger console;verbosity=normal` and MTP needs `--output Detailed` before either prints a line per finished test, which is what the Tests view's live progress counter reads.

Also parses `launchSettings.json` profiles for application launches and handles multi-target frameworks and build configurations. Profiles describing a hosting model this app cannot start (IIS Express, Docker) are returned marked `launchable: false` rather than dropped, so a project whose only profile is one of those can explain itself instead of appearing to have none.

Application launches follow `dotnet run`'s own defaults: no named profile means the first `Project` profile applies (environment and `applicationUrl` included); `RunConfig.ignore_launch_settings` is the explicit `--no-launch-profile` opt-out, and warns when it strips `ASPNETCORE_ENVIRONMENT` (which silently disables user secrets). Detected test configurations are Debug-only — `#if !DEBUG` code paths make an auto-offered Release test run a trap — though the editor offers every configuration a project declares in `<Configurations>`. A multi-targeted project gets one configuration per framework, since neither `dotnet run` nor `dotnet test` will guess. `BuildAction` / `build_action_invocation` produce the `dotnet build` / `build --no-incremental` / `clean` command lines behind the UI's build buttons.

Project classification reads `<OutputType>` (including from an inherited `Directory.Build.props`), falling back to SDK defaults for the SDKs that imply an executable, and to the `<UseMaui>` / `<IsAspireHost>` / `Aspire.AppHost.Sdk` workload markers for app types that leave `<OutputType>` to the workload. `Microsoft.NET.Sdk.Razor` is deliberately *not* treated as an executable: it builds Razor class libraries.

### `adapters::msbuild`

The opt-out from filesystem-only detection, off unless a workspace sets `msbuildEvaluation`. Runs `dotnet msbuild <project> -getProperty:...` and overlays the result on the XML scan, which is the only way to resolve properties set behind `Condition`s or in imported `.props` files. More than one property is always requested because that is what makes the SDK emit JSON rather than a bare value. Properties only — `-getProperty` does not return items, so package references (and therefore runner classification) still come from the XML. Every failure mode, including no SDK at all, falls back to the shallow scan rather than failing the workspace.

### `adapters::solution`

Reads `.sln` (the classic tab-indented format, including the `NestedProjects` section that gives solution folders) and `.slnx` (the XML replacement). Used purely to group what the scan already found — the .NET analogue of a Node workspace — so nothing is ever run *through* a solution. Project paths are normalised to forward slashes so they compare equal to the paths the scan produces.

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

`git::repo::Repo` exposes the full operation set: three comparison modes (working↔HEAD, working↔index, index↔HEAD), file/line staging and unstaging, line revert, discard, commit (with amend), branches (create from HEAD or any revision, switch, delete, and `checkout_remote_branch` — the `git switch` behaviour of creating a local tracking branch from a remote one), history, per-commit diffs, merge, and stash. Integration tests live in `crates/core/tests/git_operations.rs`.

`git::attribution` and `git::grouping` turn a diff into the decisions behind it. Attribution matches recorded agent edits (see [`intents`](#intents)) onto the current diff **by content only** — records carry line numbers and they are discarded, because between the edit and the review the file has been edited again, formatted and partly reverted, all of which move a line without changing what it says. Matching runs a three-rung ladder (exact → whitespace-collapsed → whitespace-removed) and stops there deliberately: case folding or comment stripping would let genuinely different lines compare equal. Evidence is *contiguity*, not single lines — a run scores `fidelity × distinctness × bulk` and must clear 0.60, which is exactly `0.75 × 0.80`, the statement that a match surviving only the most aggressive normalisation must be carried by near-unique text. Hunks are never split: `DiffLine::index` is `build_patch`'s contract, so a hunk keeps its identity and may carry several labels, naming a `dominant` record only on a strict majority. The governing rule is that **a wrong label is much worse than no label**, and every threshold is tuned to abstain.

`grouping` then buckets what attribution produced: stated intent first, then formatting (compared with whitespace ignored, as `git diff -w` does — except in Python and YAML, where the indent *is* the syntax), then the enclosing symbol from the hunk header git already writes. Rename detection is deliberately not attempted.

`merge_branch` follows `git merge`: up-to-date and fast-forward cases are distinguished from a real merge commit, and the result says which happened, because otherwise the branch list looks identical in all three. It refuses to start when tracked files are modified or another operation is in progress — a conflicted merge writes into the working tree, and separating that from pre-existing edits afterwards is not something the user should have to do. Conflicts are **not** an error: the merge is left in progress with `MERGE_HEAD` intact so it can be resolved in the Changes tab and committed, exactly as on the command line, with `abort_merge` as the way out.

Checkout is wrapped by `checkout_tree_tolerating_locks`, which exists for one Windows behaviour: a directory carrying the read-only attribute cannot be removed, and `RemoveDirectory` reports that as "Access is denied". The attribute is meaningless on directories but tooling sets it in bulk, so any branch switch that removes a directory fails. The wrapper clears it across the working tree's directories (never files, where the attribute *is* meaningful) and retries before falling back to a short retry for a genuinely held handle.

## `process`

The single place that spawns anything: test runs, app launches, git network calls. `Supervisor` spawns with layered environment, streams `ProcessEvent`s (stdout/stderr chunks, exit), and tracks running processes for cancellation.

- `chunker::Utf8Chunker` decodes raw byte chunks incrementally, holding back partial multi-byte characters across reads — output is read as bytes, not lines, because runners draw progress with bare `\r` and ANSI escapes that line-buffering would swallow.
- `kill` terminates the process **tree**: a new process group at spawn time (Unix `setpgid`, Windows process groups), because killing only the wrapper leaves `dotnet run`'s assembly or a dev-server holding its port.
- Colour-friendly defaults are layered under the config's own env: `FORCE_COLOR=1`, and the configuration key that re-enables .NET's console-logger colours under redirection (`Logging__Console__FormatterOptions__ColorBehavior=Enabled`).

## `intents`

What a coding agent said it was doing, collected while it is still cheap to know. Supports Claude Code and Codex, whose hook systems converged on nearly the same design — the same handler shape, the same event names, JSON on stdin — so `providers/` stays thin and exists mainly to answer *where the files are* and *what shape the payload is*.

The load-bearing detail is that **neither agent lets a model attach a rationale to a tool call**. The edit hook knows what changed and cannot say why; the end-of-turn hook has the agent's closing message and cannot say which edit it refers to. So the two are written separately — `edits.jsonl` and `labels.jsonl` — and joined afterwards on the turn identifier both carry (`prompt_id` in Claude Code, `turn_id` in Codex, normalised at ingest). Because the end-of-turn hook fires once per turn, that join is exact rather than a guess.

Every other provider difference is also resolved at ingest, so `IntentRecord` is provider-blind: Claude Code's before/after strings (in either of its two spellings) and Codex's V4A patch envelope (`patchfmt`, which must also cope with the envelope arriving as raw freeform text or nested inside a shell call) all reduce to removed and added *lines*, which is exactly what attribution consumes.

Failing hooks entirely, `providers::history` mines each agent's existing session files, matching them to a workspace by the `cwd` recorded inside. Those labels are weaker and known to be: in a Claude Code transcript an assistant message contains *either* prose *or* tool calls and never both, so the best available label is the nearest preceding sentence covering the several edits that followed. Grouping keys on that block of prose rather than on the individual call — keying on the call made every edit its own card, which measured 127 groups from 145 hunks against this repository and defeated the point.

The hook itself is this application re-invoked (`src-tauri/src/recorder.rs`), so there is no script to keep in step with the format and no interpreter to depend on. It never fails loudly, never takes long, and does nothing at all in a workspace that has not opted in — a user-level hook fires for every repository on the machine.

## `inspect`

Reading the real objects out of a .NET crash dump or a running process. The heap walk itself needs ClrMD, which is a .NET library, so it happens in the `cb-inspector` sidecar and the answer comes back as a file — which makes this **the same shape as [`testing`](#testing)**: a process streams its output live and writes a structured report, and the tree is built from the report afterwards. `dotnet test` leaves a `.trx`; the inspector leaves a `result.json`. Because the exchange is one file in and one file out, `process/` needed no changes at all: cancellation and process-tree kill work because the sidecar is just another supervised process.

- `model` — the crossing types, including `ObjectValue` (the tagged union a node's value collapses to) and `Caps`.
- `graph` / `tree` — parse the sidecar's loose wire format, classify each raw node, and assemble flat nodes into rooted trees with warnings for anything that did not fit.
- `sidecar` — locating an executable (`CB_INSPECTOR_PATH`, then the bundled directory), session paths, failure codes, and the retry rule: a reported bitness mismatch on x64 is the only failure that earns a second attempt.
- `session` — the decisions around one capture: which bitness to try first, the workspace's caps, arming dumps for a run, pruning, and an honest `InspectStatus` when nothing is installed.
- `dumps` — the three `DOTNET_Dbg*` variables that make the runtime write a heap dump on an unhandled crash with no tooling at all, plus filename encoding/decoding, listing and retention.

Two rules run through all of it. The governing one is [`grouping`](#git)'s: **a wrong value is much worse than no value**, so anything unreadable becomes an explicit `Unavailable` carrying a sentence and any cap becomes an explicit `Elided` — never a shorter list that looks complete. The second is that this reads *memory*, not an execution context: no method is called and no property is evaluated, which is why a computed property shows as the fields behind it. Both are stated wherever the feature surfaces.

Dump capture is opt-in per workspace and off by default, because a dump is a verbatim copy of process memory. Design note: [live inspection](live-inspection.md). User guide: [inspecting objects](../guides/inspecting-objects.md).

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
