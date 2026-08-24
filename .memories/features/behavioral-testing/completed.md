# Completed — behavioral-testing

## Phase 1 — test-delta spine + wire contract  ✅ (2026-08-18)
Files:
- `crates/core/src/behavioral/mod.rs` — BehavioralReport, BehavioralScorecard,
  BehavioralDelta (internally tagged on `kind`), ConsoleDelta, HeaderChange,
  BodyDelta, HttpDelta, CardBehavior. Runtime twin of `git/coverage.rs::IntentReview`.
- `crates/core/src/behavioral/compare.rs` — `diff_tests(base, work) -> TestDelta`;
  CaseTransition (Unchanged/Fixed/Regressed/StillFailing/Added/Removed), CaseDelta.
  Join by `full_name`. Rule: `is_pass = Passed only`; `is_fail = Failed|Other`
  (Other never a pass); Skipped transitions → Unchanged (abstain). Unchanged cases
  omitted from output; deterministic sort by full_name.
- `crates/core/src/behavioral/compare_tests.rs` — 7 tests (regression/fix/other-never-pass/
  still-failing/unchanged-omitted/added-removed/summaries).
- `crates/core/src/behavioral/mod_tests.rs` — 10 key-pin tests guarding serde JSON keys
  vs the hand-written TS mirror.
- `crates/core/src/lib.rs` — `pub mod behavioral;`
- `src/ipc/types.ts` — hand mirror of all the above (after IntentReview).

Verified: `cargo test -p cb-core --lib behavioral` → 17 passed. `pnpm typecheck` clean.

Note: key-pin lives in `behavioral/mod_tests.rs` (co-located), not model.rs — the new
types are in the behavioral module. Same `keys()` guard pattern as model.rs.

## Phase 2 (partial) — worktree lifecycle  ✅ (2026-08-18)
Files:
- `crates/core/src/git/repo.rs` — `clear_readonly_directories` promoted to `pub(crate)`;
  new `Repo::head_oid() -> Result<String>` (git2 read; errors on unborn branch).
- `crates/core/src/behavioral/worktree.rs` — `BaselineWorktree` (Drop-guarded),
  `WorktreeOptions{cache_by_oid}`, `teardown()`, `clear_all()`. Shells out to
  `git -C <root> worktree add --detach/remove --force/prune`. Location:
  `.code-basics/behavioral/base/<short-oid>/` (short = first 12 hex). `keep` controls
  teardown-on-drop; `adopted` = reused an existing checkout. `keep_for_reuse()` marks a
  fresh build good so it survives as cache; `sweep_other_oids` bounds disk to ~1 baseline.
  Teardown returns warnings (never hard error) and reuses `clear_readonly_directories` +
  150ms retry for Windows locks.
- `crates/core/src/behavioral/worktree_tests.rs` — 4 tests (real git worktree round-trip):
  teardown_removes_dir, teardown_is_idempotent, drop_removes_an_unkept_checkout,
  create_is_cache_hit_at_same_oid.
- `mod.rs` re-exports BaselineWorktree, WorktreeOptions.

Verified: `cargo test -p cb-core --lib behavioral` → 21 passed. clippy clean (the 1
remaining warning is pre-existing in importers/rider.rs, untouched).

## Phase 3 — console delta  ✅ (2026-08-18)
- `behavioral/console.rs` — `diff_console(base, work, &ConsoleNormalization) -> ConsoleDelta`.
  Strips ANSI, masks ISO/clock/epoch timestamps, GUIDs/long-hex ids, and both run roots →
  `<root>`. Compares as MULTISETS (interleave order is noise). Abstain: equal-after-masking →
  no delta at High; a real delta is Medium (console = weak evidence), dropping to Low when
  masking touched >heavy_mask_fraction (0.5) of lines OR ignore_ordering set.
  `ConsoleDelta::is_change()`. Shared helpers `mask_timestamps_and_ids`, `multiset_minus`
  are `pub(crate)` (reused by http.rs). 7 tests.

## Phase 4 — attribution  ✅ (2026-08-18)
- `behavioral/attribute.rs` — `attribute_behavioral(deltas, &[IntentGroup]) ->
  (Vec<CardBehavior>, Vec<BehavioralDelta> unattributed)`. Delta pinned to a group ONLY if
  its candidate files land in exactly one group; 0 or ≥2 → unattributed (never guessed,
  never split). Candidate paths: Test→files_hint; Console→group files named in changed
  lines; HTTP→none (unattributed by design). Card confidence = weakest member (Test capped
  Medium). Cards sorted by group_id. 8 tests.

## Phase 5 (pure parts) — .http parse + response diff  ✅ (2026-08-18)
- `behavioral/httpfile.rs` — `parse_http_file(text) -> HttpScenario` (PURE). VS Code REST
  Client / JetBrains syntax: ### separators + trailing name, # @name, METHOD url [HTTP/x],
  headers, body, @var + {{var}} substitution. `# @readiness METHOD url STATUS [timeout=
  interval=]` convention. Abstains: `> {% %}` scripts skipped w/ warning; unresolved
  {{var}} left in place w/ warning; readiness missing status ignored w/ warning. Types
  HttpRequestSpec/Readiness/HttpScenario are plain (NOT IPC — internal to replay). 7 tests.
- `behavioral/http.rs` — `diff_http(name, before, after, ignore) -> HttpDelta` (PURE).
  RecordedResponse{status,headers,body,content_type}. VOLATILE_HEADERS ignored (date/server/
  request-id/etag/…) + caller ignore list. JSON bodies canonicalized (keys sorted) THEN
  masked THEN compared → key order & timestamps never false deltas. Confidence: status change
  = High; body-declared-json-unparseable = Low; else header/body change = Medium; none = High.
  `HttpDelta::is_change()`. 9 tests.
  BUGFIX during dev: JSON branch must mask-then-compare (was comparing Values before masking →
  empty-but-Some body delta for a timestamp-only diff).

Verified: `cargo test -p cb-core --lib behavioral` → 52 passed; full lib suite → 2070 passed.
clippy clean in behavioral. All PURE, headless — no live app needed.

## Integration — backend + frontend + docs  ✅ (2026-08-18, via workflow wf_259bac05-56b)
Backend (agent, verified independently):
- `crates/core/Cargo.toml` — reqwest 0.13.4 { default-features=false, features=["blocking","rustls","json"] }.
  NOTE: reqwest 0.13 feature is `rustls` (not 0.12's `rustls-tls`).
- `behavioral/prepare.rs` (+tests) — scan_baseline, find_config, pure `assemble_report` (tested).
  Scorecard.abstained = warnings.len(). files_hint LEFT EMPTY (safe abstain; console still attributes by line scan).
- `behavioral/replay.rs` (+tests) — blocking reqwest: await_ready (poll+timeout), send, record_from_parts.
- `src-tauri/src/commands/behavioral.rs` — behavioral_diff + behavioral_clear. tee() captures stdout to
  Arc<Mutex<String>> while forwarding; distinct :base/:work ids; stale reports deleted; worktree
  keep_for_reuse on success, finish() warnings drained; every failure → empty report + warning (never errors).
- `src-tauri/src/lib.rs` — both commands registered.
Frontend (agent + my post-review edits):
- `src/ipc/api.ts` — behavioralDiff (Channel like runTests) + behavioralClear.
- `src/components/behavioralPanelLogic.ts` (+test, 737 total) — behavioralBadge, behavioralScoreLine,
  transitionTone, deltaLine, + EXTRACTED httpStatusTone & pickBehavioralConfig (post-review nits).
- `src/components/IntentPanel.tsx` — optional behavioral prop; per-card badge + overall panel; unattributed
  never pinned to a card.
- `src/views/ChangesView.tsx` — "Run before/after" button; uses pickBehavioralConfig.
- `src/styles.css` — behavioral-badge/-overall/-delta/-run + tone classes (--pass/--fail/--text-faint).
Docs: commands.md (behavioral_diff/clear), core-crate.md (## behavioral), pnpm docs:index (95 cmds), docs:check pass.

Reviewer verdict: CLEAN + contract-accurate. 2 low nits (deltaLine tone untested, config-select inline) → BOTH FIXED.
Verified independently: cargo test behavioral 60✓, cargo check --workspace✓, clippy (only pre-existing rider.rs)✓,
pnpm typecheck✓, pnpm test 737✓, docs:check✓.

## Serverful HTTP replay  ✅ (2026-08-18, workflow wf_332d980c-f69 + my review fixes)
- httpfile.rs: discover_http_files(root) via workspace::source_walker (SKIP_DIRS-honoring), *.http/*.rest.
- scenario.rs (NEW): pure decision seam — SideResult, pair_and_diff (readiness gate → per-key diff, abstain
  on unready/missing/error, is_change()-gated), plan_replay (flatten, key="{path}#{name}", first readiness),
  choose_launch_config (App kind: passed-if-App else sole-App else Abstain).
- crates/core/tests/behavioral_replay.rs (NEW): REAL 127.0.0.1:0 test — await_ready + send + closed-port timeout.
- commands/behavioral.rs: run_http_side (spawn-run → spawn_blocking(await_ready) → spawn_blocking(send) →
  cancel → await), run_http_replay orchestrator. SEQUENTIAL base(worktree) then work(real). http deltas
  computed BEFORE assemble_report.
- Reviewer verdict: correct on all critical axes. 3 findings:
  * MEDIUM (real hang) FIXED: cancel/registration race — cancel could return false before run() registered
    the pid (reachable via tiny @readiness timeout), then run_handle.await hung forever + leaked the server.
    Fix (behavioral.rs run_http_side): retry cancel up to 100×50ms until it takes OR run_handle.is_finished(),
    then `tokio::time::timeout(10s, run_handle)` so it can NEVER hang.
  * LOW (silent drop) FIXED: duplicate @name in one .http collided on the BTreeMap key → dropped one request.
    Fix (scenario.rs plan_replay): occurrence-suffix repeats (`key`, `key#1`, …); first keeps plain key.
    +test plan_disambiguates_duplicate_request_names.
  * LOW (style, NOT a defect, DEFERRED): two abstain guard-clauses (no-@readiness, base-config-missing-at-HEAD)
    live inline in the untestable command rather than a pure fn. Behavior correct; extraction is marginal.
Verified: cargo test behavioral 73✓, behavioral_replay 2✓, cargo check --workspace✓, clippy cb-core+cb-app clean.

## STILL DEFERRED (honest gaps)
1. HTTP replay serverful launch NOT wired — replay.rs/diff_http/httpfile all done+tested, but the command
   parses .http and abstains the actual send with a warning (report.http empty). Follow-up: drive
   base-app-up→await_ready→replay→down, THEN work-app→replay→down (sequential, same port).
2. LIVE GUI verification not done (can't click button headlessly). Backend compiles+registered; needs a
   real workspace + `pnpm tauri dev` click-through by user (or me launching to confirm boot).
