# Fix: intent cards show 133 generic "New X" groups instead of intent-titled groups

## Context

The Changes tab's grouped view is supposed to show cards titled by what the agent *said* it was doing ("Fix textbox input error"), each containing all related edits across files. Instead the user sees 133 single-file, single-hunk cards titled "New import", "New usize", "New AtomicU64".

Investigation found the symbol fallback is doing exactly what it was designed to do — **the real problem is that no intent data exists**, plus a handful of defects in the fallback itself:

1. **Hooks pinned to the wrong workspace** (root cause). `command_line()` in `crates/core/src/intents/providers/hooks_json.rs:101-111` bakes `--workspace <install-time root>` into the hook command even for user-scope installs. The hooks in `~/.claude/settings.json` are pinned to `...\Code\ONEflight`, and `hook::resolve_root` (`intents/hook.rs:407-415`) prefers that over the payload's `cwd`. So every edit made in code-basics is dropped by `relative_to`. On disk today: ONEflight has 52 orphaned labels and **no `edits.jsonl` exists anywhere** — geometry was never recorded, and labels can't join to hunks without it.
2. **This repo never opted in**: `hook::is_enabled` = `.code-basics/intents/` dir exists; code-basics doesn't have one.
3. **Mined history unused**: `intent_groups` only reads the on-disk jsonl; 21 Claude Code transcripts for this repo are never mined unless the user clicks "Import past sessions".
4. **Silent degradation**: with zero records, attribution short-circuits (`attribution.rs:154`), pass 1 (bucket key `intent:{turn_id}` — the only cross-file merge) never fires, and pass 3 makes one card per distinct symbol → N hunks ≈ N cards. The UI gives no hint anything is missing.
5. **`declaration_name` names the type, not the binding** (`grouping.rs:249-281`, "last identifier wins"): `let total: usize = 0;` → "usize", `static COUNTER: AtomicU64` → "AtomicU64".
6. **`symbol_from_header` is too permissive** (`grouping.rs:196-207`): emits any ≤80-char header line verbatim — this is where "New import" comes from (`import …`/`use …` header lines).
7. **Verb duplication**: the badge already says NEW/CHANGED (`IntentPanel.tsx:26-32`), and the Rust label repeats it (`format!("{verb} {symbol}")`, `grouping.rs:362-366`) → "NEW  New import".
8. **Label quality**: no `Intent:` instructions section in this repo's CLAUDE.md, so Stop-hook labels fall back to first-sentence prose ("Committed and pushed").

User decisions (confirmed):
- **Repair local setup too** — fix the pinned hooks in `~/.claude/settings.json`, create `.code-basics/intents/`, run the past-session import.
- **Collapse singleton fallback cards per file** — several single-hunk symbol cards in one file merge into one "Several changes in <file>" card.

Repo memory folder: `.memories/bugs/intent-cards-explode-without-intent-data/` (work-item.md, plan.md, notes.md, todos.md, completed.md — maintained throughout).

All work is tests-first per CLAUDE.md: write each test, watch it fail for the right reason, then implement.

## Phase C — `declaration_name` names the binding, not its type

*(C/D/E are pure cb-core and land first — immediate visible improvement, independent of hook work.)*

Tests in `crates/core/src/git/grouping_tests.rs`:
- `a_let_binding_names_the_variable_not_its_type` — `+let total: usize = 0;` → `Some("total")` (fails today)
- `a_static_names_the_item_not_its_type` — `+static COUNTER: AtomicU64 = ...` → `Some("COUNTER")` (fails today)
- `a_typescript_annotated_const_names_the_binding` — `const cache: Map<string, number> = ...` → `Some("cache")`
- `a_csharp_property_names_the_property` — `public Bitness Bitness { get; }` → `Some("Bitness")` (guard; already passes — last-identifier rule is correct for C# forms)
- Existing guards (lines ~261, 279, 286, 298, 305) stay green.

Implementation in `grouping.rs::declaration_name`: within the pre-`(`/`=`/`<`/`{` head, if it contains an internal `:`, take the last identifier *before the first `:`*; otherwise keep the last-identifier rule.

Also tighten `symbol_from_header` (`grouping.rs:201-206`): reject non-declaration headers that start with `import`/`use`/`using`/`from`/`#include`/`package` or contain `;`/quotes — those hunks fall to per-file `Other` ("a wrong label is much worse than no label"). Tests: `an_import_line_in_the_header_is_not_a_symbol`, `a_use_statement_header_is_not_a_symbol`, plus the existing real-function-header test stays green.

## Phase D — stop repeating the badge verb in the label

Rust-side (`grouping.rs:362-366`); `IntentGroup` shape unchanged (label stays a String, key-pinning test untouched), bucket ids unchanged (stage/revert by id unaffected):
- `NewSymbol`/`ModifiedSymbol`: label = bare `{symbol}` (badge carries New/Changed)
- `Formatting`: "Whitespace only" (badge already says "Formatting")
- `Other`: keep "Other changes in {file}"; `Intent`: agent's label, unchanged

Test: `a_symbol_card_label_is_the_bare_symbol`; update any test asserting the old strings.

## Phase E — collapse singleton fallback cards per file

New pure post-pass in `group()` (`fn collapse_singletons(Vec<IntentGroup>) -> Vec<IntentGroup>`, called before sorting): a symbol-kind group with exactly one file and one hunk is a singleton; when a file yields **two or more** singletons, merge them into one `Other` card per file, label `"Several changes in {file_name}"`, id `other:{path}` (merging into an existing `Other` bucket for that file — ids must not collide). Groups with ≥2 hunks or spanning files are untouched; a file's single symbol card keeps its symbol label. Never merges across files, never invents an intent.

Tests: `many_singleton_symbols_in_one_file_collapse_into_one_card_per_file`, `a_symbol_spanning_files_survives_the_singleton_collapse`, `a_files_single_symbol_card_keeps_its_symbol_label`, `singleton_collapse_merges_into_an_existing_other_bucket_for_the_file`; re-verify existing tests at grouping_tests.rs:312 and :620.

## Phase A — user-scope hooks must not be pinned to one workspace

1. `providers/hooks_json.rs`: `command_line`/`commands_for`/`plan_merge` take `Option<&Path>` workspace; emit `--workspace` only for `Some`. Callers (`claude_code.rs:128`, `codex.rs:134`) pass `Some(root)` for project scope, `None` for user scope.
   Tests (`providers_tests.rs`): `a_user_scope_hook_command_omits_the_workspace_flag`, `a_user_scope_install_plan_does_not_pin_a_workspace`, `a_project_scope_install_plan_still_names_the_workspace`; update the existing command test (~line 288) to project scope.
2. Recorder robustness (`intents/hook.rs` + `src-tauri/src/recorder.rs:49-57`): with no explicit `--workspace`, walk the payload `cwd`'s ancestors and resolve to the first *enabled* workspace (`resolve_enabled_root`). Tests: `resolve_root_falls_back_to_the_payload_cwd_when_no_workspace_is_named`, `resolve_root_ascends_from_a_subdirectory_to_the_enabled_workspace`.
3. Detect + repair pinned installs: new `hooks_json::pinned_workspace(path) -> Option<String>`; `status()` (claude_code + codex) reports `capture: None` plus a caveat when a user-scope hook is pinned to a different root — the existing Enable flow (preview → confirm) then *replaces* the marker entry (existing retain-then-extend in `merge_into` already does this). No IPC shape change (`caveats` already exists).
   Tests: `a_user_hook_pinned_to_another_workspace_reports_not_capturing_with_a_caveat`, `re_enabling_user_capture_replaces_the_pinned_hook_entry`, unit tests for `pinned_workspace`.

## Phase B — missing intent data becomes visible and actionable

Decision: `intent_groups` does **not** auto-mine history (IO-heavy per diff refresh; import writes files, a query must not; mined labels are coarse). Import stays user-initiated; the jsonl append + `tool_use_id` dedupe already makes re-import idempotent.

- New `src/components/intentPanelLogic.ts` + `.test.ts` (vitest): `intentDataHint(groups, providers)` decides the banner (none / capture-off-with-sessions / capture-off-no-sessions / capturing-but-unmatched / pinned-hook caveat). Tests per case.
- `IntentPanel.tsx`: when zero stated groups but groups exist, render a banner with the hint plus actions **Enable capture** (expands existing `CaptureSetup`) and **Import past sessions (n)** (existing `onImportHistory`); show imported-count feedback using the count `import_intent_history` already returns. No new Tauri commands.

## Phase F — docs

- `docs/guides/agent-intent-capture.md`: user-scope hooks no longer pin a workspace (record into whichever enabled workspace the agent works in); pinned-hook repair caveat; the new banner; label wording; singleton collapse in the fallback description.
- `pnpm docs:index` (new `intentPanelLogic.ts` must appear) and `pnpm docs:check`. `docs/reference/commands.md` needs no change (no command-surface change) — verify.

## Phase H — repair this machine's setup (user-approved)

1. Edit `~/.claude/settings.json`: remove `--workspace "...ONEflight"` from the two `record-intent` hook commands (PostToolUse + Stop), leaving the rest of each command intact. (Do not touch the unrelated USB-LCD hooks.)
2. Create `.code-basics/intents/` in this repo (the opt-in flag `hook::is_enabled` checks).
3. Append the marked `Intent:` instructions section (`instructions::SECTION`) to this repo's `CLAUDE.md` so future Stop labels are real intents, not first-sentence prose.
4. Run the past-session import for this workspace (equivalent of "Import past sessions"; 21 transcripts) and confirm `edits.jsonl`/`labels.jsonl` appear with joinable records.
5. Note: the orphaned `ONEflight/.code-basics/intents/labels.jsonl` contains code-basics prose labels; leave it (harmless — no matching edits) unless asked.

## Verification

1. Per-phase targeted loops: `cargo test -p cb-core <name>` (Git Bash, `sh` on PATH), watching each new test fail first.
2. Full: `cargo test -p cb-core`; `cargo clippy`; `cargo fmt --check`.
3. Coverage gates: `cargo llvm-cov --workspace --summary-only --fail-under-lines 70 --ignore-filename-regex "src.tauri.src.main\.rs|process.kill\.rs"`; `pnpm typecheck`; `pnpm test`; `pnpm coverage`.
4. Real-data diagnostic before/after: `cargo test -p cb-core --test intent_attribution -- --ignored --nocapture` (`report_attribution_against_this_repository`) — expect group count to fall from ~133 and labelled share to rise once import runs.
5. `pnpm docs:index && pnpm docs:check`.
6. Manual end-to-end (after Phase H): open code-basics in the app → Changes → intent view shows intent-titled cards from imported history; make a Claude Code edit in this repo → `.code-basics/intents/edits.jsonl` grows, Stop label lands, one multi-file intent card appears.

## Critical files

- `crates/core/src/git/grouping.rs` (+ `grouping_tests.rs`) — Phases C/D/E
- `crates/core/src/intents/providers/hooks_json.rs`, `claude_code.rs`, `codex.rs` (+ `providers_tests.rs`) — Phase A
- `crates/core/src/intents/hook.rs` (+ tests), `src-tauri/src/recorder.rs` — Phase A
- `src/components/IntentPanel.tsx`, new `src/components/intentPanelLogic.ts`/`.test.ts` — Phase B
- `docs/guides/agent-intent-capture.md`, `docs/INDEX.md` (regenerated) — Phase F
- `~/.claude/settings.json`, `.code-basics/intents/`, `CLAUDE.md` — Phase H (local setup repair)
