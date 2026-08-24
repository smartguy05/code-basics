# Plan: Installable quality-gate hook (intent-hook method)

## Decision (user, 2026-08-20)
Follow the **same method used for installing the intent hooks**. That means:
- The gate becomes a **self-invoking `cb-app.exe quality-gate` subcommand** (like
  `record-intent`), NOT a shipped `quality-gate.mjs`. Removes the committed
  `.claude/hooks/*.mjs`.
- Install via the existing machinery: `InstallPlan { writes: Vec<PlannedWrite> }`
  (pure) → preview in the setup panel → `apply_writes` (backup-then-write, `.bak`).
- settings.json **additive marker merge**, distinct marker `code-basics-qgate`
  so it coexists with the intent recorder's `code-basics-intent` Stop entry.
- Repo (Project) and User scope, same as intent hooks: Project pins `--workspace`,
  User leaves it off and resolves the workspace from the Stop payload cwd.

## Architecture (mirrors intents)
1. **cb-core decision logic** — new `crates/core/src/qgate/` (pure, tested):
   port the JS helpers (gatesForChanges, AI-REJECTED regex, memory reminder,
   loop guard) to Rust. This is where all decisions live.
2. **cb-core installer** — generalize `intents/providers/hooks_json.rs` merge to
   be (marker, events, entries)-parameterized; add a qgate `install_plan(root,
   scope) -> InstallPlan` + `status(root)` reusing `PlannedWrite`/`apply_writes`.
   Command line: `"<exe>" quality-gate [--workspace "<root>"]` via current_exe().
3. **src-tauri runner** — a `quality-gate` CLI arm (like `record-intent` in
   recorder.rs): read Stop payload from stdin, compute the change set (git),
   ask cb-core which checks to run, spawn `pnpm typecheck`/`cargo fmt --check`,
   exit 2 to block on failure. Thin: decisions in cb-core, spawning here.
4. **Tauri commands** — `quality_gate_status`, `quality_gate_install_plan`,
   `install_quality_gate`, registered in lib.rs; TS wrappers + types.
5. **Frontend** — a setup section (reuse IntentPanel CaptureSetup preview-confirm
   pattern, or a sibling) with Enable-for-repo/for-me/Re-apply + the plan preview.
6. **Dogfood migration** — replace this repo's hand-written `.claude/settings.json`
   qgate entry + delete `.claude/hooks/*.mjs`; regenerate via the installer path.
7. **Docs** — update development.md + agent-intent-capture (or new guide).

## Tests-first order
1. `qgate` decision logic tests (Rust) — mirror the .mjs test cases.
2. hooks_json generalization tests (marker param; intent path unchanged).
3. qgate install_plan/status tests (project vs user, idempotent re-install, .bak).
4. Build/typecheck; wire IPC + UI; dogfood; docs.

## Open sub-questions (resolve while implementing)
- User-scope gate: resolve workspace from payload cwd; no-op if repo lacks the
  tooling (no `typecheck` script / no Cargo.toml) so a global gate is safe.
- Keep `CB_GATE_FULL` (clippy opt-in) and `stop_hook_active` loop guard.
