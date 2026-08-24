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
- [x] Persist the resized size too (was the deferred nicety above). `PanelLayout`
      gains optional width/height (same numeric guards); pure `clampPanelSize`
      (floor 360×280, ceiling 96vw/92vh, floor wins). `ReviewPanel` seeds size
      from the layout and captures grip resizes via a `ResizeObserver` (grip fires
      no pointer event), debounced, only after a real user resize. 6 new tests.
      Commit 9dc1b54. (styles.css lines 877-880 comment now stale — size IS
      persisted; left untouched, out of that commit's slice.)

## Capture-side intent attribution (done — commit ad65e93)
- [x] Live-hook `SubagentStop`: added to `hooks_json::EVENTS` (propagates to
      install/detect/uninstall/command_line); new `HookEvent::SubagentStop` routed
      to `ingest_label` like `Stop` — but NEVER through `ask_for_intent` (the
      pre-existing `event != Stop` guard already excludes it, so a subagent stop
      can never block/exit-2). Joins to edits via the shipped path-scoped
      cross-turn binder; no turn-id match needed. 3 tests + the events-recognised
      test updated to three events.
- [x] Retroactive sidechain mining in `claude_code::read_transcript`: no longer
      skips `isSidechain`. First pass builds uuid→(parentUuid,isSidechain); second
      pass groups each sidechain line by its subagent-root uuid
      (`resolve_subagent_root`, cached, abstains on cycle/dangling/unknown parent).
      Shared `TurnState`/`process_entry` drives main (sub_root=None, id unchanged)
      and each subagent (`claude-history-{session}-sub-{root}-{block}`) so parallel
      interleaved subagents never share a turn. Labels stay `Inferred`. 5 tests
      incl. inverted sidechain-skip + interleaved-subagents key case.
- [x] Fixture fix: `providers_tests::existing_dashboard_hooks` now seeds a
      pre-existing `SubagentStop` hook so the additive-merge preservation test
      genuinely covers the new event (was the one full-suite failure).

## Follow-ups (optional, non-blocking)
- [ ] MANUAL empirical check (tests can't cover): with capture enabled, run a Task
      subagent that edits + ends with `Intent(paths): …`, inspect `.code-basics/
      intents/` — confirm a SubagentStop label recorded and Changes→Intent
      attributes the subagent's hunks. If the live payload lacks
      `last_assistant_message`, 3a is a no-op and 3b (mining) is the working path.
- [x] ~~NIT: `ReviewPanel` minimize→restore can persist the CSS-default size (the
      `first` guard only skips the initial observe).~~ **DONE (WF1, 2026-08-24, C7):**
      extracted a pure `createResizeGate()` in reviewLayoutLogic.ts (keeps the last
      non-zero size, ignores 0×0, never persists the mount default, persists only a
      genuine change); ReviewPanel uses it. 4 new gate tests.
- [ ] NIT (review, non-blocking): `resolve_subagent_root` caches only the queried
      uuid, not ancestors climbed — redundant walk for deep lineages, bounded by
      the cycle guard, correctness unaffected.

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

## Phase 3 — agent-process checks (done)
- [x] Business-rule invariants — new cb-core `rules` module (`RuleDoc{id,title,body}`,
      `rules_dir` = `.code-basics/rules`, `load_rules` erosion-style: bad/unreadable
      file → warning, missing dir → empty; reuses `enhancements::split_front_matter`).
      `list_rules` command + IPC (`RuleDoc`/`RulesReport`, key-pinned). `extract-rules`
      (edit-mode) writes the invariants; `verify-rules` (read-only) checks the diff
      against them; `ReviewPanel` shows `rulesRunHint` (0 rules → run extract first).
      `.code-basics/rules/` is committed (NOT in config IGNORED); `examples/rules/`
      as a template.
- [x] Claim/AC verification — prompt-context injection: `review::compose_prompt`
      prepends context before a prompt body; `start_review` gained optional
      `context`. `verify-claims` prompt + a frontend flow (`claimVerifyLogic.ts`:
      `behavioralReportToPromptContext` renders a `behavioral_diff` report to text,
      `verifyClaimsAction` gates it) → App opens the agent panel primed with the
      evidence as `initialContext` (token-keyed remount). No agent gets a live tool;
      the report is injected as read-only context.
- [x] Security / perf / concurrency / naming-drift prompt variants — four read-only
      lens prompts bundled in `src-tauri/resources/prompts/`, each demanding concrete
      evidence and abstaining ("say so plainly rather than inventing findings").
