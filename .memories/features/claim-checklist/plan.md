# Plan

1. Tests first in `crates/core/src/git/coverage_tests.rs`:
   - `evidenced_claims_are_listed_and_partitioned_from_unfulfilled` — matched claim in `evidenced`/not `unfulfilled`; unmatched claim in `unfulfilled`/not `evidenced`.
   - `evidenced_list_length_equals_the_scorecard_count` — `evidenced.len() == scorecard.evidenced`, and evidenced+unfulfilled == claims.
   - Update key-pinning `an_intent_review_serialises_with_the_keys_the_ui_reads` → keys `["evidenced","groups","scorecard","unfulfilled"]`.
2. Implement in `coverage.rs`:
   - Add `pub evidenced: Vec<UnfulfilledClaim>` field to `IntentReview`.
   - Replace the filter with `.partition()` on `evidenced_set` membership; sort both lists by (turn_id, label); scorecard from `evidenced.len()`; `debug_assert_eq!`.
3. `src/ipc/types.ts` — add `evidenced: UnfulfilledClaim[]` to `IntentReview`.
4. Command `intent_groups` in `commands/intents.rs` returns `IntentReview` by value — no signature change needed.
5. `pnpm docs:index`, typecheck, `cargo check --workspace`, `cargo test -p cb-core coverage`.
