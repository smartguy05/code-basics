# Completed — Adversarial review button (Phase 1)

## Panel-size + capture-side follow-ups (2026-08-19, branch claude/intent-scope-and-diff-fixes)
Ran via a workflow (3 parallel implement agents → verify → 2 review agents, all Opus).
Three focused commits after a standalone `cargo fmt` cleanup (6c35923):
- **9dc1b54 — panel size persistence.** `reviewLayoutLogic.ts` PanelLayout gains
  optional width/height (same numeric guards); pure `clampPanelSize` (floor
  360×280 wins over 96vw/92vh ceiling). `ReviewPanel` seeds size + captures grip
  resizes via a debounced `ResizeObserver` (grip fires no pointer event), only
  after a real resize. 6 vitest cases.
- **ad65e93 — capture-side intent attribution.** (a) `SubagentStop` hook: added to
  `hooks_json::EVENTS`; new `HookEvent::SubagentStop` → `ingest_label` like `Stop`
  but never `ask_for_intent` (pre-existing `event != Stop` guard already excludes
  it, so a subagent stop can't block/exit-2). (b) Retroactive sidechain mining:
  `claude_code::read_transcript` groups interleaved subagents by `parentUuid`
  lineage via `resolve_subagent_root` (cached; abstains on cycle/dangling/unknown);
  shared `TurnState`/`process_entry` keeps main-session turn ids unchanged, subagent
  ids `claude-history-{session}-sub-{root}-{block}`. (c) Fixture fix in
  `providers_tests` seeds a pre-existing SubagentStop hook (the one full-suite
  failure — a fixture gap, not a regression: merge is additive).
- Gates (independently re-run): `cargo test -p cb-core` 2163 passed / 0 failed;
  `pnpm test` 825 passed; `pnpm typecheck` clean; clippy only pre-existing
  rider.rs warning; rustfmt clean; INDEX regenerated.
- Reviews: both lenses (correctness/IPC, faithfulness/abstain) approved; two
  non-blocking nits recorded in todos.md. Manual empirical SubagentStop check
  still open (tests can't cover a live payload) — see todos.md.

---


## What shipped
An in-app "Review" button (Changes tab) that runs a chosen prompt through a
detected coding-agent CLI (**Claude Code** or **Codex**) read-only against the
open workspace and streams output into a **non-blocking, minimizable** floating
panel hosted at the App level (survives tab switches).

## Agent selection + non-blocking UX (2026-08-18 follow-up)
- Detects installed agents via `resolve_program` (`claude.exe` / `codex.cmd`).
  One installed ⇒ used directly; two ⇒ an Agent dropdown. Empty ⇒ a "neither on
  PATH" warning.
- Per-agent invocation (verified against each CLI's `--help`):
  - Claude: `claude -p <prompt> [--model opus|sonnet|haiku] --permission-mode plan`
  - Codex:  `codex exec --sandbox read-only [-m <model>] <prompt>`  (prompt LAST)
- Models are per-agent: Claude offers aliases; Codex offers none (runs its own
  default, model picker hidden) — we don't invent unverifiable model names.
- Panel is a floating window (no backdrop, non-blocking). "—" minimizes to a
  spinner pill keeping the review running + console mounted; "✕" cancels + closes.

## Files
- `crates/core/src/review.rs` (+ `review_tests.rs`, wired in `lib.rs`) — the one
  decision: `REVIEW_MODELS` (`opus`/`sonnet`/`haiku`), `DEFAULT_REVIEW_MODEL`
  (`opus`), `REVIEW_PERMISSION_MODE` (`plan`), `resolve_model` (abstains on an
  unknown alias — never silently defaults), `review_args` (`-p <prompt> --model
  <m> --permission-mode plan`, prompt as one argv entry). 8 tests.
- `src-tauri/src/commands/review.rs` — `start_review` / `cancel_review` /
  `review_models`. Builds an **ad-hoc** `Invocation` directly (NOT via
  `invocation::build`, which is config-locked), streams via the shared
  `run::forward`, registered under supervisor id `review:current`.
- `src-tauri/src/commands/run.rs` — `forward` made `pub(crate)`.
- `src-tauri/src/commands/enhancements.rs` — extracted `seeded_prompts_dir`
  (pub(crate)) so review runs a prompt from the same Prompts library.
- `src-tauri/src/lib.rs` — module + 3 commands registered.
- `src/ipc/api.ts` — `startReview` / `cancelReview` / `reviewModels`.
- `src/components/reviewLogic.ts` (+ `.test.ts`, 13 tests) — `defaultModel`,
  `defaultPromptId` (prefers `code-review`), `reviewStatus` (keeps cancelled /
  done / non-zero exit / spawn-failure distinct).
- `src/components/ReviewPanel.tsx` — overlay: prompt+model pickers, Run/Cancel,
  a reused `OutputConsole`. Mounts the console before the run so no event races
  arrival.
- `src/views/ChangesView.tsx` — `reviewOpen` state + Review button + overlay.
- `src/styles.css` — `.review-overlay/.review-panel/.review-console` (console
  pane is `overflow:hidden`, per the xterm rule).
- Docs: `docs/INDEX.md` regenerated; `docs/reference/commands.md` review section.

## Verified
- cb-core: 8 review tests + full suite green (the lone `process::` cancel
  failure is the documented flaky-under-load test; passes isolated).
- Frontend: `pnpm typecheck` clean; `pnpm test` 750 green (13 new).
- clippy `-p cb-core` clean for new code; `cargo fmt` clean; docs:check passes.
- **CLI smoke test**: `claude -p "…" --model haiku --permission-mode plan` runs
  headless, streams, exits 0 — the invocation shape is real, not assumed.

## Phase 1 follow-ups: Codex models + draggable panel (2026-08-18)

Two follow-ups done (a third — "diff into prompt context" — was deselected and
stays deferred). Implemented via a Workflow (2 parallel Opus agents → verify →
adversarial review).

- **Codex model selection from `~/.codex/config.toml`** (backend only):
  - `review.rs`: pure `parse_codex_models(&str)` (real `toml` parse; top-level
    `model` leads, then each `[profiles.*] model`, deduped first-seen; abstains to
    empty on missing/blank/non-string/parse-error). `codex_models()` fs wrapper
    (path via `intents::providers::codex::codex_home`; not unit-tested, like
    `detect_agents`). `models_for(agent)` single dispatch. `resolve_model` changed
    to take `available: &[String]` and return `Option<String>` (None/blank → first
    available else agent default; empty available ignores a request).
  - `commands/review.rs`: `review_agents` fills each row via `models_for`;
    `start_review` computes `available` and validates through the new signature.
    `agent_args` and `ReviewAgentInfo` unchanged → **no `types.ts`/frontend edit**
    (the picker already shows when `models.length > 0`).
  - 8 new tests in `review_tests.rs`; existing `resolve_model` tests migrated to
    the slice signature.

- **Draggable/resizable panel** (frontend only):
  - New `reviewLayoutLogic.ts` (+ `.test.ts`, 9 tests): `clampPanelPosition`
    (8px margin, per-axis clamp, oversized pins to origin) and
    `load/savePanelLayout` under `cb.agentPanel.layout` (mirrors `loadAgentPrefs`).
  - `ReviewPanel.tsx`: header pointer-drag with capture, ignores the —/✕ buttons,
    seeds `pos` from stored layout, switches to a top/left inline anchor when
    dragged. A pure click (no move) does **not** persist — `moved` guard added
    during review (a reviewer flagged the bottom-right→top-left flip on click).
  - `styles.css`: `resize: both`, min 360×280 / max 96vw×92vh, `cursor: move` on
    the header. Position persists; size is CSS-native, not persisted (deferred).

- **Gotcha (recorded):** a workflow agent ran a workspace-wide `cargo fmt` that
  reformatted ~30 pre-existing-drift files outside both slices. Reverted them with
  `git restore` to keep the diff focused and the per-hunk intent cards clean; the
  drift is pre-existing and left as-is.

## Verified (this follow-up)
- `cargo test -p cb-core --lib review`: 28 passed. `rustfmt --check` on the three
  changed Rust files: clean. `clippy -p cb-core`: only the pre-existing
  `rider.rs` `while_let` warning (not in new code).
- `pnpm test`: 792 passed (incl. 9 new). `pnpm typecheck`: clean.
- `docs/INDEX.md` regenerated (new pub fns picked up); `docs:check` passed.
- Manual `pnpm tauri dev` (Codex picker with a real config; drag/resize/persist)
  left as the manual step.

## Not done (manual)
Live end-to-end in `pnpm tauri dev` (click Review → stream → cancel; claude off
PATH → graceful Failed) — left as the manual verification step.
