# Build & Diagnostics MCP — notes / gotchas for later phases

- **`Invocation` carries NO pointer to the diagnostics artifacts** (unlike test's
  `ReportSpec`). The app side must recompute paths with
  `dotnet::build_log_paths(root, config.id)` to read them back. This is deliberate
  — `build_action_invocation` is pure and does no I/O.
- **Create the build dir before running.** `build_log_paths` only computes paths;
  the MSBuild file logger does NOT create the parent dir. `run.rs`/`debug.rs` now
  create it; Phase 3's `build_solution` command must `create_dir_all(config::build_dir(root))`
  before running the steps too.
- **Solution build = per-project artifacts, then merge.** `plan_solution_build`
  mints `solution-build:<name>` ids => distinct log files => no clobber. Phase 3
  must parse each step's two artifacts (via `build_log_paths`) and merge into one
  `BuildReport` (statuses combine: any Failed => Failed, else any warnings =>
  SucceededWithWarnings, else SucceededClean).
- **`last_build` is keyed by config id** (per Phase-2 instruction, like
  `last_test_run`). The MCP `get_*` tools take NO config arg (workspace-level).
  RESOLVED in Phase 4: reserved key `WORKSPACE_BUILD_KEY = "mcp-build:solution"`
  in `src-tauri/src/build/agent.rs`. `build_solution` writes it, `get_*` read it via
  `AppState::previous_build_for(root, KEY)`. Deliberately its own key — the editor's
  Build button does not record here — so "the most recent build" the server reports
  is unambiguously the most recent build the *server* ran.
- **build_solution scope decision (Phase 4):** runs ONE `dotnet build` at the
  workspace root (a minimal dotnet RunConfig with project=None ⇒ `dotnet build` with
  no project arg, resolving the .sln/single project in the root — the literal "build
  the solution"). NOT the per-project `plan_solution_build` + merge that earlier notes
  floated. Chosen because the task's Phase-4 spec is singular ("runs the build via
  slot.supervisor.run_tracked(...), writes+parses the file-logger artifacts") and one
  invocation is simpler and matches what a dev types. Trade-off: a repo whose .sln is
  not at the opened root (or has several) makes `dotnet build` error ⇒ status Failed
  (an honest answer, though not per-project). If per-project/merge is wanted later,
  add a pure `plan_workspace_build` + `merge_reports` in cb-core (tests-first) and loop
  run_tracked in `build/agent.rs::build_solution`.
- **Dead-code warnings** on `record_build`/`previous_build`/`build_report_or_never_built`
  are expected until Phase 3 wires the command + pipe host. Not an error (no
  `deny(warnings)` in the tree).
- **IPC types NOT done** — `src/ipc/types.ts` mirror for `BuildReport`/
  `BuildDiagnostic` is Phase 5. Key-pinning tests live in `build/build_tests.rs`
  (not central `model.rs`) since the types live in `build/model.rs`.
- **Env-var test race (pre-existing, not ours):** `mcp_editor`/`mcp_roslyn` tests
  set `CB_*_INSTANCES_PATH` via `std::env::set_var` and race under parallel test
  threads. Run cb-app tests with `--test-threads=1` to get a clean signal.

## Post-implementation review resolutions (all phases done + reviewed)

- **`BuildStatus::Building` now has a runtime producer (was the one major finding).**
  `build/agent.rs::build_solution` records `BuildReport::of_status(Building)` into
  `last_build` **before** awaiting `run_tracked`, overwriting it with the parsed report
  on completion. A concurrent `get_build_status` during a long build now truthfully
  reports "building" instead of the previous report / `NeverBuilt`. All six states are
  reachable at runtime.
- **`column`/`project` null-not-absent is now pinned** by
  `build_tests.rs::absent_column_and_project_serialise_as_null_not_omitted` — a
  `skip_serializing_if` regression would fail the suite (matches the SearchHit precedent).
- **Removed dead `AppState::previous_build`** (active-workspace read) — the pipe uses
  `previous_build_for(root, id)`. Its two tests were rewritten to read by explicit root.
- **Accepted as-is (design judgments, documented not changed):** (1) a cancelled build
  (`None` exit) reports `Failed` — no `Cancelled` state exists and agent-initiated builds
  rarely cancel; revisit only if it bites. (2) the parser's indented-line continuation
  heuristic is a mild guess, but low-risk under the errors-only/warnings-only file loggers.
- **STILL REQUIRED TO USE AT RUNTIME:** rebuild `target/release/cb-app.exe`
  (`cargo build --release`, **with the app closed** — a running app holds the exe →
  `Access is denied (os error 5)`) so `cb-app.exe mcp-build` is dispatchable and the pipe
  host serves. Until then the server code is complete and fully tested headlessly but
  cannot be exercised end-to-end. Install via Settings → the Build MCP panel after the
  release rebuild.
