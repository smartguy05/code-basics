# Todos — review tooling

## Phase 1 (done)
- [x] cb-core `review` helper (tests-first)
- [x] `start_review`/`cancel_review`/`review_models` commands
- [x] `api.ts` wrappers, `reviewLogic`, `ReviewPanel`, Changes-tab button
- [x] docs INDEX + commands.md

## Phase 1 follow-ups (optional)
- [x] Detect Claude Code / Codex; agent dropdown when both installed.
- [x] Non-blocking minimizable panel (loader pill), hosted at App level.
- [x] Persist last-chosen agent/model/prompt (localStorage) across opens.
      (reviewLogic `loadAgentPrefs`/`saveAgentPrefs`/`preferred*`; posture NOT
      persisted on purpose. Done in enhancements-run-agent follow-up.)
- [x] Menu-bar entry in `MenuBar.tsx` mirroring the Changes button.
      ("Review changes…" item under Enhancements → App `openReview`.)
- [x] Codex model selection — reads `~/.codex/config.toml`. `review::parse_codex_models`
      (pure: top-level `model` first, then `[profiles.*] model`, deduped; abstains to
      empty on missing/blank/non-string/parse-error), `codex_models()` (fs wrapper via
      `codex::codex_home`), `models_for(agent)` dispatch. `resolve_model` now takes the
      available `&[String]` and returns `Option<String>`. Command layer fills/validates
      through `models_for`. No IPC/types.ts change — picker appears automatically once
      Codex reports models. 8 new tests.
- [ ] Consider passing the working-tree diff into the prompt context explicitly
      (today the prompt file owns the "review the current changes" instruction;
      the agent explores the repo itself read-only). Codex has `codex review
      --uncommitted` as a dedicated alternative worth evaluating.
      (DEFERRED — deselected by Anthony 2026-08-18; leave the agent exploring itself.)
- [x] Make the floating panel draggable/resizable. `reviewLayoutLogic.ts`
      (`clampPanelPosition` keeps it on-screen; `load/savePanelLayout` under
      `cb.agentPanel.layout`), header pointer-drag in `ReviewPanel.tsx` (ignores the
      —/✕ buttons, switches to a top/left anchor; a pure click no longer persists),
      `resize: both` + min/max + `cursor: move` in styles.css. Position persists; size
      is CSS-native and not persisted (deferred nicety). 9 new tests.

## Phase 2 — deterministic tier (next)
- [ ] Secrets-in-diff scanner (regex on Added lines; reuse erosion scan plumbing;
      cite DiffLine::index). Tests-first with planted secrets. Highest ROI.
- [ ] Scope-creep alarm from Scorecard `unattributedLines` / `kind:"other"`.
- [ ] Risk-weighted card badges (path heuristics + erosion + kind); optionally
      expose per-hunk `AttributedSpan.confidence` (computed, not serialized).

## Phase 3 — agent-process checks (prompts feeding the button)
- [ ] Business-rule invariants (extract → `.code-basics/rules/*.md` → verify).
- [ ] Claim/AC verification prompt (reuse behavioral harness for runnable claims).
- [ ] Security / perf / concurrency / naming-drift prompt variants.
