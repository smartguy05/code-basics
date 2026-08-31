# Completed: intents shown for changes already committed

## Symptom (reported while using the app)

Cards in the Changes tab carried reasons from turns whose work had been
committed long ago.

## Root cause

`IntentRecord` has **no timestamp and no commit** (`intents/mod.rs`), and
`attribution::attribute_file` matches every record ever recorded for a path
against the current diff **by content alone**. `load_edits` filtered on only
four things — empty edit, branch equality, path-inside-root, `tool_use_id`
dedupe — and the branch gate is a no-op for all mined history, which carries
`branch: None`. Nothing had ever removed a record; `intents::clear` was
all-or-nothing. So a committed record stayed forever and re-matched the moment
its text reappeared. (`edits.jsonl` was 16.5 MB / 7,939 records here, fully
re-parsed by `next_seq` on every hook fire.)

## Fix

New `crates/core/src/intents/retire.rs` (+ `retire_tests.rs`, and
`crates/core/tests/intent_retirement.rs` end-to-end).

- **The rule: full absorption, judged against HEAD alone.** A record retires
  only when HEAD accounts for *every* anchorable line of its evidence
  (additions present in the HEAD blob, a pure deletion's lines absent). Uses
  `attribution::anchor_key`, so a line the matcher would never anchor on cannot
  decide a retirement either.
- **The working diff gets no vote, and this is the subtle part.** The obvious
  second condition — "and none of its lines are still in the working diff" —
  *defeats the bug*. The symptom **is** committed text reappearing as an
  addition, so a stale record always looks like it has a live remainder. I wrote
  it that way first and the reproduction stayed red until the check came out.
  Only HEAD can tell "uncommitted" from "committed and typed again". The diff is
  still read, but only to choose a `KeepReason`.
- **Partial staging is safe** because absorption must be *total*: commit half a
  card and the rest is still missing from HEAD, so the record survives whole.
  Pinned by `a_partially_committed_card_keeps_its_intent`.
- **Archive + tombstone, never delete.** Retired records → `edits-archive.jsonl`,
  labels → `labels-archive.jsonl`, identities → `tombstones.jsonl`.
  `import_intent_history` runs `reject_tombstoned` **before** `rebase_seqs`, or a
  prune would be undone by one click (the miners re-read the agents' unbounded
  session archives).
- **Tombstone identity is the conjunction `(tool_use_id, content_key)`.**
  `turn_id` is unusable: mined ids are `claude-history-{session}-{block}` and the
  block index shifts as a session is resumed. `tool_use_id` alone degenerates to
  `":0"` when a payload carries no call id. A *conjunction*, not a disjunction:
  content alone would suppress a genuinely new edit repeating an old change.

## Where it runs (ordering is load-bearing)

- `commands/git.rs::git_commit` and `recorder.rs::record_why_for_head` — both
  **strictly after** `why::record_note`. The note is built from the very records
  being retired; pruning first empties every durable-why note.
- `commands/intents.rs::intent_groups` — `run_if_head_moved` before `load`. This
  is what catches a commit typed in a floating terminal, an amend, or a rebase.
  One ref read when HEAD has not moved.
- **Not** inside `intents::load`: it is called from hooks and four commands.

## Gotchas hit

- **`next_seq` must not fall after a prune.** It is `max(seq)+1` over the file,
  so retiring the highest record would hand out a colliding number and break
  attribution's "later edits win". Fixed with `high_seq` in `prune-state.json`.
  Then broke `next_seq_continues_after_the_highest_recorded`: `high_seq == 0`
  means *nothing retired*, not "seq 0 taken", so an empty workspace must still
  start at 0.
- **First run prunes nothing, by design.** No baseline ⇒ "HEAD moved" is
  unknowable. The pre-existing backlog is cleared by the manual, preview-gated
  **Archive absorbed intents…** button instead.
- Writing a reproduction that actually reproduces took three attempts: a
  single-line record never binds (the matcher needs contiguous agreement), and
  *moving* a block leaves it as diff **context**, not an addition. What works is
  the block appearing a **second** time.

## Files

`crates/core/src/intents/retire.rs`, `retire_tests.rs`, `mod.rs` (`next_seq`,
`clear`, module registration); `crates/core/tests/intent_retirement.rs`;
`src-tauri/src/commands/{intents,git}.rs`, `recorder.rs`, `lib.rs`;
`src/ipc/{types,api}.ts`, `src/components/{IntentPanel.tsx,intentPanelLogic.ts}`,
`src/views/ChangesView.tsx`; `docs/reference/commands.md`,
`docs/guides/agent-intent-capture.md`.

## Verified

`cargo test -p cb-core` — 2,572 passed, 0 failed. `cargo fmt --check` clean,
clippy clean for the new module. Frontend `pnpm test`/`pnpm typecheck` could
**not** be run in that session: every `node_modules` entry is a junction and the
agent shell cannot traverse them ("untrusted mount point"). The new pure helpers
were verified by running the same assertions through plain `node`.
