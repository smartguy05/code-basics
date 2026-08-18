# Completed — Enhancements: Add Instructions / Run Agent

## What shipped
Renamed the Enhancements submenus and turned prompts into runnable agents:
- **Instructions → "Add Instructions"**, **Prompts → "Run Agent"** (headers in `MenuBar.tsx`).
- Clicking a prompt now **runs it as an agent** in the shared floating panel
  (the old adversarial-review panel, generalized) instead of copying to the
  clipboard.
- **Per-run posture toggle** (Read-only / Allow edits) in the panel.
- **Run-once prompts** (`once: true` front matter): recorded per workspace on a
  successful finish, badged in the menu, and confirmed before re-running.

## Key decisions
- Reused/generalized `ReviewPanel` (one panel, one supervisor slot `review:current`)
  rather than cloning. New props: `initialPromptId`, `initialMode`, `title`.
- Backend `agent_args` gained an `AgentMode {ReadOnly, Edit}`. Edit posture:
  - Claude: `--permission-mode bypassPermissions` (VERIFIED — `acceptEdits`
    still prompts for non-file Bash and would hang since supervisor closes stdin).
  - Codex: `--sandbox workspace-write`.
- `start_review` gained an optional `mode` param (default read-only; unknown refused).
- Run-once counts **on successful exit** only; panel calls `mark_agent_run` from
  the `exited & success & !cancelled` branch.
- Run state: `.code-basics/agent-runs.json` (epoch-millis stamps), pure module
  `cb_core::enhancements::runs`, added to `config::IGNORED` (gitignored).

## Files touched
- Core: `review.rs` (+tests), `enhancements/mod.rs` (`once` on Template/PromptInfo),
  `enhancements/runs.rs` + `runs_tests.rs` (new), `config.rs` (IGNORED += RUNS_FILE).
- Bridge: `commands/review.rs` (mode), `commands/enhancements.rs`
  (`agent_runs`/`mark_agent_run`), `lib.rs` (register).
- Frontend: `ipc/api.ts` (+`AgentMode`, `agentRuns`, `markAgentRun`, mode arg),
  `ipc/types.ts` (`PromptInfo.once`, `PromptRun`/`PromptRuns`), `ReviewPanel.tsx`,
  `App.tsx` (`reviewOpen` → `agentPanel` request slot), `MenuBar.tsx`,
  `enhancementsLogic.ts` (+ run-once helpers; removed `copyFeedback`) + test.
- Docs: commands.md, guides/instruction-enhancements.md, frontend.md, README, CLAUDE.md, INDEX.

## Verified
- `cargo test -p cb-core --lib` → 2118 passed.
- `pnpm typecheck` clean; `pnpm test` → 778 passed.
- `cargo check -p cb-app` OK; `pnpm docs:check` OK.
- Not yet exercised live (`pnpm tauri dev`) — needs a manual pass of the run-once
  badge/confirm and the edit-mode agent actually writing a file.

## Gotcha (worth remembering)
`cargo fmt` on the whole workspace reformatted ~30 pre-existing files that were
committed unformatted on this branch (erosion/behavioral/git/intents). Had to
`git checkout HEAD --` them to keep the diff scoped. Prefer `rustfmt --check` on
just the files you touched, or `cargo fmt -- <paths>`, on this branch.
