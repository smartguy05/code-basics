# Completed: Installable quality-gate hook

## What shipped
The quality-gate Stop hook is now installed the same way the intent hooks are —
a self-invoking `cb-app.exe quality-gate` subcommand (no shipped script),
installed via `InstallPlan`/`apply_writes` with a preview-confirm panel.

## Files added
- `crates/core/src/qgate/mod.rs` — pure decision logic (ported from the old
  `quality-gate-logic.mjs`): `gates_for_changes`, `touched_source/memories`,
  `should_remind_memories`, `has_unresolved_rejection` (manual scan, no regex),
  `should_skip_for_loop`, `has_typecheck_script`, arg parsing
  (`is_quality_gate_invocation`, `parse_qgate_args`), `Gate` enum with
  `command()`/`label()`. + `decide_tests.rs` (14 tests).
- `crates/core/src/qgate/install.rs` — installer: `status`, `install_plan`,
  marker `code-basics-qgate`, `EVENTS=["Stop"]`, timeout 180. + `install_tests.rs`
  (6 tests incl. coexistence with the recorder + idempotent + .bak).
- `crates/core/src/intents/providers/settings_merge.rs` — generic marker-based
  settings.json merge (`contains_marker`, `is_installed`, `merged_text`,
  `plan_removal`), extracted from `hooks_json` so both hooks share it.
- `src-tauri/src/qgate_run.rs` — thin runner: reads Stop payload, git change set,
  spawns checks via `process::resolve_program`, exit 2 to block. Guards each
  gate on repo tooling (`has_typecheck_script` / `Cargo.toml`) for user scope.
- `src-tauri/src/commands/qgate.rs` — `quality_gate_status`,
  `quality_gate_install_plan`, `install_quality_gate`.
- Frontend: `src/ipc/api.ts` wrappers; `IntentPanel.tsx` gained `PlanPreview`
  (extracted, shared) + `QualityGateSetup` (self-contained), rendered under the
  setup twisty.

## Files changed
- `hooks_json.rs` — `is_installed`/`plan_merge`/`plan_removal` now delegate to
  `settings_merge` (public API unchanged; 373 intent tests still pass).
- `claude_code.rs` — `claude_home`/`project_settings_path`/`user_settings_path`
  made `pub` for reuse.
- `lib.rs` (cb-core) — `pub mod qgate;`
- `lib.rs` (src-tauri) — `mod qgate_run;`, dispatch arm, `commands::qgate`
  module, 3 handlers registered.
- Docs: CLAUDE.md, development.md, agent-intent-capture.md, reference/commands.md,
  INDEX.md (regenerated).

## Dogfood migration
- This repo's `.claude/settings.json` now calls `cb-app.exe quality-gate
  --code-basics-qgate --workspace ...` (was `node .claude/hooks/quality-gate.mjs`).
- Deleted `.claude/hooks/*.mjs` (logic moved to Rust).

## Verified
- `cargo test -p cb-core qgate::` → 20/20 pass.
- `cargo test -p cb-core intents::` → 373 pass (refactor no regression).
- `cargo check --workspace --all-targets` clean; clippy warnings only pre-existing
  (rider.rs, workspace.rs, review.rs).
- `pnpm typecheck` clean. docs:check passes; index regenerated (+3 commands).

## Follow-up: provider-aware install (unit C2)
The gate installer is now provider-aware so it can target Codex's `.codex/hooks.json`
as well as Claude Code's `settings.json` (the gate's Stop entry is byte-identical for
both — `quality-gate` takes no `--provider` flag — so only the path/`provider`/caveats
differ).
- `install.rs`: added `status_for(provider, root, home)`, `install_plan_for(provider,
  root, scope, home)`, and `pub fn settings_path(provider, root, scope, home)`; old
  `status`/`install_plan` are thin ClaudeCode wrappers (all 6 prior install tests +
  callers stay green). Caveats split into `caveats_for` → `caveats` (Claude) /
  `codex_caveats` (shared-file + untrusted-project via `codex::is_trusted_in`, now
  `pub(crate)`, + first-run review note).
- `install_tests.rs`: +2 tests — `codex_project_plan_writes_codex_hooks_json`,
  `codex_gate_coexists_with_codex_intent_recorder` (both markers survive in the Stop
  array of `.codex/hooks.json`). qgate suite now 22/22.
- `setup.rs`: combined first-open plan chains the gate's Codex entry onto the SAME
  `.codex/hooks.json` write Codex intent capture produces (only when that write
  already exists — never introduces a `.codex/` file), mirroring the existing Claude
  settings.json chaining. Tolerant of an unresolvable Codex home. setup 4/4.
- IPC: `commands/qgate.rs` threads `provider: ProviderId` into all three commands;
  `api.ts` wrappers + `IntentPanel.tsx` `QualityGateSetup` now lists a gate block per
  detected agent (`QualityGateProvider`), like the intent-capture block. `App.tsx`
  first-open check passes `"claudeCode"`. `docs/reference/commands.md` updated.
- NOTE for next unit (B12+C1): qgate has no uninstall yet — add it provider-aware,
  reusing `settings_path`. No Codex-only uninstall was added here.
- Verified: `cargo test -p cb-core` qgate 22 / setup 4 / codex 77 all pass;
  `cargo check -p cb-app` clean; `cargo fmt --check` clean; `pnpm typecheck` clean;
  `pnpm test` 857 pass; docs:check passes.

## Final verification (all done)
- Release rebuilt (9.5 MB, contains the subcommand); loop guard exits 0 headless.
- Real run on the working tree: ran cargo fmt --check + pnpm typecheck → exit 0.
- Blocking smoke: planted bad .ts → exit 2 with tsc output; planted AI-REJECTED
  note → exit 2 with fix message.
- `cargo test -p cb-core` → 2183 + all integration suites pass (0 failed).
- `pnpm test` → 825 pass. `pnpm typecheck` clean. docs:check passes.
- Committed.

## B12C1 — provider-aware uninstall (done)
- CORE: `qgate::install::uninstall_plan(root,scope,home)` + provider-aware
  `uninstall_plan_for(provider,...)`: reuses `settings_path`, calls
  `settings_merge::plan_removal(EVENTS,MARKER)` → one PlannedWrite or zero writes.
  Distinct qgate marker means the recorder's Stop entry survives.
- CORE: `providers::uninstall_plan(provider,root,scope)`: removes only that
  provider's own hook entries (`hooks_json::plan_removal` on its settings_path);
  appends guard(pre-commit)+whyhook(post-commit) removals ONLY when no OTHER
  provider is still capturing (`another_provider_capturing` checks the others'
  `status(root).capture`). CLAUDE.md/AGENTS.md instruction note left in place.
- IPC (no new types — reuses InstallPlan/ProviderStatus): commands
  `quality_gate_uninstall_plan`/`uninstall_quality_gate` (qgate.rs),
  `intent_uninstall_plan`/`disable_intent_capture` (intents.rs); 4 registered
  in lib.rs; 4 api.ts wrappers.
- FRONTEND: IntentPanel CaptureSetup "Disable…" + QualityGateProvider "Turn off…",
  both preview the plan in PlanPreview and show "nothing to remove" on an empty
  plan; ChangesView `disableCapture` handler wired via new `onDisable` prop.
  SetupPrompt left as-is (only shows when nothing is installed — no-op there).
- Tests-first: `install_tests::uninstall_plan_removes_gate_and_spares_the_recorder`
  (+clean-workspace zero-write) and `providers_tests::disable_removes_only_that_
  providers_entries`. Confirmed red (fn missing) then green.
- Verified: cargo fmt --check clean; cargo check -p cb-app clean; qgate 24 /
  intents 391 pass; pnpm typecheck clean; pnpm test 857 pass; docs:index +
  docs:check pass. Did NOT rebuild release (orchestrator handles it).
