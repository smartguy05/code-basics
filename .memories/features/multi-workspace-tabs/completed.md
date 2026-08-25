# Completed — multi-workspace-tabs

## Backend (B0–B5) — DONE, 35 cb-app tests green, `cargo check`/`fmt` clean
- **state.rs rewritten** to shard `AppState` into `workspaces: Mutex<HashMap<PathBuf, Arc<WorkspaceSlot>>>` + `active: Mutex<Option<PathBuf>>`. Global `supervisor` (review/behavioral) and `pty` (terminals) stay on AppState. Per-slot `WorkspaceSlot { root, workspace, supervisor, last_test_run, last_inspect, symbols, symbols_build, lsp, lsp_generation }`.
- Key design that minimized churn: **kept method signatures compatible** — `workspace()`, `workspace_root()`, `symbols()`, `symbols_building()`, `previous_test_run()`, `previous_inspect()`, `clear_inspect()`, `lsp()`, `begin_lsp_session()`, `record_lsp_session()`, `begin_symbols_build()` all resolve the ACTIVE slot; record methods (`record_test_run(root,..)`, `record_inspect(root,..)`, `record_symbols(index by root)`, `update_symbols(root,..)`) look up by explicit root (correctly attributes a background completion). So lsp.rs, symbols.rs, files.rs, architecture.rs, terminal.rs, behavioral.rs, review.rs UNCHANGED.
- `set_workspace` = upsert-and-activate; **NO teardown on switch** (deleted the old changed-branch). Teardown only in `close`.
- New methods: `active_slot()`, `slot(root)`, `set_active(root)`, `close(root)->(removed slot, new active)`, `open_workspaces()`, `active_supervisor()`, `active_workspace_opt()`.
- Tests live in `src-tauri/src/state_tests.rs` (via `#[path]`), rewritten for new semantics (open keeps prior live, record-by-root, config_id collision fixed by per-slot supervisor, close teardown, LSP per-slot).
- **Per-slot Supervisor** fixes the root-relative config_id collision. run.rs run/tests/build/cancel/running use the slot supervisor; `cancel_run`/`running_ids` gained optional `root` (background control). inspect.rs uses `active_supervisor()`.
- **New commands** (workspace.rs): `list_open_workspaces`, `set_active_workspace(root)`, `close_workspace(root)` — registered in lib.rs, documented in docs/reference/commands.md. `current_workspace` now returns active. Canonicalization: `scan`/`scan_with` already dunce-canonicalize every root; incoming root strings canonicalized too (B0 satisfied).

## Frontend — partial
- **F1 DONE**: `src/components/workspaceTabsLogic.ts` + `.test.ts` (addOpenWorkspace dedupe, closeOpenWorkspace neighbour selection, tabLabels disambiguation). 10 tests green, typecheck clean.
- **F2 DONE**: `src/ipc/api.ts` wrappers `listOpenWorkspaces`, `setActiveWorkspace`, `closeWorkspace` + updated `openWorkspace`/`currentWorkspace` docs.

## Frontend F4/F6/F7/F8 — DONE (structural), typecheck clean + 904 tests green
- **F4 `<WorkspaceTab>`** (`src/components/WorkspaceTab.tsx`): owns everything
  per-workspace (inner tab, agentPanel, behavioralPanel/report, inspectRequest,
  openRequest/selectRequest, terminals, showSetup+effect, the 6 views,
  SearchEverywhere, SetupPrompt, ReviewPanel, BehavioralPanel). Wrapped in
  `<div className="workspace-tab" hidden={!active}>` — backgrounding = display:none,
  never unmount, so processes/terminals/LSP stay live. Exposes a
  `WorkspaceTabHandle` (openTerminal/openRunAgent/openReview/openNoteInAgent)
  registered with App via `onRegister(root, handle|null)`.
- **F6 App restructure**: App now holds `openWorkspaces: Workspace[]` +
  `activeRoot`, renders the global titlebar (MenuBar, active workspace
  name/branch/#run-config-slot/project-count, Notes/+Terminal/Rescan/Open), the
  `.ws-tabs` open-codebases strip (label+×+"+"), and one `<WorkspaceTab>` per
  root. Load restores from `listOpenWorkspaces()`+`currentWorkspace()`.
  Global Notes panel stays in App; its send-to-agent + titlebar actions route to
  the active tab's handle.
- **F7 handshake**: `activateWorkspace` awaits `setActiveWorkspace` BEFORE
  `setActiveRoot` (so newly-foregrounded views never query the previous ws).
  `closeWorkspace` calls backend close, computes neighbour via
  `closeOpenWorkspace`, realigns backend active to it. BranchMenu keyed by
  activeRoot; RunConfigMenu gated on `foreground` (new RunView prop) so only the
  active tab portals into the single `#run-config-slot`.
- **F8 (partial)**: SearchEverywhere key listener + conditional views
  (Changes/History/Architecture) gated on `active`. RunView/Tests/Inspect reads
  are event/visibility-driven (never timers) and mount when the ws is active, so
  no background active-pointer polling. cancel_run/running_ids already accept
  optional root (backend).
- CSS: `.workspace-tab`(+`[hidden]`) and `.ws-tabs`/`.ws-tab*` in styles.css.

## Verified
- Backend: 35 cb-app tests, `cargo check`/`fmt` clean.
- Frontend: typecheck clean, `pnpm test` 904 passing.

## Known limitations / remaining (see notes.md)
- **Dirty-close confirm NOT wired** — closing a tab discards its unsaved editor
  buffers silently. Needs onDirtyChange threaded RunView→WorkspaceTab→App.
- **Background-exit dump lookup** targets the active ws (inspectStatus/
  inspectRunDump have no root param) — narrow, read-only.
- Not yet done: `pnpm docs:index`/`docs:check`, manual `pnpm tauri dev` e2e,
  runtime layout check of the tab strip / titlebar.
</content>
