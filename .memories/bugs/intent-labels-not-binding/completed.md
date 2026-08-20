# Completed: Declared intents not surfacing on their cards

## Symptoms (reported while using the app on OneFlight)
1. `Intent(…/OperatorQuoteRequestForm.razor): fix non-resettable cancellation
   source` showed **no card** — file in diff, label recorded, but no edit
   geometry anywhere (changed via Bash/by hand).
2. `Intent(ONEflight.Client.Shared.Components): move per-search cancellation into
   Autocomplete` showed the **symbol title "Autocomplete"** instead of the
   reason — two declared labels scoped the same directory, so cross-turn binding
   abstained.

## Root cause
`grouping::group` only titled an Intent card from *attributed geometry*. A file
with no geometry (1) got no Intent card; a file whose geometry bound no label
because of cross-turn ambiguity (2) fell back to a symbol title. Meanwhile
`coverage.rs::claims_in_play` already counted these declared, path-scoped labels
as claims — an inconsistency between the scorecard and the cards.

## Fix (user chose "show all candidate reasons")
- `intents/mod.rs::scoped_labels_for_path(path)` — all declared, non-empty-paths
  labels whose `scope_covers` the path, deduped. `effective_scoped_label` now
  delegates its cross-turn arm to it (`[only] => Some`, else None — unchanged).
- `grouping::group` now takes `&Intents`. Per file, when a hunk did NOT bind a
  declared reason (kind is not Intent/Formatting) but declared labels scope the
  file: retitle to an Intent card — 1 reason → `label`; ≥2 → new
  `IntentGroup.candidates` (label ""), keyed `intent-ambiguous:<reasons>`.
  Confidence Low (stated, not evidenced). Whole-file changed lines populate
  `line_indices` so a geometry-less card still stages.
- **Scorecard untouched** — synthesis is grouping-only; no synthetic
  AttributedSpan (that would flip claims to evidenced + desync
  unattributed_lines). Guarded by the extended coverage test.
- IPC: `IntentGroup.candidates: Vec<String>` (skip_if empty) + `types.ts` +
  serialization test. Frontend: `intentPanelLogic.ts` (`isAmbiguousIntent`,
  `cardHeadline`, `cardCandidates`, `cardTitle`) + IntentPanel renders a
  "one of:" list.
- Callers updated to pass intents: `coverage::review`, `commands/intents.rs::
  lines_for`, `commands/behavioral.rs::working_tree_groups`, tests.

## Files
crates/core/src/intents/mod.rs, git/grouping.rs (+tests), git/coverage.rs
(+test), src-tauri/src/commands/{intents,behavioral}.rs, src/ipc/types.ts,
src/components/{intentPanelLogic.ts(+test),IntentPanel.tsx}.

## Verified
cb-core full lib 2195 pass; grouping 62, coverage 22, intents 375; frontend 840;
typecheck + clippy clean; docs:check pass.

## Note
The razor case has NO capturable geometry (Bash/manual edit), so only this fix
surfaces it. The Autocomplete case had cross-turn mined geometry + 2 dir-scoped
labels → now shows both candidates. See [[subagent-mining-missed]] for the
separate capture gap found alongside.
