# Todos — behavioral-testing

## Phase 1 — test-delta spine (pure, zero IO)  ✅ DONE
- [x] behavioral/mod.rs types + BehavioralDelta tagged union
- [x] behavioral/compare.rs diff_tests + tests (7)
- [x] register pub mod behavioral; key-pin in mod_tests.rs (10)
- [x] src/ipc/types.ts mirror; pnpm typecheck clean

## Phase 2 — worktree lifecycle  ✅ CORE DONE / prepare+command remain
- [x] clear_readonly_directories → pub(crate); Repo::head_oid()
- [x] behavioral/worktree.rs BaselineWorktree + teardown + clear_all + tests (4)
- [ ] `behavioral/prepare.rs`: build both sides via invocation::build (SEE notes.md open Q:
      scan worktree as Workspace, lookup config by id, abstain if absent); dep-drift handling
- [ ] `src-tauri/commands/behavioral.rs`: behavioral_diff (tests-only end-to-end); tee() helper
- [ ] register command in lib.rs invoke_handler!; api.ts wrapper (Channel like runTests)
- [ ] NEEDS LIVE WORKSPACE to verify end-to-end

## Phase 3 — console delta  ✅ DONE (7 tests)
## Phase 4 — attribution  ✅ DONE (8 tests)
## Phase 5 — HTTP
- [x] `behavioral/httpfile.rs` (pure parse + @readiness) — 7 tests
- [x] `behavioral/http.rs` (diff, volatile headers, JSON structural) — 9 tests
- [ ] `behavioral/replay.rs` (reqwest blocking, isolated) — NEEDS network; add reqwest dep
- [ ] sequential app runs (port conflict) — in command orchestration

## Phase 6 — frontend
- [ ] `behavioralPanelLogic.ts` + vitest
- [ ] IntentPanel.tsx per-card badge + overall panel; ChangesView "Run before/after"

## Integration  ✅ DONE (workflow wf_259bac05-56b + my post-review edits)
- [x] backend, frontend, gate, adversarial review — see completed.md
- [x] 2 review nits fixed (httpStatusTone + pickBehavioralConfig extracted & tested)
- [x] styles.css tone classes added
- [x] docs: commands.md, core-crate.md, docs:index, docs:check
- [x] independently re-verified all gates

## STILL OPEN
- [x] HTTP replay serverful launch — DONE + reviewed + hang/dup-key bugs fixed (see completed.md).
- [ ] (LOW/optional) extract the 2 inline abstain guard-clauses in behavioral.rs into a tested pure fn.
- [ ] LIVE GUI verify (pnpm tauri dev, click Run before/after in a real workspace w/ a test config);
      HTTP path needs a workspace with .http files (w/ @readiness) + an App launch config.
- [ ] Front-end still calls behavioralDiff(config.id, null, ...) → httpFiles=null, so HTTP auto-discovers
      .http files. UI to pick .http files / launch config explicitly is a future nicety.
