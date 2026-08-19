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

## Phase 2 — deterministic tier (done)
- [x] Secrets-in-diff scanner — new `ErosionCategory::Secret` + 6 Added-side
      built-in rules (AWS/private-key/GitHub/Slack tight; secret-assignment &
      connection-password tightened to exclude env-var/placeholder indirections
      and require a long whitespace-free literal). Reuses the category-generic
      erosion scan; cites DiffLine::index. Frontend: union + CATEGORY_ORDER
      (first) + label. Tests use the REAL builtins (leaks fire, placeholders
      don't).
- [x] Scope-creep alarm — pure `scopeCreep(scorecard, groups)` in
      intentPanelLogic.ts (no IPC change): unexplained-`other`-group count +
      unattributed **share of sum(lineCount)** (NOT summed with unattributed —
      that double-count made "high" unreachable). Informational banner in
      IntentPanel; abstains below 40 changed lines; "high" needs both signals.
- [x] Risk-weighted card badges — pure `cardRisk(group, erosionFlags)`: kind
      `other` / low confidence / boundary-matched sensitive path (dropped
      `config`, guarded `auth`≠`author`) / erosion flag intersecting the card's
      own path+lineIndices (secret/removedSafeguard/deletedAssertion ⇒ high).
      Quiet by default (null ⇒ no badge). ChangesView now scans erosion for the
      intent view too and clears it on mode change so indices stay aligned.
      Used group.confidence — did NOT expose AttributedSpan over IPC.

## Phase 3 — agent-process checks (prompts feeding the button)
- [ ] Business-rule invariants (extract → `.code-basics/rules/*.md` → verify).
- [ ] Claim/AC verification prompt (reuse behavioral harness for runnable claims).
- [ ] Security / perf / concurrency / naming-drift prompt variants.
