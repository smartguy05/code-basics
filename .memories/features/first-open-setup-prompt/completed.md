# Completed: First-open setup prompt

## What shipped
Opening a workspace with no agent hooks installed shows a **modal** offering to
install intent capture + the quality gate together. Pick scope (this repo / just
me) → preview the exact writes → confirm. "Not now" (session) / "Don't ask again"
(per-workspace localStorage). Once either hook is installed the check stops
raising it.

## Files added
- `crates/core/src/setup.rs` (+ `setup_tests.rs`, 4 tests) — `setup_plan` builds
  ONE combined `InstallPlan`, chaining the gate's Stop entry onto the intent
  recorder's settings.json (via `qgate::install::merged_into`) so both markers
  land in one write instead of clobbering. Drops the gate's redundant caveat when
  chained; keeps it when the gate stands alone.
- `src-tauri/src/commands/setup.rs` — `setup_install_plan` (preview),
  `install_setup` (apply + exec bits + intents dir). Registered in lib.rs.
- `src/components/SetupPrompt.tsx` — the modal (reuses `.modal-*` classes).
- `src/components/PlanPreview.tsx` — extracted from IntentPanel; shared preview
  (added optional `confirmLabel`).
- `src/components/setupPromptLogic.ts` (+ `.test.ts`, 11 tests) — `needsSetup`,
  `shouldPrompt`, `isDismissed`/`setDismissed` (localStorage per root).

## Files changed
- `settings_merge.rs` — added `merged_into_text(existing, ours, marker)`;
  `merged_text` now delegates to it.
- `qgate/install.rs` — added `pub merged_into`.
- `crates/core/src/lib.rs` — `pub mod setup;`
- `src/ipc/api.ts` — `setupInstallPlan` / `installSetup` wrappers.
- `src/components/IntentPanel.tsx` — import shared PlanPreview (removed local copy).
- `src/App.tsx` — `showSetup` state, effect on `workspace.root` calling both
  status APIs → `shouldPrompt`, and the modal render near the other overlays.
- Docs: CLAUDE.md (overlays), agent-intent-capture.md, reference/commands.md,
  INDEX regenerated.

## Correctness note
Combined plan chains merges so the previewed settings.json = what lands (one
write, both markers). Verified by `applying_the_plan_installs_both_hooks`.

## Verified
- cb-core: setup 4, qgate 20, intents providers 183 pass; full lib 2186 pass +
  1 known-flaky `process::cancel_stops_a_long_running_process` (passes in
  isolation; process/ untouched).
- Frontend: 836 pass (incl. 11 new); `pnpm typecheck` clean.
- clippy clean for new modules; `cargo check --workspace --all-targets` clean.
- docs:check passes; INDEX regenerated (+2 commands, +1 module).
- Committed.
