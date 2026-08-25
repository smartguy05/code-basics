# TODOs — multi-workspace-tabs

## Frontend (test-first, pure logic first)
- [x] F1: `workspaceTabsLogic.ts` + `.test.ts` (10 tests green)
- [x] F2: `api.ts` wrappers — setActiveWorkspace / closeWorkspace / listOpenWorkspaces
- [x] F3: prop-ify ChangesView + SearchEverywhere (workspace prop; 903 tests green)
- [x] F4: extract `<WorkspaceTab>` (WorkspaceTab.tsx; handle registry)
- [x] F5: terminal scoping — TerminalDescriptor.cwd, terminalLayoutKey(root) (11 tests)
- [x] F6: multi-open in App — openWorkspaces + activeRoot + .ws-tabs strip
- [x] F7: handshake (activateWorkspace awaits setActiveWorkspace; closeWorkspace →
      neighbour realign); BranchMenu keyed activeRoot; RunConfigMenu gated on foreground
- [~] F8: background-poll audit done (views never poll on timers; SearchEverywhere +
      conditional views gated on active). Dirty-close confirm NOT wired (deferred).

## Remaining (needs user / follow-up)
- Runtime e2e via `pnpm tauri dev` (GUI — user verifies tab strip + titlebar layout).
- Dirty-close confirmation (data-loss guard) — thread onDirtyChange RunView→App.
- Optional: root param on inspectStatus/inspectRunDump for background-exit dumps.
- cargo test -p cb-core (unchanged by this work) + coverage gates.

### F4/F6/F7 notes for next session
- The big App.tsx restructure. Titlebar currently mixes GLOBAL chrome (MenuBar
  File menu, Open, Notes button) with PER-WORKSPACE chrome (workspace name/root,
  BranchMenu, #run-config-slot portal, project count, +Terminal, Rescan). Decide
  the split: workspace tab strip is new global chrome; per-workspace chrome moves
  into WorkspaceTab's header (or is fed the active workspace).
- MenuBar onRunAgent/onOpenReview open the agentPanel, which moves INTO
  WorkspaceTab — so MenuBar (global) must reach the active tab's handlers.
- Move into WorkspaceTab: inner `tab`, agentPanel, behavioralPanel/report,
  inspectRequest, openRequest/selectRequest+requestToken, showSetup+effect,
  terminals (now with cwd=workspace.root), the 6 views, SearchEverywhere,
  SetupPrompt, ReviewPanel, BehavioralPanel, BranchMenu, run-config-slot.
- Keep mounted-but-hidden for ALL open roots (Run/Tests/Objects + terminals);
  Changes/History/Architecture stay conditional and only in the foreground tab.
- cancel_run/running_ids already accept optional root (backend) for F8 background
  control; api.ts wrappers still call them without root.

## Backend (test-first, each phase failing test then implement)
- [ ] B0: canonicalize root key (two spellings → one slot)
- [ ] B1: WorkspaceSlot + sharded AppState, accessors preserved; per-slot
      Supervisor + last_test_run; rewrite inverted state.rs tests
- [ ] B2: commands — open_workspace inserts, set_active_workspace, close_workspace,
      list_open_workspaces; delete teardown-on-switch branch; lib.rs + docs
- [ ] B3: process control per slot; optional root on cancel_run/running_ids
- [ ] B4: LSP per slot — explicit-root ensure_session/spawn_session, per-slot
      generation, teardown-on-close, reborn-slot nonce
- [ ] B5: symbols per slot
- [ ] B6: audit ~104 commands for a second mid-body state.workspace() (inspect.rs:162,168)

## Verification
- [ ] cargo test -p cb-core (Git Bash), fmt, clippy, coverage ≥70
- [ ] pnpm test + typecheck + coverage
- [ ] pnpm docs:index + docs:check
- [ ] manual pnpm tauri dev end-to-end (two repos, live background, close)
