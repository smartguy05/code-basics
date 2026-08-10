# Plan

Built bottom-up, tests first at each layer.

1. **`crates/core/src/intents/reject.rs`** — the decision layer.
   `comment_prefix`, `sanitise_reason`, `marker_block`, `is_marker_line`,
   `anchors`, `insert_markers`, `iso_date`, plus the `reject_file`
   orchestration and `FileRejection` / `RejectSummary`.
2. **`crates/core/src/intents/guard.rs`** — the `pre-commit` guard.
   `block`, `hook_path`, `plan_for`, `planned_write`, `is_installed`,
   `plan_removal`, `ensure_executable`. Reuses `providers::PlannedWrite` and
   `apply_plan` so the hook is previewed and backed up like every other write.
3. **`Repo::hooks_dir()`** — new accessor honouring `core.hooksPath`.
4. **Wiring** — `providers::guard_write()` shared helper, appended to both
   providers' `install_plan`; `enable_intent_capture` calls
   `guard::ensure_executable` after `apply_plan`.
5. **Instruction section** — `instructions.rs::SECTION` gained a "Rejected
   changes" subsection, so `refreshed()` keeps it current automatically.
6. **Command** — `reject_intent_group`; `lines_for` widened to return
   `Vec<GroupFile>` because rejecting needs hunk indices.
7. **Frontend** — `RejectSummary` type, `rejectIntentGroup` wrapper,
   `canRejectInMode` / `rejectReasonError` / `rejectFeedback` in
   `intentPanelLogic.ts`, reject buttons + inline reason prompt in
   `IntentPanel.tsx`, `rejectGroup` in `ChangesView.tsx`.
8. **Docs** — the guide, `commands.md`, `configuration.md`, `INDEX.md`.

## Deviations from the original plan

- **No `PlannedWrite.executable` field.** The plan had one, which would have
  been an IPC type change (`types.ts` + the key-pinning test). The mode bit is
  not something the user needs to preview, so `guard::ensure_executable` is
  called after `apply_plan` instead. One fewer type crossing IPC.
- **The guard matches the token *followed by a date*, not the bare token.**
  Found while writing the integration test: `reject.rs` defines the token and
  the docs explain it, so a bare-token grep would make those files permanently
  uncommittable. See notes.md.
- **Reject feedback travels back to the panel** rather than through
  `setError`, because `withBusy` clears the error on success. Mirrors how
  `importHistory` already returns its count.
