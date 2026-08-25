# Completed

## Change
Exposed the evidenced-claim list from `git/coverage.rs::review()` instead of discarding it.

## Files touched
- `crates/core/src/git/coverage.rs` — added `pub evidenced: Vec<UnfulfilledClaim>` to `IntentReview`; `review()` now `.partition()`s claims-in-play into `evidenced`/`unfulfilled` by `evidenced_set` membership, sorts both by (turn_id, label), computes scorecard from `evidenced.len()`, and `debug_assert_eq!`s the invariant.
- `crates/core/src/git/coverage_tests.rs` — added `evidenced_claims_are_listed_and_partitioned_from_unfulfilled`, `evidenced_list_length_equals_the_scorecard_count`; updated key-pinning test to expect `["evidenced","groups","scorecard","unfulfilled"]`.
- `src/ipc/types.ts` — added `evidenced: UnfulfilledClaim[]` to `IntentReview`.
- `docs/INDEX.md` — regenerated.

## Notes
- `commands/intents.rs::intent_groups` returns `IntentReview` by value; the new field flows out with no command-signature change.
- `UnfulfilledClaim` deliberately NOT renamed (row reused for both halves) to keep churn low.

## Verification
- `cargo test -p cb-core --lib coverage` — 35 passed, 0 failed.
- `cargo check --workspace` — clean.
- `pnpm typecheck` — clean.

## Status: DONE (backend).

---

## Change (frontend)
Rendered the per-claim checklist in the intent view, replacing the flat unfulfilled list.

## Files touched (frontend)
- `src/components/claimChecklistLogic.ts` (NEW) — pure `claimRows(evidenced, unfulfilled, behavioral) -> ClaimRow[]` (`{label,paths,state}`, state `unmatched|evidenced|corroborated`). Card id built as `intent:${turnId}:${label}` from the fields (never by parsing an id string; label may contain ':'), looked up in a Map over `behavioral?.attributions` by `groupId`; corroborated only when the matching card carries ≥1 delta (else evidenced — abstain). Sort: unmatched → evidenced → corroborated, stable within a state. Also `claimChecklistCaption(rows)` (`"N claims · X corroborated · Y evidenced · Z unmatched"`, null when empty).
- `src/components/claimChecklistLogic.test.ts` (NEW) — 12 vitest cases (node env).
- `src/components/IntentPanel.tsx` — added `evidenced: UnfulfilledClaim[]` prop; `CLAIM_MARK` map (✗/✓/✓✓) + `ClaimRowView`; renders `claimRows(...)` + caption inside the summary block in place of the old `unfulfilled.map` block. Dropped now-unused `unfulfilledCaption` import/`unfulfilledHeading`; `hasSummaryDetails` now keys on `checklist.length`.
- `src/views/ChangesView.tsx` — added `evidenced` state, `setEvidenced(review.evidenced ?? [])` in `refreshIntent`, passed `evidenced={evidenced}` to `<IntentPanel>` (additive; risk-weighted-diff edits untouched).
- `src/styles.css` — `.claim-row`/`.claim-mark` + marker colours (unmatched=fail, evidenced/corroborated=pass).

## Verification (frontend)
- `npx vitest run src/components/claimChecklistLogic.test.ts` — 12 passed.
- `pnpm test` — 959 passed (41 files).
- `pnpm typecheck` — clean (needed `rows[0]?.state` for noUncheckedIndexedAccess).

## Status: DONE (frontend).
