# Intent coverage — completed

Feature #1 from the `feature-ideas.html` report. Extends the intent attribution
join with the two failure directions plus an aggregate.

## What shipped
- **`crates/core/src/git/coverage.rs`** (+ `coverage_tests.rs`, 13 tests): new
  wire types `IntentReview { groups, unfulfilled, scorecard }`,
  `UnfulfilledClaim { turnId, label, provider, paths }`,
  `Scorecard { claims, evidenced, unmatched, hunks, attributedHunks, unattributedLines }`.
  `review(diffs, attributions, intents)` runs `grouping::group`, then a reverse
  pass that reads the forward pass's output — a turn is *evidenced* when some
  `AttributedSpan` carries its id; a declared claim in play but not evidenced is
  *unfulfilled*. Only `LabelSource::Declared` labels become claims. `evidenced`
  is derived as `claims - unfulfilled.len()` so the scorecard can never disagree
  with the list.
- **`grouping::kind_order`** reversed: `Other` (Unexplained) now sorts to the
  TOP (was near-bottom); Formatting still last. Renamed the overstated test to
  `intent_cards_sort_before_locations_and_formatting`; added
  `unexplained_cards_sort_to_the_top` and `formatting_still_sorts_last_below_unexplained`.
- **`src-tauri/src/commands/intents.rs`**: `intent_groups` returns `IntentReview`
  via `coverage::review` (command name + `generate_handler!` unchanged).
- **IPC**: mirrored `IntentReview`/`Scorecard`/`UnfulfilledClaim` in
  `src/ipc/types.ts`; `intentGroups` in `api.ts` returns `IntentReview`.
- **Frontend**: `intentPanelLogic.ts` gained pure `scorecardLine(sc)` and
  `unfulfilledCaption(claims)` (+ vitest). `IntentPanel.tsx` renders a scorecard
  strip + an unmatched badge + an informational unfulfilled-claims section (no
  actions — nothing in the tree to act on). `ChangesView.tsx` holds
  `scorecard`/`unfulfilled` state from the review.

## Verified
`cargo test -p cb-core --lib coverage` (13) + `grouping` (57) green;
`cargo check -p cb-app` clean; `pnpm typecheck` clean; `pnpm test` 697 pass;
`pnpm docs:index` + `docs:check` clean.

## Notes
- Ordering is owned by core (`kind_order`); the frontend never re-sorts.
- Abstain discipline held: wording is "no matching change in this diff", never
  "not done"/"lie" (a vitest asserts this).
