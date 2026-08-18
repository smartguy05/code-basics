# Completed — stash-management

## Backend (cb-core + src-tauri)
- `git/repo.rs`: added `StashEntry` wire struct (index/id/message/branch/time);
  `stash_list` (via `stash_foreach` + commit lookup for time), `stash_apply`,
  `stash_drop`, `stash_clear`; changed `stash_pop` to take an `index`.
  `parse_stash_branch` free fn parses "On <b>:" / "WIP on <b>:" (abstains to None).
- `git/mod.rs`: re-export `StashEntry`.
- `commands/git.rs`: `git_stash_list/apply/drop/clear`; indexed `git_stash_pop`.
- `lib.rs`: registered the 4 new commands.
- Preview reuses existing `git_commit_diff` / `git_commit_file_contents` (a stash IS a commit).

## Frontend
- `ipc/types.ts`: `StashEntry` interface. `ipc/api.ts`: gitStashList/Apply/Drop/Clear,
  indexed gitStashPop.
- `components/stashLogic.ts` (+ `.test.ts`): `stashSummary`, re-export `formatTime`.
- `components/StashPanel.tsx`: self-contained list + read-only DiffView preview +
  Create / Apply / Pop / Drop / Clear all.
- `views/ChangesView.tsx`: Grouping union gains "stashes"; segmented toggle extracted to
  `groupingToggle` const with a third "Stashes" button; short-circuits to `<StashPanel>`.
- `views/HistoryView.tsx`: removed the two old sidebar stash buttons (consolidated).

## Docs (full pass)
- `docs/reference/commands.md` — the 4 new stash commands + indexed pop.
- `docs/architecture/core-crate.md` — git bullet lists stash list/save/apply/pop/drop/clear.
- `docs/architecture/frontend.md` — ChangesView line notes Files/Intent/Stashes toggle.
- `docs/getting-started/using-the-app.md` — Changes section rewritten: three-view toggle,
  Stashes panel actions, Intent staged/partial badges. Removed "stash save/pop" line.
- `docs/guides/agent-intent-capture.md` — toggle now "Files, Intent, Stashes"; staged-badge note.
- `README.md` — Changes bullet (Files/Intent/Stashes + stash manager); removed stash from History bullet.
- `CLAUDE.md` — git core bullet (stash-is-a-commit preview) + frontend bullet (three-view toggle).
- INDEX regenerated; `pnpm docs:check` passes (22 files, <500 lines, links resolve).
- No AGENTS.md at repo root (enhancements would create/splice one; nothing to update).

## Follow-ups (same branch)
- IntentPanel file-row hover: full path now titles the whole `.group-file` row
  (nested title on inner span was unreliable over the row's own title).
- Intent view staged/unstaged visibility: `intentPanelLogic.stagedState` /
  `groupStagedState` + a `StageTag` pill (staged/partial) on each card headline
  and file row, fed by `statusFiles` from ChangesView. `.stage-tag` CSS in
  styles.css. Answers "see what's staged" without a Files-mode Staged section.

## Verification (all green)
- `cargo test -p cb-core` 1962 pass; new 6 stash integration + 3 unit tests.
- `pnpm test` 662 pass; `pnpm typecheck` clean; `pnpm build` ok; docs:check ok.
- clippy/fmt clean for touched files (2 pre-existing warnings in rider.rs/workspace.rs).
- NOT yet done: live `pnpm tauri dev` smoke test.
