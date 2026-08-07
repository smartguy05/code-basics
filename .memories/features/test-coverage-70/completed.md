# Completed

## Phase 4 — frontend vitest (2026-08-07, workflow wf_f07a759f-58d, tooling + 5 Opus extractors + verify)
- vitest 4.1.10 + @vitest/coverage-v8 added (pnpm, no package-lock.json); `pnpm test` / `pnpm coverage` scripts; vite.config.ts extended in place (vitest/config, node env, coverage over `src/**/*Logic.ts` + language.ts, lines threshold 70).
- 10 logic modules extracted mechanically (verify agent diffed the biggest three against HEAD — bodies byte-equivalent): testsLogic, consoleLogic, inspectLogic, diffLogic, configLogic, changesLogic, historyLogic, treeLogic, recentsLogic (+ language.ts tested in place).
- **206 frontend tests, logic-module coverage 99.49% lines / 100% functions.** typecheck + build clean.
- Tech debt noted: InspectView re-exports preferApplicationProcess (for RunView) and OutputConsole re-exports stripAnsi (for TestsView) — importers should later point at the logic modules directly.
- Gotcha: vitest 4's coverage text table omits fully-covered files — use coverage-summary.json, don't trust the terminal table.

## Phase 5 — docs + final gates (2026-08-07, inline)
- CLAUDE.md: commands (pnpm test/coverage, llvm-cov gate + sh-on-PATH warning), quality-gate paragraph, tests-first bullet (frontend *Logic.ts convention), cb-core module list gained invocation.rs, src-tauri bullet now points at cb_core::invocation::build as the single dispatch point, frontend section mentions *Logic.ts.
- docs/guides/development.md: command table + coverage section + frontend convention; docs/architecture/frontend.md:83 rewritten. docs:check passes (20 files).
- **Final gates: docs:check ok; typecheck ok; pnpm test 206/206; pnpm coverage 99.49% lines (threshold 70); cargo fmt --check clean; clippy only the 2 pre-existing warnings; cargo llvm-cov gate exit 0 — TOTAL 86.27% lines / 85.50% regions / 73.74% functions; 961 lib + 44 + 2 + 2 tests, 0 failures.**
- One flaky failure observed in an intermediate coverage run (cb-core lib, exit 101); identifying output was lost to a `tail` pipe, and the immediate full rerun was 100% green twice-measured. Watch for recurrence.
- NOTE: all work is UNCOMMITTED on branch claude/lightweight-ide-replacement-52cp2n (user did not ask for commits). Pre-existing uncommitted docs edits (docs/INDEX.md, README.md, using-the-app.md, development.md) were present before this work began.

## Phase 3 — src-tauri → cb-core refactor (2026-08-07, workflow wf_670a207e-fd7, 6 Opus refactorers + verify)
- **Coverage: 84.16% → 86.27% lines** (11290 lines, 1550 missed). Functions 73.74%.
- New cb-core API: `invocation::{build, plan_compound, rerun_filter}` (invocation.rs moved wholesale from src-tauri — single dispatch point confirmed by grep, src-tauri file deleted via git mv), `workspace::workspace_from_dir`, `git::repo::{NetworkKind, resolve_network}` (serde attrs verbatim, re-exported from commands/git.rs — IPC surface unchanged), `inspect::session::{AttemptOutcome, attempt_outcome, other_bitness, retry_bitness, enumeration_outcome}`, `intents::rebase_seqs`, `secrets::resolve_project_path`, `intents::hook` recorder-arg parsing. ~60 new tests.
- Verify agent: cargo check/test workspace green (961 lib + 44 + 2 + 2 cb-app), fmt clean, clippy only the 2 pre-existing warnings, `git diff --stat -- src/` empty (types.ts untouched), no test attributes deleted, docs/INDEX.md regenerated (docs:index + docs:check pass), pnpm typecheck clean.
- **Behaviour deltas for review (deliberate, tested):** (a) bad CLI path now prints `code-basics: {e}` — scan-failure loses the "could not open <path>:" prefix; (b) compound start errors now aggregate ALL member failures joined "; " instead of first-only; (c) rebase_seqs preserves the original prefix-sum arithmetic exactly (characterized: [0,1,2] from base 0 → [0,1,3]).

## Phase 0 — tooling + baseline (2026-08-07)
- Installed cargo-llvm-cov 0.8.7 + llvm-tools-preview.
- Baseline: **78.30% lines** workspace (exclusions: src-tauri/src/main.rs, process/kill.rs).
- Found + documented: process:: tests hang without `sh` on PATH → all cargo runs go through Git Bash (notes.md).
- Phase 2 cancelled: repo.rs 91.6%, trx 94.3%, junit 95.5%, solution 94.3% already.

## Phase 1 — cb-core gap tests (2026-08-07, workflow wf_33e5146b-7be, 3 Opus writers + verify)
- **Coverage: 78.30% → 84.16% lines** (11076 lines, 1754 missed). Functions 66.49% → 71.74%.
- codex.rs 20.93% → **95.89%** (60 tests; home-dir seam via `*_in(home: Option<&Path>)` inherent methods, Provider impl delegates; no public API change).
- claude_code.rs 15.58% → **97.53%** (63 tests incl. providers_tests; seam via `ClaudeCode::with_home(...)`, `new()` unchanged).
- providers/mod.rs 53.12% → **85.94%**.
- Batch agent: +86 tests across intents/hook/manifest/dotnet/config/workspace/model/sidecar/session/changelists/attribution; fixed the three process:: spin loops with `tokio::time::timeout(30s)` + "is sh on PATH?" expect message.
- Suite: 894 lib + 44 git_operations + 2 intent_attribution, 0 failures. fmt clean. Clippy: 2 pre-existing warnings (rider.rs:65 while_let_loop, workspace.rs:945 cmp_owned in an old test) — left alone, verified pre-existing vs HEAD.
- Verify agent confirmed no test attributes deleted anywhere (`git diff -U0` scan).
- Note: `Invocation::new` from the plan does not exist (grep-verified) — replaced with 5 Invocation IPC-shape tests.
