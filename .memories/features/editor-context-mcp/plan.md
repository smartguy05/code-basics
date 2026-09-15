# Implementation plan — Editor Context MCP

Pattern: **Roslyn twin** (a named-pipe shim forwarding to the running app), with one
key difference — the state to expose lives in the **React frontend**, not the backend,
so it must be *pushed* into `AppState` (like the browser consent slot) and the pipe host
reads it back.

## A. IPC model (`crates/core/src/model.rs` + `src/ipc/types.ts`)
New camelCase struct `EditorContext` (put on `model.rs` — crosses IPC) with a
key-pinning test in the `tests` module. Fields:
- `activeFile: Option<String>` (workspace-relative, forward slashes)
- `cursor: Option<{ line: u32, character: u32 }>`
- `viewport: Option<{ firstVisibleLine: u32, lastVisibleLine: u32 }>`
- `selection: Option<{ startLine, startCharacter, endLine, endCharacter, text: String }>`
  (`text` capped, e.g. 4 KiB; empty selection => None)
- `openFiles: Vec<{ path, active, dirty, pinned }>`
- `recentFiles: Vec<{ path }>` (edit-recency order, capped ~20)
Mirror exactly in `types.ts` (hand-mirrored; see docs/architecture/ipc-contract.md).

## B. cb-core pure module `crates/core/src/editor_context/`
Mirror `crates/core/src/roslyn/` module-for-module, each with a `#[path] *_tests.rs`:
- `argv.rs` — SUBCOMMAND `"mcp-editor"`, MARKER `--code-basics-mcp-editor`, `--workspace`/`--instance`,
  `is_mcp_editor_invocation`, `parse_mcp_editor_args`.
- `wire.rs` — METHOD `"editor.tool"`, `Request { protocol, token, workspace, tool, arguments }`,
  `ToolAnswer { ok, code, text }`, request/answer value builders, `parse_request`, `PipeFailure`.
- `instances.rs` — `PROTOCOL_VERSION`, `Listener`, `EditorInstance`, `InstancesFile`,
  `pipe_name(pid) = \\.\pipe\code-basics.editor.<pid>`, `instances_path` (`CB_EDITOR_INSTANCES_PATH`),
  tolerant load / atomic save, `upsert`/`remove`, `choose_instance` (same 5 `InstanceError`s as Roslyn).
- `liveness.rs` — delegate to `crate::mcp::liveness::alive_by`.
- `tools.rs` — the 4 tool-name consts, `ALL`, `descriptors()`, `EditorToolCall`, `parse_call`, `is_known`.
- `answer.rs` — `EditorRefusal { NoWorkspace, NoContext, ... }` with `code()`/`sentence()`.
- `render.rs` — `EditorContext` -> agent prose per tool (abstain cleanly: no active file =>
  a plain "no file is active" sentence, never a fabricated path).
- `serve.rs` — SERVER_NAME `"code-basics-editor"`, INSTRUCTIONS, `initialize_result`,
  `tools_list_result` (calls `tool_gate::filter_descriptors(.., ServerId::EditorContext, ..)`),
  `answer_result`, `unknown_tool_answer`, `disabled_tool_answer`.
- `install.rs` — `editor_mcp_entry(exe, workspace)`, `entry_args`, `status_for`,
  `install_plan_for`, `uninstall_plan_for`, `caveats_for` (state that selection text &
  file paths reach the agent; read-only). Reuse `crate::mcp::install` merge helpers.

Reuse wholesale: `mcp::ndjson`, `mcp::serve` (route/envelopes), `mcp::install/*`,
`mcp::liveness`, `tool_gate`.

## C. tool_gate + features (small edits)
- `tool_gate/mod.rs`: add `ServerId::EditorContext` (id `"editor"`, label `"Editor context"`),
  extend `ALL` to 5, add `descriptors()` arm -> `editor_context::tools::descriptors()`. Update
  `tool_gate_tests.rs`.
- `features/mod.rs`: add `FeatureId::EditorContextMcp` (id `"editorContextMcp"`), extend `ALL` to 6,
  `label`/`description`/`default_enabled => true`. Update `features_tests.rs` incl.
  `every_feature_states_its_own_default`.

## D. src-tauri backend
- `state.rs`: add `editor_context: Mutex<Option<EditorContext>>` to `WorkspaceSlot`; accessors
  `record_editor_context(root, ctx)` (by explicit root, returns bool if slot still open) and
  `editor_context_for(root) -> Option<EditorContext>` (mirror `lsp_for_root` fallback via
  `dunce::canonicalize`). Add `#[cfg(windows)] editor_pipe` slot + `set_/published_` accessors
  mirroring `roslyn_pipe`.
- `src-tauri/src/editor_context/` (mod/pipe/registry/agent) mirroring `src-tauri/src/roslyn/`:
  process-global pipe started at `.setup`, withdrawn at exit; `answer_line` resolves
  `state.editor_context_for(request.workspace)` and calls `agent::answer` -> `render::*`.
- `src-tauri/src/mcp_editor.rs` runner (stdio server, client side) mirroring `mcp_roslyn.rs`:
  `is_mcp_editor_invocation`, tool-gate check, `choose_instance`, `exchange` over derived pipe,
  TOOL_TIMEOUT ~15s.
- `commands/editor_context_mcp.rs`: 5 install verbs (status/install_plan/install/uninstall_plan/
  uninstall) + `set_editor_context(root, ctx)` push command (no-op silently if feature off is
  enforced frontend-side; command just records).
- `lib.rs`: add `mod mcp_editor;` self-dispatch branch in `run()` (after `mcp_redis`); register new
  commands in `generate_handler!`; call `editor_context::start_listener` in `.setup`; withdraw at exit.

## E. Frontend
- `src/components/editorContextLogic.ts` (+ test) — pure: assemble `EditorContext` from
  (openFiles, activeFile, dirty/pinned sets, active editor's selection/cursor/viewport, recent list);
  maintain edit-recency (append on dirty/save, dedupe, cap); decide equality so we don't push
  identical context; cap selection text.
- `FileEditor.tsx` — add an `updateListener.of` that reports `{ selection.main, view.viewport }` via a
  new `onEditorContextChange` prop (debounced); reuse existing `visible` viewport ref. Only the editor
  matching `activeFile` (center) reports for MVP; docking multi-visible is a documented limitation.
- `RunView.tsx` — hold active editor's live selection/cursor/viewport; maintain recent-edited list;
  build `EditorContext` via the logic module; push via a debounced `api.setEditorContext(root, ctx)`
  **only when the feature is enabled** (read features once, like other gated surfaces).
- `src/ipc/api.ts` — `setEditorContext(root, ctx)` invoke + the 5 install wrappers (mirror
  `roslynMcp*`). `src/ipc/types.ts` — `EditorContext` mirror.
- Settings/Plugins: server appears in MCP-tools settings automatically via `ServerId::ALL`; add the
  install entry alongside the other MCP servers (mirror Roslyn's Settings wiring) and a Plugins/menu
  affordance if the others have one.

## F. Docs
- `pnpm docs:index` after adding files. Update `docs/reference/commands.md` (tracks `generate_handler!`).
- New guide `docs/guides/editor-context-mcp.md` (mirror `sql-mcp-server.md`/`roslyn-mcp-server.md`).

## Verification (run ALL — do not trust workflow self-reports)
- `cargo test -p cb-core editor_context::` and `cargo test -p cb-core` (Git Bash / sh on PATH).
- `cargo test -p cb-core` model key-pin + tool_gate + features tests.
- `cargo fmt --check`, `cargo clippy` (CB_GATE_FULL).
- `pnpm typecheck`, `pnpm test` (editorContextLogic).
- `pnpm docs:index` + `pnpm docs:check`.
- Manual: build release, `pnpm tauri dev`, open a repo, `cb-app.exe mcp-editor --workspace <root>`
  via an MCP client (or the install flow), confirm each tool reflects the live editor.
</content>
