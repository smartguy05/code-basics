# Completed — workflow/subagent edits lost their stated intent

## Symptom
In Changes → Intent, edits made via a **workflow/subagent** (or any background
task) showed as "**N files changed together** (One turn)" with no stated intent,
while the main agent's `Intent:` reasons sat under "**N stated intents with no
matching change in this diff**".

## Root cause (confirmed from this repo's own `.code-basics/intents/` records)
Everything joins on `turn_id` (`intents/hook.rs:265`, `Intents::label_for`
`intents/mod.rs:205`, `coverage.rs`). A subagent's `PostToolUse` edits record
geometry under the **subagent's** turn id (e.g. `8d11df35…`, no label); the main
agent's declared `Intent(paths): …` lines record under a **later main-turn** id
(e.g. `bf15f517…`, no geometry). Background work edits in a turn that has no
reason yet, and the reason is declared in a later turn — so the **join must
bridge turns**; capture-time alignment can't fix it.

## Fix (cb-core only; no IPC/types.ts change — `intent_groups` returns
`IntentReview`, not raw spans)
An **effective label** per span that may come from another turn, carrying the
label's own turn id, made coherent across the card AND the scorecard:
- `intents/mod.rs::effective_scoped_label(record, path)` — same-turn `label_for`
  first (unchanged priority); else the **unambiguous Declared + non-empty-paths**
  label covering the path across turns (reuses `scope_covers`). Abstains on ≥2
  covering labels; never an Inferred or empty-paths label.
- `git/attribution.rs` — `AttributedSpan.label_turn_id`; span build resolves via
  `effective_scoped_label`.
- `git/coverage.rs::review` — on a mutable clone: `bind_bare_orphan` (single
  orphan turn + single unbound bare Declared label → bind; abstain otherwise);
  `evidenced_claims` keyed by `(label_turn_id|turn_id, label)`; `claims_in_play`
  label-centric (same-turn + path-scoped + bound-bare), keyed by the label's turn.
- `git/grouping.rs` — Intent card keyed `intent:{label_turn}:{label}` so one
  orphan geometry turn can split into its two real declared intents.

## Review findings addressed (adversarial pass)
- **Bare bridge could mislabel** with a lone *stale* bare `Intent:` (its geometry
  already committed, absent from diff) — no local signal can detect staleness.
  Per Anthony's choice: **keep the bridge but flag it `Confidence::Low`**
  (`bind_bare_orphan` downgrades the bound spans; the card's min-confidence goes
  Low). Path-scoped and same-turn declared cards stay High. The association is
  shown as a heuristic, not asserted — consistent with "a wrong label is worse
  than no label".
- **Missing abstain tests** — added: `a_bare_label_abstains_when_two_candidate_
  reasons_exist` and `a_bare_label_spent_on_its_own_turn_does_not_also_bind_an_
  orphan` (the `!already` guard). Bare-bind test now asserts Low; path-scoped test
  asserts not-Low (both on multi-line fixtures so geometry scores above Low and
  the downgrade is observable).

## Deferred (explicitly out of scope, per plan)
Capture-side: install `SubagentStop` (`hooks_json.rs:42`, `hook.rs:116`) and stop
the miner skipping `isSidechain` (`claude_code.rs:310`). Helps a Task-subagent
that states its *own* intent; does not affect the workflow case fixed here.

## Verified
- Implemented via workflow (interrupted mid-run day 1; finished + verified day 2).
- `cargo test -p cb-core`: 2139 passed; new: 8 `effective_label`/coverage
  cross-turn tests incl. the exact screenshot case
  (`one_orphan_geometry_turn_splits_into_two_declared_intent_cards`) + the
  integration test `a_declared_reason_binds_across_turns_through_the_public_review`.
  The lone `process::…cancel` failure is the documented load-flake (passes
  isolated).
- clippy clean for new code (only pre-existing `rider.rs` warning); all changed
  files rustfmt-clean; `pnpm typecheck` + `pnpm test` (792) green;
  `docs/INDEX.md` regenerated + `docs:check` passed.
- Kept the diff focused: reverted rustfmt module-recursion collateral in
  `intents/providers/*` (pre-existing drift, not ours).

## Not done (manual)
Live confirmation in `pnpm tauri dev`: open Changes → Intent on this working tree
and confirm the review-panel edits resolve into the two stated-intent cards
("Codex model selection…", "Draggable/resizable review panel") and the
"unmatched" pill clears. Left as the manual step.
