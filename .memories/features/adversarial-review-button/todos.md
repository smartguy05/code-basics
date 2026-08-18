# Todos — review tooling

## Phase 1 (done)
- [x] cb-core `review` helper (tests-first)
- [x] `start_review`/`cancel_review`/`review_models` commands
- [x] `api.ts` wrappers, `reviewLogic`, `ReviewPanel`, Changes-tab button
- [x] docs INDEX + commands.md

## Phase 1 follow-ups (optional)
- [x] Detect Claude Code / Codex; agent dropdown when both installed.
- [x] Non-blocking minimizable panel (loader pill), hosted at App level.
- [ ] Persist last-chosen agent/model/prompt (localStorage) across opens.
- [ ] Menu-bar entry in `MenuBar.tsx` mirroring the Changes button.
- [ ] Codex model selection (currently uses its configured default; would need a
      verified model list or reading `~/.codex/config.toml`).
- [ ] Consider passing the working-tree diff into the prompt context explicitly
      (today the prompt file owns the "review the current changes" instruction;
      the agent explores the repo itself read-only). Codex has `codex review
      --uncommitted` as a dedicated alternative worth evaluating.
- [ ] Make the floating panel draggable/resizable (fixed bottom-right for now).

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
