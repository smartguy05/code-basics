# Notes / gotchas — Editor Context MCP

## Stage 5 — frontend (done 2026-09-14)

- **Two-part privacy gate, frontend side.** `RunView`'s push effect returns early
  when `editorContextEnabled` is false AND `FileEditor`'s `onEditorContextChange`
  is passed `undefined` while off (so a background scroll churns no state and
  pushes nothing). The backend `set_editor_context` does NOT gate — the pipe host
  re-checks the feature. So context can sit stale in `AppState` after the feature
  is switched off, and the pipe returning `Disabled` is what stops it being
  served. Do not add a "clear on disable" push; it is unnecessary and would race a
  legit push during a debounce window.

- **`editorContextEnabled` is threaded, not read here.** `WorkspaceTab` computes
  `featureEnabled(features, "editorContextMcp")` and passes it to `RunView` as a
  prop. `RunView` never reads the features store (it does not have it). Same
  pattern as `sqlEnabled`/`mcpEnabled` in `WorkspaceTab`.

- **Only the active workspace tab contributes a live position (MVP limitation).**
  `FileEditor.onEditorContextChange` reports `{cursor, selection, viewport}` for
  **workspace files only** (gated in `scheduleContextReport` on
  `source.kind === "workspace"`; a diff never reaches `FileEditor`, and a secrets
  file is outside the workspace). `RunView` tags each report with the reporting
  tab id (`editorLive = {id, position}`) and `buildEditorContext` uses it only
  when `id === activeFile` AND the active tab is a workspace file. **Split
  docking** can show two editors side by side; only the one matching `activeFile`
  contributes cursor/selection/viewport. A docked, non-active editor's caret is
  NOT reported. This is a documented MVP limitation — the push model is
  single-active-editor. If multi-visible is ever wanted, report per-visible-editor
  and pick by focus, not by `activeFile`.

- **Positions need no conversion.** `FileEditor.liveEditorPosition` reads
  `Line.number` (1-based) and `pos - line.from` (0-based UTF-16) straight off
  CodeMirror — exactly the wire convention (`usagesExtension` relies on the same).
  Selection text is capped by `editorContextLogic.capSelectionText`
  (`SELECTION_TEXT_CAP = 4096` code points, surrogate-safe via `Array.from`), NOT
  sliced in `FileEditor`, so a whole-file selection does not push megabytes.

- **All decisions in `components/editorContextLogic.ts` (+ test, node env, no
  DOM):** `buildEditorContext` (workspace-only filtering, path normalisation to
  forward slashes, live-gating on active-file), `pushRecent` (dedupe-to-front,
  cap 20, same-ref on no-op), `capSelectionText`, `editorContextEqual` (structural
  compare so an unrelated re-render does not re-push). `FileEditor`/`RunView` are
  shells that gather inputs and call these. 30 new logic tests.

- **Recency = "became dirty".** `RunView.setFileDirty` calls `pushRecent` when a
  **workspace** file transitions to dirty (an edit). `pushRecent` returns the same
  array reference when the path is already first, so re-editing the front file
  does not re-render. Secrets/diff ids are not paths and are skipped.

- **Double debounce.** `FileEditor` debounces the report at 150ms
  (`EDITOR_CONTEXT_DEBOUNCE_MS`); `RunView`'s push effect debounces the IPC at
  200ms and skips it entirely when the built context equals the last pushed one.

- **Plugin wiring mirrors Roslyn, but feature-gated.** `EditorMcpPanel.tsx` is the
  twin of `RoslynMcpPanel.tsx` (reuses `mcpServerLogic` wholesale;
  `planOutcome(.., "Editor context MCP server")`). Opened via the Plugins menu
  (`plugin.editorMcp` → `WorkspaceTab.openEditorMcp` on the handle → `EditorMcpPanel`),
  registered as a command only while the feature is on, and an effect closes the
  modal when the feature is switched off. `pluginMenuLogic` entry has
  `feature: "editorContextMcp"` (NOT always-on like Roslyn — with the feature off
  there is nothing to install against, so the row is omitted). `featuresLogic`
  `FeatureKey` gained `editorContextMcp`; `shortcutLogic` gained the
  `PLUGIN_LABELS` entry + the `plugin.editorMcp` command;
  `McpToolsPage.SERVER_FEATURE` gained `editor: "editorContextMcp"` so the MCP
  tools settings page shows a server toggle bound to the feature (the server
  itself already appears via `ServerId::ALL`).

- **api.ts wrappers:** `setEditorContext(root, ctx)` + `editorMcpStatus` /
  `editorMcpInstallPlan` / `installEditorMcp` / `editorMcpUninstallPlan` /
  `uninstallEditorMcp` (mirror `roslynMcp*`). Backend command names differ from
  Roslyn's install verbs: `editor_mcp_server_status` /
  `editor_mcp_server_install_plan` / `install_editor_mcp_server` /
  `editor_mcp_server_uninstall_plan` / `uninstall_editor_mcp_server`.

- **`noUncheckedIndexedAccess` is on.** Array index access is `T | undefined`;
  `editorContextEqual` guards `openFiles[i]`/`recentFiles[i]` and the test uses
  `?.` on `ctx.openFiles[0]`.

## Stage 3

- **The feature is re-checked in the pipe host every call — deliberately.** The
  frontend stops pushing while the feature is off, but a context pushed BEFORE it
  was switched off still sits in `AppState`. Serving that leaks state the user
  said to stop sharing. So `pipe.rs::answer_line` reads `feature_enabled()` fresh
  and hands it to `agent::answer`, which returns `Disabled` (not the stale data)
  when off. The `set_editor_context` command does NOT gate on the feature (note 4)
  — that would drop a legit push during a debounce race; the gate is the frontend
  not calling + the pipe re-check.

- **`agent::answer` is a pure free fn on plain data** (unlike roslyn's, which
  needs an LspHandle and is untestable). So the two app-side decisions
  (feature-off ⇒ Disabled, none ⇒ NoContext) ARE unit-tested in
  `agent_tests.rs`. Keep any new decision here (not inline in `pipe.rs`) so it
  stays testable — CLAUDE.md's command-body rule.

- **NoWorkspace is produced in TWO places, both `EditorRefusal::NoWorkspace`:**
  the shim (`mcp_editor.rs::call_tool`, before reading the registry) and the pipe
  host (`pipe.rs::answer_line`, if `request.workspace` is blank). Shim is the
  practical path (it can't `choose_instance` without a workspace); the host guard
  is belt-and-braces. `EditorRefusal::NoWorkspace.answer()` builds the ToolAnswer
  (the `.answer()` helper Stage 2 added).

- **No Peer plumbing.** Roslyn's pipe carries a `Peer` (pid+program) for a future
  banner; the editor host drops it — `agent::answer` takes only data, no banner is
  planned, and keeping it would add untested Win32 (`client_process_id`,
  `process_image_name`) for nothing. If a banner is ever wanted, copy roslyn's
  `describe_peer`/`peer_label` back in.

- **`editor_context_for`/`record_editor_context` use a canonicalize fallback**
  (`state.rs::slot_for_root`) for BOTH read and write, because the root comes from
  outside the backend (frontend push root; registry-published
  `Workspace.root.display()` on the pipe side) while the slot map is keyed by the
  canonical root — same reason `lsp_for_root` has the fallback.

- **`set_editor_pipe`/`editor_pipe` mirror `roslyn_pipe`** exactly: replace-then-
  stop-AFTER-dropping-the-guard (stop() aborts a task; never hold a
  `std::sync::Mutex` across it). Process-global, opened once at `.setup`, withdrawn
  at Exit; republished on workspace open/activate/close beside the roslyn republish.

- **TOOL_TIMEOUT is 15s** (roslyn is 60s): reading pushed state is a map lookup,
  so anything slow is a stuck app, not real work.

- **Stage 5 (frontend) contract:** call `api.setEditorContext(root, ctx)`
  (command `set_editor_context`, args `{root, ctx}`) debounced, ONLY while the
  `editorContextMcp` feature is enabled. Install verbs are
  `editor_mcp_server_status`/`_install_plan`/`_uninstall_plan`,
  `install_editor_mcp_server`/`uninstall_editor_mcp_server` (mirror the
  `roslyn_mcp_*` wrappers). Cap `selection.text` frontend-side (~4 KiB).

- **Stage 6 (docs):** `pnpm docs:index` after adding files; update
  `docs/reference/commands.md` (tracks `generate_handler!` — 6 new commands);
  add `docs/guides/editor-context-mcp.md` (mirror roslyn/sql guides). NOT done
  in Stage 3.

## Stage 2

- **`EditorRefusal::answer()` is new vs the Roslyn twin.** `render.rs` returns a
  refusal directly for the no-active-file case (`get_active_file`/
  `get_selection`), so `answer.rs` grows an `answer(self) -> ToolAnswer` helper
  that `RoslynRefusal` does not have (roslyn's render builds `ToolAnswer::refused`
  inline). `answer.rs` therefore `use super::wire::ToolAnswer;`.

- **render vs answer split — the important line.** NoActiveFile is a REFUSAL
  (ok:false, code `noActiveFile`) because the tool has nothing to point at. But
  an empty selection, no open tabs and no recent files are GENUINE data answers
  (ok:true, "...complete answer, not a truncated one"), the way a Ready-but-empty
  Roslyn result is data. Do not "fix" those into refusals — pinned by tests
  (`selection_with_an_active_file_but_no_selection_is_a_genuine_empty_not_a_refusal`
  etc.).

- **The editor tools take NO arguments.** Unlike Roslyn (path + position/symbol),
  every editor tool reads live state directly. `tools::parse_call` ignores its
  `arguments` param entirely — spurious args are NOT a shape error (inventing one
  over an ignored field would be a lie). Schemas are empty-object
  (`properties:{}`, `required:[]`).

- **Stage 3+ contract to honor:** the app-side agent layer (src-tauri) produces
  the three pre-render refusals — NoWorkspace (no --workspace), Disabled (feature
  off), NoContext (feature on but `editor_context_for(ws)` is None) — BEFORE
  calling `render::*`. render only ever gets a `&model::EditorContext` and only
  ever produces NoActiveFile (+ data). Wire it: feature-off ⇒ Disabled;
  slot None ⇒ NoContext; slot Some(ctx) ⇒ render.

- **Registry/instances mirror Roslyn exactly** but with editor-namespaced
  strings: file `editor-instances.json`, env `CB_EDITOR_INSTANCES_PATH`, pipe
  `\\.\pipe\code-basics.editor.<pid>`, PipeNameMismatch code
  `editor_registry_tampered`, wire METHOD `editor.tool`. PROTOCOL_VERSION=1.

- **clippy:** the sole `cargo clippy -p cb-core --lib` warning
  (`redis/dsn.rs` manual split_once) is PRE-EXISTING and unrelated — do not
  attribute it to editor_context.

## Stage 1

- **`ServerId::descriptors()` must stay total and compile now.** Stage 2 will
  replace the `ServerId::EditorContext => Vec::new()` arm with
  `crate::editor_context::tools::descriptors()`. Until then the settings MCP-tools
  page shows the "Editor context" server with an **empty** tool list — that is
  expected, not a bug. Grep for `TODO(editor-context stage 2)` to find it.

- **Nullable vs optional on the wire.** The optional `EditorContext` fields
  deliberately have NO `skip_serializing_if`, unlike most of `model.rs`. They
  serialise as `null` when `None`, and `types.ts` mirrors them as `X | null`.
  Rationale: the shim (stage 2 `render.rs`) must distinguish "no active file"
  (an honest abstain) from a malformed/absent payload, and a key that is always
  present is the simplest contract for a state that is *pushed* frontend→backend
  and read back. Pinned by `an_empty_editor_context_keeps_its_null_keys`.

- **Position convention** (app-wide, see work-item.md / api.ts:1286-1302):
  1-based line, 0-based UTF-16 character. `EditorCursor.line`,
  `EditorViewport.first/lastVisibleLine`, `EditorSelection.*Line` are 1-based;
  `*Character` are 0-based UTF-16. Documented in the struct doc-comments so
  stage 2 (`positions`/`render`) and the frontend logic module use one rule.

- **Test filter gotcha.** `cargo test -p cb-core editor_context` (substring) is
  the way to catch all three new tests; a prefix like `model::tests::editor`
  only matches the one test whose name starts with `editor`. Also note only ONE
  positional filter is accepted per `cargo test` invocation.

- **Adding a `FeatureId` is a two-installer contract.** The
  `features::store::tests` read the actual NSIS script and Linux seed JSON off
  disk and fail unless the new id (its stable id, label, and verbatim
  description) is present in both. There is no way to add a feature and keep
  `cargo test -p cb-core` green without editing
  `src-tauri/installer/windows/installer.nsi` and
  `src-tauri/resources/linux/features.json`. The NSIS var name is derived as
  `$Feature<HeadUpper><rest>` from the id (so `editorContextMcp` →
  `$FeatureEditorContextMcp`). Description text must be byte-identical between
  `features/mod.rs` and the .nsi (keep it ASCII).

- Full-suite reminder from CLAUDE.md: run cargo from Git Bash (sh on PATH) or
  the `process::` tests fail/hang; concurrent cargo invocations serialise on the
  shared target lock, so a stall in `process::` late in the run is the lock, not
  a break — re-run before reporting broken.
