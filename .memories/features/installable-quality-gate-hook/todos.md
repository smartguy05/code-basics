# Todos: Installable quality-gate hook

## Done
- [x] cb-core qgate decision logic + tests
- [x] settings_merge generalization + hooks_json delegation (no regression)
- [x] qgate installer (status/install_plan) + tests (coexistence, idempotent, .bak)
- [x] src-tauri runner (quality-gate subcommand) + lib.rs dispatch
- [x] Tauri commands + registration
- [x] TS api wrappers + frontend PlanPreview/QualityGateSetup
- [x] dogfood: repo settings.json → subcommand; delete .mjs
- [x] docs (CLAUDE.md, development.md, agent-intent-capture.md, commands.md, INDEX)

## Remaining
- [ ] Finish `cargo build --release -p cb-app` (running at hand-off) — the hook
      calls target/release/cb-app.exe, which must contain the subcommand.
- [ ] Run full `cargo test -p cb-core` and `pnpm test` (green before claiming done).
- [ ] Smoke test: `echo '{}' | cb-app.exe quality-gate --workspace <repo>` behaves
      (exit 0 clean tree; blocks on a planted bad .ts / AI-REJECTED note).
- [ ] Commit the feature.

## Possible follow-ups
- [x] ~~Wire an uninstall command~~ **DONE (WF3, 2026-08-24, B12+C1):** `qgate::install::uninstall_plan`
  + provider `uninstall_plan`; commands `quality_gate_uninstall_plan`/`uninstall_quality_gate`,
  `intent_uninstall_plan`/`disable_intent_capture`; "Disable…"/"Turn off…" buttons via PlanPreview.
  Shared pre/post-commit hooks removed only when no other provider still captures.
- [x] ~~Extend to Codex settings~~ **DONE (WF3, 2026-08-24, C2):** provider-aware `install_plan_for`/
  `status_for`/`settings_path`; targets `.codex/hooks.json`; first-open setup chains the gate into
  Codex's single hooks write. Old 3-arg `install_plan`/`status` kept as ClaudeCode wrappers.
- Consider a shared PlanPreview across other install flows if more appear.

## Follow-up opened by the WF3 review (testability seam)
- [ ] **`providers::uninstall_plan`'s shared-hook gating has no injection seam.**
  `another_provider_capturing` reads the real home dirs via `providers::all()`, so on a machine
  with global user-scope capture (both `~/.claude/settings.json` and `~/.codex/hooks.json`) it is
  always `true` — the "remove shared hooks when last provider" branch can't be deterministically
  unit-tested there. Logic is correct; the test (`shared_hooks_are_removed_only_when_no_other_provider_still_captures`)
  pins the direction biconditionally and actively verifies the keep-in-place branch. If this is
  revisited, add a home-dir injection seam so both directions test deterministically.
