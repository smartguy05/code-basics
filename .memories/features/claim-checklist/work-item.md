# Feature: Per-claim checklist (BACKEND)

Expose the evidenced-claim list that `git/coverage.rs::review()` currently computes and throws away.

## Background (verified)
- `review()` builds the full declared-claim universe in `claims_in_play()`, then filters to the unmatched subset and keeps only a count (`scorecard.evidenced: u32`) for the evidenced half.
- `evidenced_claims()` yields the `(turn,label)` keys of accepted attribution spans; unmatched = claims_in_play minus evidenced_set.

## Acceptance criteria (backend only)
1. `IntentReview` gains `pub evidenced: Vec<UnfulfilledClaim>` (reuse the row shape; do not rename UnfulfilledClaim).
2. `review()` PARTITIONS claims-in-play into evidenced vs unfulfilled by membership in evidenced_set.
3. Invariant `scorecard.evidenced == evidenced.len()` holds.
4. Tests first; key-pinning JSON test updated.
5. `src/ipc/types.ts` mirror updated. No other `src/` file touched (React rendering is a separate agent).

## Out of scope
- ChangesView.tsx / IntentPanel.tsx and any rendering (another agent).
