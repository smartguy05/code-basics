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

## Final verification (all done)
- Release rebuilt (9.5 MB, contains the subcommand); loop guard exits 0 headless.
- Real run on the working tree: ran cargo fmt --check + pnpm typecheck → exit 0.
- Blocking smoke: planted bad .ts → exit 2 with tsc output; planted AI-REJECTED
  note → exit 2 with fix message.
- `cargo test -p cb-core` → 2183 + all integration suites pass (0 failed).
- `pnpm test` → 825 pass. `pnpm typecheck` clean. docs:check passes.
- Committed.
