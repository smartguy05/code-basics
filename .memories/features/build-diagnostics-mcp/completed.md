# Build & Diagnostics MCP — completed work

Plan: `~/.claude/plans/would-this-be-a-luminous-cupcake.md`. Server: `mcp-build`
(build_solution / get_errors / get_warnings / get_build_status).

## Phase 1 (core parse+model) — done (prior session)
`crates/core/src/build/{model,parse,mod,build_tests}.rs`, wired into `lib.rs`
after `browser`. `BuildStatus` = six distinct answers; `parse_report` only ever
yields the three a completed build justifies (Failed / SucceededWithWarnings /
SucceededClean). App layer supplies NeverBuilt/Building/CouldNotStart via
`BuildReport::of_status`.

## Phase 2 (invocation wiring + app-side cache slot) — done
- `crates/core/src/adapters/dotnet.rs`:
  - New `pub fn build_log_paths(root, config_id) -> (errors, warnings)` —
    `.code-basics/build/<sanitised id>.errors.log` / `.warnings.log`. Per-config
    names so a solution build's per-project steps do not clobber each other.
  - `build_action_invocation` now appends, **for Build/Rebuild only** (not Clean):
    `/p:GenerateFullPaths=true`, `-flp1:errorsOnly;logfile=<errors>`,
    `-flp2:warningsOnly;logfile=<warnings>`, and env
    `DOTNET_CLI_UI_LANGUAGE=en-US` layered UNDER config.env (user override wins).
  - Tests added in `dotnet_tests.rs`; updated two exact-`==vec!["build"]` tests to
    assert their real property (no target / no arg leak) since args now include loggers.
- `crates/core/src/config.rs`: `BUILD_DIR="build"`, `build_dir(root)`, `"build/"`
  added to `IGNORED` (gitignored, covered by existing ignore test).
- `crates/core/src/invocation.rs`: `plan_solution_build` reuses
  `build_action_invocation` unchanged — each step now carries the loggers with
  distinct artifacts. Test `plan_solution_build_steps_write_distinct_diagnostics_artifacts`.
- `src-tauri/src/state.rs`: `WorkspaceSlot.last_build: Mutex<HashMap<String,BuildReport>>`
  (keyed by config id like `last_test_run`); `record_build(root,id,report)` /
  `previous_build(id)`; free fn `build_report_or_never_built(Option<BuildReport>)`
  (absent cache => NeverBuilt, never empty SucceededClean). Tested in `state_tests.rs`.
- Regression guard: `commands/run.rs::build_project` and
  `commands/debug.rs::build_dotnet_target` now `create_dir_all(config::build_dir)`
  before running — the file logger does not create its dir (mirrors `invocation::build`
  pre-creating results_dir).

Tests: `cargo test -p cb-core --lib` 4199 passed; `cargo test -p cb-app --lib`
278 passed (single-threaded). `cargo fmt --check` clean.
(Two mcp_editor/mcp_roslyn failures appear only under parallel runs — a PRE-EXISTING
env-var race in those untouched tests; both pass with `--test-threads=1`.)

## Phase 4 (app pipe host + runner + registration) — done
Mirrors `src-tauri/src/roslyn` + `editor_context` exactly.
- `src-tauri/src/build/{mod,agent,agent_tests,pipe,pipe_tests,registry}.rs`:
  - `pipe.rs` — `\\.\pipe\code-basics.build.<pid>`, per-user DACL, `first_pipe_instance`
    fail-closed, process-global accept loop. `answer_line` refuses empty-workspace
    (`NoWorkspace`), reads `FeatureId::BuildMcp` fresh per call (`feature_enabled`) and
    hands it to `agent::answer` (two-way gating precedent).
  - `agent.rs` — `answer(app, workspace, enabled, call)`. Reads
    (get_errors/get_warnings/get_build_status) go through the PURE tested `read_answer`
    (disabled→Disabled; else `render` over `build_report_or_never_built(cached)`).
    `build_solution` runs ONE `dotnet build` at the workspace root (project=None) via
    `slot.supervisor.run_tracked`, drains+discards output, reads back the two file-logger
    artifacts via `dotnet::build_log_paths(root, KEY)`, `parse_report`s them, caches under
    the reserved `WORKSPACE_BUILD_KEY = "mcp-build:solution"`, renders `build_outcome`.
    Spawn failure / undirectory-able build dir ⇒ `CouldNotStart`; `Ok(None)` exit ⇒
    not-success (any captured diags still surface).
  - `registry.rs` — twin of roslyn/editor registry (`BuildInstance`, mint_token, own_exe,
    publish/republish/withdraw).
- `src-tauri/src/mcp_build.rs` (+ `mcp_build_tests.rs`) — runner. `TOOL_TIMEOUT = 20 min`
  (builds are slow; longest of any server). No `cb_core::build::mcp::liveness` module, so a
  local `alive(&BuildInstance)` wraps `cb_core::mcp::liveness::alive_by`. Otherwise mirrors
  `mcp_editor.rs`.
- `state.rs`: added `#[cfg(windows)] build_pipe` field + `build_pipe_published`/`set_build_pipe`,
  made `slot_for_root` pub, added `previous_build_for(root, id)` (canonicalize fallback — pipe
  answers by --workspace, not active). Tested in state_tests.rs.
- `lib.rs`: `mod build;` + `mod mcp_build;`, dispatch branch (8th self-dispatch), setup
  `build::start_listener`, exit `build::registry::withdraw`.
- `workspace.rs`: added `build::registry::republish` at the 3 roslyn/editor republish sites
  (open/set_active/close).

Tests: `cargo check --workspace --all-targets` clean (1 pre-existing dead-code warn on the
Phase-2 `previous_build`, unused since the pipe uses `previous_build_for`). `cargo test -p
cb-core --lib build::` 119 passed. `cargo test -p cb-app --lib --test-threads=1` 293 passed,
0 failed. `cargo fmt --check` clean.

RELEASE REBUILD NEEDED: the installed `target/release/cb-app.exe` must be rebuilt before
`cb-app.exe mcp-build` is dispatchable and the pipe host runs. Not done here (app may be
holding the exe — Access-denied — and it is a heavy relink); do `cargo build --release` when
the app is closed.

## Phase 5 (frontend panel + IPC types + install commands + docs) — done
- `src/ipc/types.ts`: added `BuildSeverity`/`BuildStatus`/`BuildDiagnostic`/`BuildReport`
  camelCase mirrors, keys matching `build_tests.rs` pins (column/project cross as null).
  (No Tauri command returns these yet — they cross the pipe as rendered text — but mirrored
  per the explicit Phase-5 task and the specta Type derives.)
- `src-tauri/src/commands/build_mcp.rs` (NEW): the 5-command install bridge, exact twin of
  `roslyn_mcp.rs` over `cb_core::build::mcp::install`. Registered in `lib.rs`
  (`pub mod build_mcp;` + 5 `generate_handler!` lines after the editor_context block).
- `src/ipc/api.ts`: `buildMcpStatus`/`buildMcpInstallPlan`/`installBuildMcp`/
  `buildMcpUninstallPlan`/`uninstallBuildMcp` wrappers.
- `src/components/BuildMcpPanel.tsx` (NEW): mirror of `EditorMcpPanel`, reuses
  `mcpServerLogic` wholesale (no new logic module needed — no new decisions), serverLabel
  "Build MCP server".
- Wiring: `featuresLogic.FeatureKey` += "buildMcp"; `shortcutLogic` PLUGIN_LABELS +
  `plugin.buildMcp` COMMAND (plugin: "buildMcp"); `pluginMenuLogic` PluginAction "buildMcp"
  + PLUGINS row (feature-gated, needsWorkspace) between editorMcp and redis;
  `pluginMenuLogic.test.ts` updated (ALL_KEYS, "lists every plugin", + dedicated block);
  App.tsx dispatch; WorkspaceTab handle (`openBuildMcp`) + state + effect + command reg +
  render.
- Docs: `docs/guides/build-mcp-server.md` (NEW, <500 lines); `docs/reference/mcp-servers.md`
  += Build section + header/see-also updates; `commands.md` self-dispatch line += mcp-build.

Gate: `pnpm typecheck` clean; `pnpm test` 2136 passed (82 files); `pnpm docs:index` +
`pnpm docs:check` pass (32 files, links resolve); `cargo check -p cb-app` clean (only the
pre-existing `previous_build` dead-code warn); `cargo fmt --check` clean.
RELEASE REBUILD still needed before `mcp-build` is dispatchable (unchanged from Phase 4).
