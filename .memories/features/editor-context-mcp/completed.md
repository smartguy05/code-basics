# Completed — Editor Context MCP

## Final integration verification (main session, 2026-09-14)

Re-ran the FULL gate on the merged tree of all 5 stages (workflow self-reports are
not trusted per the standing rule). All green:
- `cargo check --workspace --all-targets` → clean.
- `cargo test --workspace` → 0 failed (121 cb-core `editor_context::`, 11 host
  `editor_context` in cb-app, 4 `mcp_editor`).
- `cargo fmt --check` → exit 0. `cargo clippy --workspace --all-targets` → clean
  (only the pre-existing `redis/dsn.rs` manual-split_once warning).
- `pnpm typecheck` → clean. `pnpm test` → 82 files / 2133 passed.
  `pnpm docs:check` → passed.

**Gotcha (worth remembering for future workflows):** the IDE's live diagnostics
after the workflow showed phantom errors — `crate::editor_context` "not found" in
state.rs, `EditorRefusal::answer` "not found", swathes of dead_code. ALL were
STALE mid-workflow LSP snapshots captured before the final wiring/methods landed.
The real compiler (`cargo check`) disagreed completely. Do not act on post-workflow
IDE diagnostics without re-compiling first.

**Not done:** no end-to-end manual run against a live window + MCP client
(headless). Recommend driving `cb-app.exe mcp-editor --workspace <root>` once to
confirm a real tool call round-trips.

## Stage 6 — docs + index (done 2026-09-14)

Documented the feature and re-synced the generated index. All docs gates green
(`pnpm docs:index`, `pnpm docs:check`).

Files created:
- **docs/guides/editor-context-mcp.md** (248 lines) — the feature guide,
  mirroring `roslyn-mcp-server.md`/`sql-mcp-server.md`: what it is, the
  push-from-frontend model, the privacy feature flag (`editorContextMcp`, default
  on) and its two-fold gate (frontend stops pushing + pipe host re-checks), the 4
  argument-free tools, the position convention + 4096-code-point selection cap,
  the 4 refusal codes + 5 instance errors, `--workspace` as the boundary, the
  pipe/registry/liveness security model, read-only with no per-call consent gate,
  the previewed install flow (3 provider/scope targets, `ACCESS_NOTE`), the
  single-active-editor MVP limitation, and a code-location table.
- **docs/reference/mcp-servers.md** (62 lines) — NEW reference file holding the
  install/uninstall command bridges for the SQL, browser, Roslyn and
  editor-context MCP servers, including `set_editor_context`. Extracted from
  commands.md because adding the editor-context section pushed commands.md over
  the hard 500-line `docs:check` limit (it was already at 500). This is the
  "split it logically" the check demanded.

Files edited:
- **docs/reference/commands.md** — added then extracted the MCP-server sections;
  now 468 lines. The former "## SQL MCP server" / "## Roslyn MCP server" /
  "## Editor context MCP server" sections are replaced by a single
  "## MCP server installation" pointer to `mcp-servers.md` (Tasks server install
  bridge stays under the Tasks section).
- **docs/guides/roslyn-mcp-server.md** — inbound link retargeted
  `commands.md#roslyn-mcp-server` → `mcp-servers.md#roslyn-mcp-server` (the
  anchor moved with the section).
- **docs/README.md** — added the guide (under guides) and the new reference file
  (under Reference).
- **docs/INDEX.md** — regenerated via `pnpm docs:index` (NOT hand-edited): 756
  files, 235 commands, 236 IPC wrappers, 213 core modules.

Note: the 6 new `#[tauri::command]`s were already registered in
`generate_handler!` (Stage 3); Stage 6 only documented them. Their reference rows
live in `mcp-servers.md`, not `commands.md`.

### Verification
- `pnpm docs:index` → wrote INDEX.md, exit 0.
- `pnpm docs:check` → "passed: 31 files, all under 500 lines, all relative links
  resolve", exit 0.

## Stage 5 — frontend (done 2026-09-14)

Wired the frontend half: assemble the pushed `EditorContext`, report the live
editor position, push it debounced while the feature is on, and add the install
UI beside the other MCP servers.

Files created:
- **src/components/editorContextLogic.ts (+ .test.ts)** — pure, node-env,
  no-DOM. `buildEditorContext` (workspace-source files only; path normalisation
  to forward slashes; live cursor/selection/viewport attached only when the
  active tab is a workspace file), `pushRecent` (dedupe-to-front, cap 20,
  same-ref no-op), `capSelectionText` (`SELECTION_TEXT_CAP=4096` code points,
  surrogate-safe), `editorContextEqual` (structural, so an unrelated re-render
  does not re-push). Types `LiveEditorPosition`, `EditorContextInput`. 30 tests.
- **src/components/EditorMcpPanel.tsx** — the TWIN of `RoslynMcpPanel.tsx`,
  reusing `mcpServerLogic` wholesale (`planOutcome(.., "Editor context MCP
  server")`) and the shared `PlanPreview`. Editor-specific description/caveats:
  states that selection text and file paths reach the agent, read-only, and only
  while the feature is on.

Files edited:
- **src/ipc/api.ts** — `setEditorContext(root, ctx)` push wrapper + the five
  install wrappers (`editorMcpStatus`/`editorMcpInstallPlan`/`installEditorMcp`/
  `editorMcpUninstallPlan`/`uninstallEditorMcp`) mirroring `roslynMcp*`.
- **src/components/FileEditor.tsx** — new `onEditorContextChange` prop; a
  debounced (`EDITOR_CONTEXT_DEBOUNCE_MS=150`) `updateListener` reporting live
  caret/selection/viewport on doc/selection/viewport/geometry/focus changes,
  gated to workspace files inside `scheduleContextReport`; an initial report on
  build; timer cleared on unmount. Reuses `usagesExtension.visibleLineRange`.
- **src/views/RunView.tsx** — new `editorContextEnabled` prop; `recentPaths` and
  `editorLive` state + `lastPushedContext` ref; `setFileDirty` records recency on
  a workspace file becoming dirty; a push effect building the context via
  `buildEditorContext` and calling `api.setEditorContext` (debounced 200ms,
  skipped when equal, and only while enabled); `onEditorContextChange` wired on
  the active editor only while the feature is on.
- **src/components/WorkspaceTab.tsx** — import `EditorMcpPanel`;
  `editorContextEnabled = featureEnabled(features, "editorContextMcp")`;
  `editorMcpPanelOpen` state + `openEditorMcp` + close-on-disable effect;
  register `plugin.editorMcp` while enabled; `openEditorMcp` on the
  `WorkspaceTabHandle` (interface + both ref shapes + the stable wrapper); render
  the panel behind an `Occluder`; pass `editorContextEnabled` to `RunView`.
- **src/App.tsx** — route `row.action.kind === "editorMcp"` → `openEditorMcp()`.
- **src/components/featuresLogic.ts** — `FeatureKey` gained `editorContextMcp`.
- **src/shortcutLogic.ts** — `PLUGIN_LABELS.editorContextMcp` + the
  `plugin.editorMcp` command (tagged `plugin: "editorContextMcp"`).
- **src/components/pluginMenuLogic.ts (+ .test.ts)** — new `PluginAction`
  `{kind:"editorMcp"}` and a feature-gated `PLUGINS` entry (omitted when the
  feature is off, disabled-with-reason when no codebase). Test updated for the
  new key + a dedicated block.
- **src/components/McpToolsPage.tsx** — `SERVER_FEATURE.editor =
  "editorContextMcp"`, so the MCP-tools settings page shows a server toggle bound
  to the feature (the server itself already appears via `ServerId::ALL`).

### Verification
- `pnpm typecheck` → clean.
- `pnpm test` → 82 files, 2133 tests passed (incl. 30 editorContextLogic + the
  updated pluginMenuLogic).
- `pnpm coverage` → lines 98.12% (gate ≥70).

### Scope kept
Did NOT do Stage 6 docs (`pnpm docs:index`, `docs/reference/commands.md`, new
`docs/guides/editor-context-mcp.md`) — that is the next stage.

## Stage 3 — src-tauri backend (done 2026-09-14)

Wired the pushed editor state and the pipe host, a module-for-module TWIN of the
Roslyn host (`src-tauri/src/roslyn/`, `mcp_roslyn.rs`, `commands/roslyn_mcp.rs`).
All 273 cb-app tests pass (incl. 15 new editor tests); fmt clean; clippy clean
for cb-app (only pre-existing `redis/dsn.rs` split_once warning remains).

Files created:
- **src-tauri/src/editor_context/mod.rs** — `start_listener` (opens the
  process-global pipe at `.setup`, `set_editor_pipe` + `republish`); `#[cfg]`
  no-op off Windows. Declares `agent`, `#[cfg(windows)] pipe`, `registry`.
- **src-tauri/src/editor_context/registry.rs** — `mint_token` (two v4 UUIDs),
  `own_exe`, `instance_for`, `publish` (read-modify-write upsert), `open_roots`,
  `republish` (no-op with no listener), `withdraw`. Over
  `cb_core::editor_context::instances`.
- **src-tauri/src/editor_context/agent.rs (+ agent_tests.rs)** — the ONE decision
  point, made a **pure testable free fn**: `answer(enabled, Option<EditorContext>,
  EditorToolCall) -> ToolAnswer`. feature-off => `EditorRefusal::Disabled`;
  none => `EditorRefusal::NoContext`; else dispatch to
  `cb_core::editor_context::render::*`. 6 tests (disabled-even-with-context,
  distinct codes, per-tool routing, genuine-empty stays data).
- **src-tauri/src/editor_context/pipe.rs (+ pipe_tests.rs)** `#[cfg(windows)]` —
  process-global named-pipe listener mirroring roslyn/pipe.rs: same
  `user_only_descriptor` DACL (`D:P(A;;GA;;;<SID>)`), `first_pipe_instance(true)`,
  re-arm-before-handoff accept loop. `answer_line` parses request→call, guards
  blank workspace => `EditorRefusal::NoWorkspace`, reads `feature_enabled()`
  fresh + `state.editor_context_for(request.workspace)` (State guard dropped
  before await), calls `agent::answer`. NO Peer plumbing (agent takes plain
  data, no banner planned) — dropped client_process_id/process_image_name. 6
  tests incl. pipe-name-matches-core-derivation and no-collision with roslyn/
  browser.
- **src-tauri/src/mcp_editor.rs (+ mcp_editor_tests.rs)** — stdio server (client
  side) mirroring mcp_roslyn.rs. `TOOL_TIMEOUT=15s` (¼ of roslyn's; reading
  pushed state is a map lookup). MCP route via `mcp::serve`; `call_tool` checks
  `tools::is_known` → `load_gate().is_enabled(ServerId::EditorContext,..)` →
  require --workspace (`EditorRefusal::NoWorkspace.answer()`) → `instances::load`
  → `choose_instance(.., &liveness::alive)` → `exchange` over
  `instances::pipe_name(verified pid)`. Off-Windows => unsupported. 4 tests.
- **src-tauri/src/commands/editor_context_mcp.rs** — 5 install verbs
  (`editor_mcp_server_status/_install_plan/uninstall_plan`,
  `install_/uninstall_editor_mcp_server`) delegating to
  `cb_core::editor_context::install` + `providers::apply_writes_atomically`; PLUS
  `set_editor_context(state, root, ctx)` push command → `record_editor_context`
  (records only; feature-off gate is frontend-side; harmless no-op if slot
  closed).

Files edited:
- **src-tauri/src/state.rs** — added `editor_context: Mutex<Option<EditorContext>>`
  to WorkspaceSlot (+ ctor); `record_editor_context(root,ctx)->bool` and
  `editor_context_for(root)->Option<EditorContext>` via new private
  `slot_for_root` (dunce::canonicalize fallback, mirrors lsp_for_root — used by
  BOTH record and read since root originates outside the backend);
  `#[cfg(windows)] editor_pipe` slot + `editor_pipe_published`/`set_editor_pipe`
  (replace-then-stop-after-guard-drop, mirrors roslyn_pipe).
- **src-tauri/src/lib.rs** — `mod editor_context;`, `mod mcp_editor;`,
  `commands::editor_context_mcp`; self-dispatch branch after mcp_redis; 6 commands
  in `generate_handler!`; `editor_context::start_listener` spawned in `.setup`
  (twin of roslyn spawn); `editor_context::registry::withdraw` at Exit.
- **src-tauri/src/commands/workspace.rs** — `editor_context::registry::republish`
  beside the roslyn one in open_workspace / set_active_workspace / close_workspace.

Key decisions honored: (1) render only gets `&EditorContext`, genuine empties
stay data. (2) the 3 pre-render refusals produced app-side — Disabled/NoContext
in `agent::answer`, NoWorkspace in pipe host + shim. (4) namespaced strings all
from cb_core::editor_context. (5) reused mcp::{ndjson,serve,install,liveness},
tool_gate wholesale.

### Verification
- `cargo test -p cb-core editor_context::` → 121 passed.
- `cargo test -p cb-app` → 273 passed, 0 failed (15 new editor tests confirmed
  running by name).
- `cargo check --workspace --all-targets` → clean.
- `cargo fmt --check` → clean (fmt wrapped one long line in mcp_editor_tests.rs).
- `cargo clippy -p cb-app --all-targets` → no cb-app warnings.

### Scope kept
Did NOT touch the frontend (Stage 5: editorContextLogic.ts, FileEditor/RunView
push, api.ts wrappers, Settings/Plugins wiring), docs (Stage 6:
`pnpm docs:index`, commands.md, new guide), or installers beyond Stage 1.

## Stage 2 — cb-core pure module (done 2026-09-14)

Built `crates/core/src/editor_context/` as a module-for-module TWIN of
`crates/core/src/roslyn/`. Every file has a co-located `#[path="..._tests.rs"]`
`mod tests`; written tests-first. 121 editor_context tests, all green.

Files created (impl + tests each):
- **argv.rs** — SUBCOMMAND `"mcp-editor"`, MARKER `"code-basics-mcp-editor"`,
  `--workspace`/`--instance`, `is_mcp_editor_invocation`,
  `parse_mcp_editor_args` (+ `McpEditorInvocation`).
- **wire.rs** — METHOD `"editor.tool"`, `Request{protocol,token,workspace,tool,
  arguments}`, `ToolAnswer{ok,code,text}`, `request_value`/`answer_value`,
  `RequestProblem` (4) + `parse_request` (method→shape→protocol→token order),
  `token_matches`, `PipeFailure` (4) + `parse_answer`.
- **instances.rs** — `PROTOCOL_VERSION=1`, `Listener`, `EditorInstance`,
  `InstancesFile`, `pipe_name(pid)=\\.\pipe\code-basics.editor.<pid>`,
  `instances_path` (`CB_EDITOR_INSTANCES_PATH` override), tolerant load / atomic
  save, `upsert`/`remove`, `choose_instance` — SAME five `InstanceError`
  variants as Roslyn (NoneRunning, WorkspaceNotOpen, Ambiguous, VersionMismatch,
  PipeNameMismatch); PipeNameMismatch code is `"editor_registry_tampered"`.
- **liveness.rs** — `alive(&EditorInstance)` → `crate::mcp::liveness::alive_by`;
  re-exports `same_executable`.
- **tools.rs** — 4 tool consts (`get_active_file`/`get_selection`/
  `get_open_files`/`get_recent_files`), `ALL`, `descriptors()` (empty-object
  schemas — tools take NO arguments), `EditorToolCall` (Copy enum), `parse_call`
  (ignores arguments — spurious args are NOT a shape error), `is_known`,
  `unknown_tool_sentence`.
- **answer.rs** — `EditorRefusal { NoWorkspace, Disabled, NoContext,
  NoActiveFile }` with `code()`/`sentence()`/**`answer()`** (the `.answer()`
  helper is new vs Roslyn — render.rs calls it for NoActiveFile; RoslynRefusal
  has no such method because roslyn render builds ToolAnswer inline).
- **render.rs** — `get_active_file`/`get_selection`/`get_open_files`/
  `get_recent_files` over `&model::EditorContext` → `ToolAnswer`. NoActiveFile is
  a REFUSAL (from answer.rs); empty selection / no open tabs / no recent files
  are GENUINE ok answers ("complete answer, not truncated"), never refusals.
  Abstains on cursor/viewport not reported; abstains on dirty/pinned if the
  active file is not among open tabs; never fabricates a path or position.
- **serve.rs** — SERVER_NAME `"code-basics-editor"`, INSTRUCTIONS,
  `initialize_result`, `tools_list_result` (→ `tool_gate::filter_descriptors(..,
  ServerId::EditorContext, ..)`), `disabled_tool_answer`, `answer_result`,
  `unknown_tool_answer`.
- **install.rs** — `editor_mcp_entry`, `entry_args`, `status_for`,
  `install_plan_for`, `uninstall_plan_for`, `caveats_for`, `ACCESS_NOTE`
  (states selection text + open/recent PATHS reach the agent, read-only),
  SCOPE_NOTE (also mentions the feature must be ON). Reuses `mcp::install`
  helpers wholesale.
- **mod.rs** — declares the 9 submodules; declared `pub mod editor_context;` in
  `crates/core/src/lib.rs`.

Also:
- **tool_gate/mod.rs** — replaced the Stage-1 `TODO(editor-context stage 2)`
  `Vec::new()` arm with `crate::editor_context::tools::descriptors()`.
- **tool_gate/tool_gate_tests.rs** — `the_editor_context_server_is_known` now
  asserts the real 4 tool names instead of `descriptors().is_empty()`.

Reused wholesale (NOT reimplemented): `mcp::ndjson`, `mcp::serve`,
`mcp::install/*`, `mcp::liveness`, `tool_gate`.

### Verification
- `cargo test -p cb-core --lib editor_context::` → 121 passed.
- `cargo test -p cb-core` (full) → lib 4169 passed; all integration suites green
  (0 failed anywhere), ~5 min wall incl. process::/lsp suites.
- `cargo fmt` + `cargo fmt --check` → exit 0.
- `cargo clippy -p cb-core --lib` → clean for editor_context; the one remaining
  warning is pre-existing in `redis/dsn.rs` (manual split_once), untouched here.

### Scope kept
Did NOT touch `src-tauri` (state.rs, command surface, runners, lib.rs
self-dispatch), the frontend, docs, or the installers — all Stage 3+ work.

## Stage 1 — Contracts (done 2026-09-14)

The shared foundation the rest of the feature builds on. Files touched:

- **crates/core/src/model.rs** — added `EditorContext` plus nested structs
  `EditorCursor`, `EditorViewport`, `EditorSelection`, `EditorOpenFile`,
  `EditorRecentFile` (all `Serialize, Deserialize, Clone, Debug, specta::Type`,
  `rename_all="camelCase"`; the position/copy structs also `Copy, Eq`). The
  "what am I looking at" fields (`activeFile`, `cursor`, `viewport`,
  `selection`) are `Option<_>` with **no** `skip_serializing_if`, so they
  always cross as `null` rather than absent — lets the shim tell an honest
  nothing from a missing key. `selection.text` is a plain `String` (capped
  frontend-side). Added two key-pinning tests in the `tests` module:
  `editor_context_serialises_with_the_keys_the_shim_reads` and
  `an_empty_editor_context_keeps_its_null_keys`.
- **src/ipc/types.ts** — hand-mirrored `EditorContext` + the five nested
  interfaces. Optional Rust fields mirror as `X | null` (not optional `?`),
  matching the no-`skip_serializing_if` decision.
- **crates/core/src/tool_gate/mod.rs** — added `ServerId::EditorContext`
  (id `"editor"`, label `"Editor context"`), extended `ALL` to 5. The
  `descriptors()` arm **temporarily returns `Vec::new()`** with a
  `TODO(editor-context stage 2)` comment — the `crate::editor_context::tools`
  module does not exist yet, so this keeps the crate compiling.
- **crates/core/src/tool_gate/tool_gate_tests.rs** — renamed
  `list_reports_all_four_servers...` → `list_reports_every_server...` and made
  it assert `ServerId::ALL.len()`; added
  `every_server_has_a_distinct_stable_id_and_label` and
  `the_editor_context_server_is_known`.
- **crates/core/src/features/mod.rs** — added `FeatureId::EditorContextMcp`
  (id `"editorContextMcp"`, label `"Editor context MCP"`, one-line
  description), extended `ALL` to 6, `default_enabled => true` with a
  justifying comment mirroring the other MCP features (feature visible grants
  nothing; installing into an agent is a separate previewed step; read-only).
- **crates/core/src/features/features_tests.rs** — added `editorContextMcp`
  to `every_feature_has_a_distinct_stable_id` and to
  `every_feature_states_its_own_default`.

### Verification
- `cargo test -p cb-core --lib editor_context` → 3 passed (2 model, 1 gate).
- `cargo test -p cb-core --lib model::tests::editor / features::tests /
  tool_gate::tests` → all green.
- `cargo test -p cb-core --lib model::` → 153 passed.
- `cargo fmt` (wrapped one long assert line) + `cargo fmt --check` exit 0.
- Full `cargo test -p cb-core` — see notes.md / gate result.

### Necessary deviation from "do not touch src-tauri"
The cb-core test gate (`features::store::tests`) reads the real packaging
artefacts and **fails** unless every `FeatureId` appears in BOTH installers
(`every_feature_appears_in_both_installers`,
`the_windows_installer_writes_json_the_app_can_parse`,
`first_launch_adopts_the_bytes_the_windows_installer_writes`). Adding
`EditorContextMcp` therefore forced two packaging edits, without which
`cargo test -p cb-core` stays red:
- **src-tauri/resources/linux/features.json** — added `"editorContextMcp":true`.
- **src-tauri/installer/windows/installer.nsi** — added `Var
  FeatureEditorContextMcp` + `...Checkbox`, the page checkbox (label
  `"Editor context MCP"` + verbatim description), the default-check and
  state-read blocks, and the `,"editorContextMcp":` FileWrite branch in
  `WriteFeaturesSeed`; updated the example-bytes comment. Description made
  plain ASCII (parentheses, no em-dash) to match sibling entries and avoid
  NSIS codepage fragility — the string must be byte-identical in mod.rs and
  the .nsi, pinned by the test.
These are the feature-flag *contract*, not the stage-2 backend/command surface,
which remains untouched.

### Scope otherwise kept
Did NOT touch the `src-tauri` command surface, `state.rs`, the `editor_context`
core module, or the frontend beyond `types.ts`, per the stage-1 brief.
