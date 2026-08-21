# Notes — user-authored intents (write your own intent on a card)

Added 2026-08-21. Lets the user write/overwrite an intent on any Changes→Intent
card — for code changed manually or by an agent that didn't follow the hooks.
User decisions: **per-card scope** (not whole file) and **true overwrite**, and
**prompt before overwriting an agent intent**.

## Mechanism — reuse the recorded-intent pipeline, don't add a parallel one
A user note is stored as the card's changed-line **content** + a declared label,
then at `intents::load` converted to ordinary `IntentRecord`s + `IntentLabel`
and merged in. Attribution rebinds by content (survives line moves); grouping
titles the card. Given the **highest seq** on load → wins any contested line =
true overwrite. This is why NO new grouping/attribution logic was needed.

Key files:
- `crates/core/src/intents/user.rs` (+`user_tests.rs`) — `UserIntent`/`UserEdit`,
  `upsert` (overlap-replace = re-annotate replaces), `remove_overlapping` (clear),
  `to_intents`, `merge_into` (rebases user seqs above every agent seq), fs in
  `user-intents.json` (separate file; NOT wiped by `intents::clear`).
- `intents::mod` — `ProviderId::User` (as_str "user"); `load()` calls `merge_into`.
- `git/grouping.rs` — `IntentGroup.user_authored` (serde skip when false; derived
  in `finish()` from id prefix `intent:usernote:`); `changed_content(diff, indices)`
  pure helper to capture a card's line content.
- `commands/intents.rs` — `set_card_intent(group,label,mode)` / `clear_card_intent(group,mode)`;
  `card_edits` re-derives geometry via group+diff. Registered in `lib.rs`.
- Frontend: `intentPanelLogic.intentEditPlan(group)` (tested) decides prefill /
  overwrite-confirm / hasUserNote. `IntentPanel` GroupCard: "Add intent…" /
  "Override intent…" / "Edit note…" + "Clear note", inline editor with confirm
  step. `ChangesView` wires `setCardIntent`/`clearCardIntent` → api → refreshAll.
  `types.ts`: `IntentGroup.userAuthored?`, `ProviderId` adds "user". `api.ts`
  wrappers `setCardIntent`/`clearCardIntent`.

## Gotchas / decisions
- turn_id is `usernote:{id}` (distinctive prefix, NOT bare `user:`) so a grouping
  key `intent:usernote:{id}:{label}` reliably marks user cards; ids are colon-free
  (`u{n}`) so parsing the key segment is safe.
- `ProviderId::User` is NOT an installable agent — `providers::instructions::path_for`
  got a defensive arm; it's never in `providers::all()`.
- Overwrite is by seq at LOAD (rebased above agent max), not save — so a note saved
  early still beats a later agent edit.
- Stale notes: if the user edits the annotated lines, the stored content no longer
  matches the diff → the note silently stops binding. Acceptable MVP; "Clear note"
  removes it, re-annotating replaces. No GC of fully-stale notes yet (todo).
- IPC: `IntentGroup` is NOT pinned in model.rs; its key test is in grouping_tests.rs
  and still passes because userAuthored is omitted when false.

## Verified
2218 cb-core tests + 844 frontend tests pass; typecheck + `cargo fmt --check` clean;
docs:index + docs:check pass. End-to-end grouping test:
`git::grouping::tests::a_user_note_wins_the_line_and_marks_the_card`.
