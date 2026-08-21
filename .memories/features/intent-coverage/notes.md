# Intent panel — UI decluttering (2026-08-20)

The Changes | Intent panel was too busy above the cards. Cleaned up so the
group cards dominate:

- **Setup pane auto-collapses when capture is on.** `IntentPanel` initial
  `setupOpen = !capturing` was captured before providers loaded async (they
  arrive as `[]` → `capturing=false` → pane opened → never re-collapsed). Now a
  `lastCapturing` ref + effect follows capture *transitions*: collapses when it
  flips on, opens when off. A manual toggle in between is respected until
  capture next changes.
- **Scorecard line, scope check and unfulfilled claims folded** behind the
  "N groups" header (now a twisty toggle, `summaryOpen`, default collapsed,
  `.intent-summary`). The unmatched badge stays on the header so the signal is
  still glanceable. `hasSummaryDetails` gates the twisty/click. Behavioral
  overall stays outside the fold (only appears after the user runs it).
- **Run before/after + Verify claims now inline** (were stacked): `.behavioral-run`
  is still column, but the two buttons live in a `.behavioral-actions` flex row
  (each `flex:1`), with a one-line `.behavioral-hint` caption below that doubles
  as the live run status. Clarified the difference in the caption + tooltips:
  before/after = observable outcome diff, no agent; Verify claims = same run then
  an agent judges the claims.

Files: `src/components/IntentPanel.tsx`, `src/views/ChangesView.tsx`,
`src/styles.css`. Pure `*Logic.ts` unchanged, so no test changes; typecheck +
vitest (840) green.

# Grouping: formatting-move bug + evidenced intent split (2026-08-20)

Three fixes in `crates/core/src/git/grouping.rs` (+ grouping_tests.rs), driven
by user review of the Changes|Intent cards:

1. **`is_formatting_only` no longer calls a reorder "whitespace only".** It used
   to sort removed/added normalised lines and compare — so a statement swap
   (same lines, new order) had equal multisets and was labelled Formatting. A
   moved line is also byte-identical del+add. Fix: keep BOTH raw and normalised
   per line; a hunk is formatting iff normalised multisets match AND the RAW
   multisets differ (some whitespace actually changed). Raw-equal ⇒ pure move ⇒
   NOT formatting. Reversed the old test `reordering_..._is_formatting_only` →
   `..._is_not_formatting` (it encoded the bug); added `swapping_two_statements_
   is_not_formatting`. CRLF test still passes because raw keeps the `\r`.

2. **Evidenced split.** When one hunk's attribution spans tie lines to ≥2
   distinct DECLARED intents, each intent becomes its own card carrying only its
   span's `line_indices` (remainder → symbol/Other, never re-attached). Safe
   because stage/revert/reject already act on `GroupFile.line_indices`, not whole
   hunks (see src-tauri/commands/intents.rs `lines_for`/`selected`). Before this,
   such hunks with no dominant were dropped to a location card and the intents
   showed as *unmatched*. Only per-line evidence triggers it; path-only covering
   is untouched (still abstains to one card). Tests: `two_evidenced_intents_in_
   one_hunk_split...`, `unattributed_remainder...not_claimed...`.

3. **De-dup evidenced reasons out of ambiguous candidates.** A declared reason
   already bound elsewhere (has its own card) was also listed as a "candidate"
   on an ambiguous card for a different unbound hunk it path-scopes. Now
   `evidenced_intents` (declared spans anywhere in the diff) is filtered out of
   `covering`, so a known intent never masquerades as a guess. If filtering
   leaves 1 reason → titled card; 0 → symbol/Other. Test: `an_evidenced_reason_
   is_not_repeated_as_an_ambiguous_candidate`. NOTE: deliberately does NOT merge
   the unbound hunk INTO the evidenced card — that'd be unevidenced attribution
   (violates abstain + the user's "split only where evidenced" choice).

Frontend (intentPanelLogic.ts / IntentPanel.tsx): ambiguous headline changed
from "Ambiguous intent" → "N stated intents for this file"; caption "one of:" →
"could be any of:". Test updated.

# Changes tab auto-polls while showing (2026-08-20)

User: "when I'm on the changes tab it should automatically poll and update"
(previously had to switch tabs to see external edits). `src/views/ChangesView.tsx`:
- `POLL_MS = 2000` setInterval while mounted (the view is mounted only while the
  Changes tab is active, so the interval is tab-scoped). Skips a tick while
  `busyRef` (mirror of `busy`), `document.hidden`, or grouping==="stashes".
- Calls refreshStatus + refreshIntent + refreshErosion each tick. Must call the
  intent/erosion refreshers directly, NOT rely on the `status` cascade:
  `FileChange` carries only the status letter (no content hash), so an agent
  editing an already-M file doesn't change gitStatus → cascade wouldn't fire.
- De-churn: each refresher stores a JSON signature of its result in a ref
  (statusSignature/intentSignature/erosionSignature) and only setState when it
  changed — so a quiet tick does the IPC read but causes no re-render/regroup.
  Mode-change effect clears intent+erosion signatures (per-mode line numbering).
- Deliberately does NOT reload the open diff pane on poll: it's an editable
  editor, reloading would discard unsaved edits / jump scroll. Clicking a
  file/card still calls loadFile for fresh content.
