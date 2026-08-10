# Completed

## cb-core

- `intents/reject.rs` + `reject_tests.rs` — comment-syntax table, reason
  sanitising and wrapping, marker block, anchor translation, bottom-up
  insertion with replace-not-stack, `iso_date`, `reject_file`,
  `FileRejection`, `RejectSummary`. **27 unit tests.**
- `intents/guard.rs` + `guard_tests.rs` — the `pre-commit` script,
  marker-bounded merge/refresh/removal, `hook_path`, `ensure_executable`.
  **10 unit tests.**
- `git/repo.rs` — added `Repo::hooks_dir()`, honouring `core.hooksPath`
  (absolute or relative to the working tree).
- `intents/providers/mod.rs` — `guard_write()` shared helper.
- `intents/providers/{claude_code,codex}.rs` — both `install_plan`s append the
  guard write.
- `intents/providers/instructions.rs` — `SECTION` gained "Rejected changes";
  one new test pinning that the section names the real token and both halves of
  the contract.
- `tests/reject_markers.rs` — **11 end-to-end tests** against real
  repositories: revert + note placement, per-hunk rejection with a kept hunk,
  unmarked JSON, and the hook actually refusing/allowing `git commit` including
  the `CB_ALLOW_REJECTED` escape hatch and the mention-is-not-a-note case.

## src-tauri

- `commands/intents.rs` — `reject_intent_group`; `lines_for` widened to return
  `Vec<GroupFile>` (rejecting needs hunk indices) with `stage_intent_group` and
  `revert_intent_group` adapted; `enable_intent_capture` now makes the hook
  executable.
- `lib.rs` — registered in `generate_handler!`.

## Frontend

- `ipc/types.ts` — `RejectSummary`.
- `ipc/api.ts` — `rejectIntentGroup`.
- `components/intentPanelLogic.ts` — `canRejectInMode`, `rejectReasonError`,
  `rejectFeedback`. **8 new vitest cases.**
- `components/IntentPanel.tsx` — **Reject group…** and per-file **Reject…**,
  inline reason prompt (Enter confirms, Escape cancels), disabled in the staged
  view with a title saying why; outcome reported in the panel's feedback line.
- `views/ChangesView.tsx` — `rejectGroup`, `mode` passed to the panel.
- `styles.css` — `.reject-prompt`.

## Docs

- `docs/guides/agent-intent-capture.md` — "Rejecting a change" + "The commit
  guard".
- `docs/reference/commands.md`, `docs/reference/configuration.md` updated.
- `docs/INDEX.md` regenerated (`pnpm docs:index`).
- This repo's own `CLAUDE.md` marked section brought in line with the new
  `SECTION`, since the repo dogfoods its own installer — `refreshed()` is now a
  no-op there rather than pending a "Re-apply setup…".

## Quality gate at completion

- `cargo test -p cb-core` — **1095 pass, 0 fail** (1038 lib + 44
  git_operations + 2 intent_attribution + 11 reject_markers).
- `pnpm test` — 229 pass (8 new). `pnpm typecheck` — clean.
- `cargo fmt --check` — clean. `cargo clippy --workspace --all-targets` — the
  only two warnings left are pre-existing (`importers/rider.rs:65`,
  `workspace.rs:967`); all three introduced here were fixed.
- `cargo build` — links `cb-app.exe` successfully.
- Coverage: `reject.rs` 92.13% lines, `guard.rs` 96.43%; workspace total 86.31%
  lines, gate (70) passes. Frontend coverage 99.55% lines.
- `pnpm docs:check` — 20 files, all under 500 lines, all links resolve.

## Not verified

The GUI click-through. Everything below the UI is covered by tests against real
repositories and the real `git`, but nobody has yet pressed **Reject group…**
in the running app. See todos.md.
