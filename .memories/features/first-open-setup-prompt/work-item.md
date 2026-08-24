# Work item: First-open setup prompt

## Goal
When a workspace opens and the agent hooks are not yet installed, show a **modal**
offering to set them up, so setup happens once and isn't forgotten.

## User decisions (2026-08-20)
- **Modal dialog on open** (not a banner).
- **Preview then install both** — reuse the preview-confirm; install intent
  capture + quality gate together.
- **Let me pick scope each time** — modal offers "For this repo" and "For me".
- (Implied) needs a "Don't ask again" so it doesn't nag every open.

## Correctness constraint
Intent capture and the quality gate both write Claude Code `settings.json`, and
each `PlannedWrite` carries the FULL file content computed at plan time. Applying
them independently would clobber. So a **combined plan** chains the merges into a
single settings.json write with both markers (`code-basics-intent` +
`code-basics-qgate`); other files (instructions, guard, whyhook, codex hooks) are
appended unchanged.

## Acceptance criteria
- [ ] cb-core `setup::setup_plan(root, scope, providers, gate_home)` → combined
      InstallPlan (settings.json chained), tested.
- [ ] settings_merge gains `merged_into_text(existing, ours, marker)`; qgate gains
      `install::merged_into(text, pin)`.
- [ ] Tauri `setup_install_plan(scope)` (preview) + `install_setup(scope)` (apply
      + exec bits + intents dir), registered.
- [ ] Frontend: modal `SetupPrompt` on open when not set up & not dismissed;
      pick scope → preview → confirm. "Not now" (session) / "Don't ask again"
      (localStorage per root). Decision logic in a tested *Logic.ts.
- [ ] PlanPreview shared (extracted from IntentPanel).
- [ ] docs + memory updated; cargo test + pnpm test + typecheck green.
