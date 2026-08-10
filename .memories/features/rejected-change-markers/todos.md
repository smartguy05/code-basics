# TODO / Remaining

## Open

- [ ] **Verify in the running app.** Everything is covered by tests, but the
  feature has not yet been driven through the real UI: enable capture on a
  scratch repo, confirm `.git/hooks/pre-commit` appears in the install preview,
  reject a real card, confirm the commit is blocked, then fix and commit.
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
