# Completed — Adversarial review button (Phase 1)

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

## Not done (manual)
Live end-to-end in `pnpm tauri dev` (click Review → stream → cancel; claude off
PATH → graceful Failed) — left as the manual verification step.
