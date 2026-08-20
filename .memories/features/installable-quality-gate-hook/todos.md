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

## Possible follow-ups (not required)
- Wire an uninstall command (settings_merge::plan_removal already generic).
- Extend to Codex settings if desired (currently Claude Code settings.json only).
- Consider a shared PlanPreview across other install flows if more appear.
