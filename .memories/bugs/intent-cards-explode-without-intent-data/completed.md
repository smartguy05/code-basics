# Completed — 2026-08-07

All phases of plan.md done in one session (Workflow orchestration, 4 Opus agents + orchestrator follow-up).

## Code (files touched)
- `crates/core/src/git/grouping.rs` + `grouping_tests.rs` — declaration_name colon rule
  (binding not type) + NOT_A_SYMBOL word rejection (import/use lines with a DECLARING
  keyword, e.g. `import type`, `pub use`, no longer name a card "import"/"use");
  header_can_name_a_symbol gate on the permissive header fallback; bare-symbol labels
  (badge carries the verb); Formatting label -> "Whitespace only"; collapse_singletons
  post-pass (>=2 one-hunk symbol cards in a file merge into `other:{path}` /
  "Several changes in {file}"). 12 new tests, all watched fail first.
- `crates/core/src/intents/providers/hooks_json.rs` — command_line/commands_for/plan_merge
  take Option<&Path>; --workspace emitted only for project scope; new pinned_workspace /
  pinned_elsewhere / pinned_caveat.
- `providers/claude_code.rs`, `providers/codex.rs` — user-scope install plans un-pinned;
  status() demotes a user hook pinned elsewhere to capture:None + caveat.
- `crates/core/src/intents/hook.rs` — new resolve_enabled_root (ancestor walk from payload
  cwd to first enabled workspace); `src-tauri/src/recorder.rs` uses it. ~10 new tests.
- NEW `src/components/intentPanelLogic.ts` + `.test.ts` (11 tests) — intentDataHint /
  importFeedback; `IntentPanel.tsx` banner (Enable capture / Import past sessions (n) +
  feedback); `ChangesView.tsx` importHistory returns the count.
- `docs/guides/agent-intent-capture.md` updated; `docs/INDEX.md` regenerated.

## Local setup repair (user-approved)
- `~/.claude/settings.json`: removed `--workspace ...ONEflight` from both record-intent
  hooks (backup: settings.json.bak-intent-pin).
- Created `.code-basics/intents/`; appended the marked Intent section to repo CLAUDE.md.
- Imported past sessions via a throwaway ignored test (deleted after): 692 records,
  226 labels -> `.code-basics/intents/`.
- Rebuilt `target/debug/cb-app.exe` (hook binary) with the new resolve logic.

## Follow-up: Intent instruction wording clarified (same day)
`instructions::SECTION` rewritten (+ CLAUDE.md block re-synced, guide updated): the old
text showed both forms in one fence (read as "output both"), said "one line per distinct
change" with the plain form (but `label_for` uses only the FIRST plain label per turn),
never said paths must be workspace-relative, and hid the comma-separated multi-path form.
New tests pin all of that: `the_request_warns_that_extra_plain_lines_are_ignored`, updated
`the_request_describes_both_the_plain_and_file_scoped_forms` (multi-path + "workspace-
relative") and `the_requested_form_is_one_the_hook_can_parse` (round-trips two paths).

## Follow-up 2: Re-apply setup (same day)
- `instructions.rs`: new `END_MARKER`; `planned_write` now REWRITES the marked span in
  place when the section exists but is stale (abstains when current, or when the end
  marker is missing — never guesses the span). Tests:
  `an_out_of_date_section_is_rewritten_in_place`, `a_marker_without_its_end_is_left_alone`
  (watched fail first), plus provider guard `re_enabling_refreshes_a_stale_instruction_section`.
- Hook side needed nothing: `merge_into` retain-then-extend already replaces marker
  entries, and `enable_intent_capture` re-derives + re-applies the whole plan.
- `IntentPanel.tsx` CaptureSetup: "Re-apply setup…" button when capture is on, previewing
  at the currently-installed scope through the same confirm dialog. No IPC change.
- Caveat wording in both providers: "added or brought up to date" (was "appended").
- Guide documents the button + rewrite semantics. All gates green (1046 Rust tests,
  217 vitest, fmt, docs:index/check; clippy only pre-existing warnings).

## Follow-up 3: per-file navigation inside a card (same day)
User decisions: hide other groups' hunks when viewing a card's file; per-file
stage/revert too; keep auto-open of the first file.
- `diffLogic.ts` `onlyHunks(diff, hunks)` (4 vitest tests, watched fail first) — the
  scoped diff; tolerant of stale indices.
- `ChangesView.tsx`: `groupHunks` state; `selectGroupFile`; `shownDiff` feeds the pane
  and the whole-file buttons ("Revert file" becomes "Revert shown" when scoped);
  cleared by `openFile` and on switching back to the Files view.
- `IntentPanel.tsx` GroupCard: clickable file rows (path + per-file hunk/line counts,
  selected highlight, per-file Stage/Revert on hover for multi-file groups). CSS
  `.group-file` uses existing tokens (--border hover, --accent-dim selected).
- Backend: `stage_intent_group`/`revert_intent_group` take `path: Option<String>`
  filtering `lines_for`'s result; api.ts wrappers optional param; commands.md updated.
- Gates: typecheck, 221 vitest, cargo check workspace, fmt, docs:index/check.

## Verification
- `cargo test -p cb-core`: all green (994 lib + integration). `cargo fmt --check` clean;
  clippy: only the 2 pre-existing warnings (workspace.rs cmp_owned, junit while-let).
- `pnpm typecheck` clean; `pnpm coverage` 99.5% lines (217 tests).
- Real-repo diagnostic (`report_attribution_against_this_repository --ignored`):
  before: 133 one-hunk "New X" cards, 0 intent. After: 62 hunks -> 31 groups, 2 Intent
  cards, one of them recorded LIVE by the repaired hooks during this very session.
- docs:index + docs:check pass.
