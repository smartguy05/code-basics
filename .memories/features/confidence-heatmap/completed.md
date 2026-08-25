# completed — confidence-heatmap BACKEND

## What shipped
Per-file agent self-reported confidence captured via an optional `[confidence:
low|medium|high]` token in the Intent grammar, surfaced on `IntentGroup`.
Kept strictly separate from `attribution::Confidence` (match-trust). Abstains
(None) when absent or unrecognised.

## Files changed
- crates/core/src/intents/mod.rs — new `pub enum SelfConfidence {Low,Medium,High}`
  (serde camelCase, Type); optional `self_confidence: Option<SelfConfidence>` on
  `IntentLabel` (`#[serde(default, skip_serializing_if="Option::is_none")]`).
- crates/core/src/intents/hook.rs — `parse_declared_labels` now returns
  `(paths, text, Option<SelfConfidence>)`; new `strip_confidence_token` (trailing,
  case-insensitive, abstains + keeps text on unknown level). `parse_labels_with_source`
  gained the 4th tuple element; `parse_labels` unchanged 2-tuple. `ingest_label`
  records it. INTENT_REQUEST wording adds the token sentence.
- crates/core/src/intents/providers/instructions.rs — SECTION const adds the
  `[confidence: …]` sentence + example (kept in sync with INTENT_REQUEST).
- crates/core/src/git/grouping.rs — `IntentGroup.self_confidence` +
  `Bucket.self_confidence`; `caution_rank`/`more_cautious` helpers; a
  `declared_confidence` map keyed (turn,label) built from declared labels;
  threaded through every Intent-creating path (per-intent split, dominant span,
  single-covering). Ambiguous/non-intent/inferred/derived = None. Merges keep the
  most cautious. Does NOT touch existing `confidence`.
- crates/core/src/intents/{user.rs, providers/claude_code.rs} — set
  `self_confidence: None` on their IntentLabel literals.
- Test literals updated across coverage_tests, attribution_tests, why_tests,
  grouping_tests, intents_tests, behavioral/{prepare,attribute}_tests, and
  tests/{intent_attribution,durable_why}.rs.
- src/ipc/types.ts — `SelfConfidence` union + `selfConfidence?` on IntentGroup.
- docs/INDEX.md regenerated.

## Tests added (all pass)
- intents_tests.rs: self_confidence_serialises_in_camel_case;
  a_label_with_a_self_confidence_round_trips_with_the_camel_case_key;
  a_label_without_a_self_confidence_omits_the_key;
  a_label_recorded_before_the_confidence_field_existed_reads_as_none.
- hook_tests.rs: a_declared_intent_can_carry_a_self_confidence_token;
  a_declared_intent_without_a_token_has_no_self_confidence;
  the_confidence_token_is_matched_case_insensitively;
  an_unknown_confidence_level_abstains_and_keeps_the_text;
  parse_labels_strips_the_confidence_token_from_the_label.
- grouping_tests.rs: a_declared_labels_self_confidence_reaches_its_group;
  a_declared_label_without_a_stated_confidence_leaves_the_group_none;
  a_group_gathering_several_confidences_keeps_the_most_cautious;
  self_confidence_appears_only_when_the_agent_stated_one.

## Verification
- cargo test -p cb-core intents -> 402 passed, 0 failed
- cargo test -p cb-core grouping -> 71 passed, 0 failed
- cargo test -p cb-core (full suite, background) -> 0 failures
- cargo check --workspace --all-targets -> clean
- cargo fmt --check -> clean; pnpm typecheck -> clean; pnpm docs:check -> pass

## Final IntentGroup JSON shape (for the UI agent)
`selfConfidence` is OPTIONAL and camelCase: `"low" | "medium" | "high"`, present
ONLY when the agent stated one (omitted, never null, otherwise). Distinct from
`confidence` (match-trust). Example:
{ id, kind, label, candidates?, symbol?, files, lineCount, confidence,
  selfConfidence?: "low", userAuthored? }

---

# completed — confidence-heatmap FRONTEND

## What shipped
The diff-pane confidence heatmap: changed lines are tinted by the agent's
self-reported confidence for the intent that produced them. Strongest tint on
`low` (where the reviewer should look hardest), faintest on `high`. Per-file /
per-group granularity — a group's `selfConfidence` applies to every changed line
it owns. Abstains (no tint) for lines whose group stated no `selfConfidence`.
Cloned exactly from the risk overlay's parallel decoration-layer seam.

## Files changed
- src/components/confidenceLogic.ts (NEW) — pure decision module.
  `confidenceForFile(path, diff, groups) -> {index, level}[]`: for each group
  with a `selfConfidence` whose files include `path`, emit its `lineIndices` at
  that level; LOWEST confidence wins on overlap (moreCautious: low<medium<high);
  `diff` bounds output to indices actually present in the file's hunks; lines
  with no stated confidence are omitted (abstain). `confidenceClass(level)` ->
  `cb-line-confidence-<level>`.
- src/components/confidenceLogic.test.ts (NEW) — 8 tests (see below).
- src/components/DiffView.tsx — cloned the risk layer: `setConfidenceLines`
  StateEffect + `confidenceLineField` StateField (uses `confidenceClass`),
  registered after `riskLineField`; new `confidence?: {index,level}[]` prop
  (destructured); a driving useEffect keyed on serialized `confidenceKey` with
  the same rebuild-trigger deps as the risk effect. Import of `confidenceClass`
  + `SelfConfidence` type.
- src/views/ChangesView.tsx — `confidenceForOpenFile` useMemo (calls
  confidenceForFile for selectedPath, deps [diff, selectedPath, intentGroups]),
  passed as `confidence={confidenceForOpenFile}` into <DiffView>, additive to the
  existing risk/coverage wiring. Import added.
- src/styles.css — `.cb-line-confidence-low` (red stripe rgba(224,85,97,.14) +
  inset 3px var(--fail), loudest), `-medium` (inset 3px var(--skip)), `-high`
  (inset 3px rgba(122,160,122,.55), faintest). Mirrors the risk-* precedents.

## Tests added (all pass)
confidenceLogic.test.ts:
- maps a group's changed lines to its stated self-confidence
- abstains — emits nothing — for lines whose group has no self-confidence
- only emits for the requested path
- takes the LOWEST confidence when two groups claim the same line
- takes medium over high, and low over medium
- does not emit an index that is not part of this file's diff
- returns an empty list when no group has a self-confidence
- confidenceClass names the per-level decoration class

## Verification (verbatim)
- pnpm test -> Test Files 42 passed (42); Tests 967 passed (967)
- pnpm typecheck -> clean (tsc --noEmit, no output)
Backend was already done (SelfConfidence union + selfConfidence? on IntentGroup
in types.ts); no Rust/IPC type change needed this pass.
