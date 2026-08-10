# TODO / Remaining

## Open

- [x] **Reject verified in the running app** (2026-08-10, by Anthony). Rejected
  the second of two hunks in a `.ts` file on a scratch repo: the kept hunk above
  it survived, and the note landed above the restored line at the file's own
  indent — i.e. the anchor shift for a kept earlier hunk is right in practice,
  not just in `a_kept_earlier_hunk_shifts_the_anchor_by_its_net_line_change`.
- [ ] **The commit guard is still only verified by tests.** That run did not
  enable capture, so `.git/hooks/pre-commit` was never installed and the
  install preview / blocked-commit path has not been seen in the UI. Covered by
  `tests/reject_markers.rs`, but not clicked through.
- [ ] `guard::plan_removal` has no caller, exactly like
  `hooks_json::plan_removal` — there is still no `disable_intent_capture`
  command. If one is ever added it should take the guard out too.
- [ ] Re-rejecting the same place: the note's own lines are part of the diff on
  the second pass, so group geometry includes them. Text handling is covered;
  the second-pass geometry is not. Revisit if it misbehaves.
- [ ] Consider a marker for languages with no line comment (JSON, CSS). Today
  they are reverted and reported unmarked, which is honest but means the reason
  reaches no one.

## Deliberately not doing

- No persisted rejection ledger. The comment in the file is the record, and
  `IntentGroup.id` is only stable within one computation, so there is no
  durable key to hang a "dismissed" state on anyway.
- No ESLint / per-language lint rule. The git hook is language-agnostic and
  covers every repo capture is enabled in.
