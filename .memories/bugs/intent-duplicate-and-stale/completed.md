# Intent records duplicated + panel stale until file reopen

Two independent bugs found while testing multi-intent capture on one file.

## 1. Every edit and label written twice

**Root cause:** the `record-intent` hook was installed at **both** scopes and
Claude Code runs both per event — project `.claude/settings.json` (explicit
`--workspace`) and user `~/.claude/settings.json` (no `--workspace`, fires for
every repo). So each PostToolUse/Stop recorded twice. Masked on read
(`load_edits` dedups by `tool_use_id`; labels dedup downstream) so it only
bloated `edits.jsonl`/`labels.jsonl` — the unbounded-growth risk.

**Fix (code, "most specific wins" — no config edit):**
- `intents/hook.rs`: `defers_to_project_hook(provider, had_explicit_workspace,
  project_hook_present)` (pure) + `project_record_hook_present(root)` (reuses
  `hooks_json::pinned_workspace` / `pinned_elsewhere` to detect a project hook
  pinned to this root).
- `src-tauri/src/recorder.rs`: after `is_enabled`, a global (no `--workspace`)
  Claude Code invocation stands down when the workspace has its own project
  hook. Fails toward recording (unreadable settings ⇒ record).
- Tests in `hook_tests.rs` (3 new). Claude Code only; Codex unaffected.

**Not live until the installed `cb-app.exe` (Program Files) is rebuilt/reinstalled** —
the hooks run the installed binary, not the dev build. Existing on-disk
duplicates are harmless (deduped on read); not cleaned up.

## 2. Cards stale until the file is closed/reopened

**Root cause:** intent cards recompute only on the 2s working-tree poll (skipped
while `document.hidden`) and visibility edges. Hooks write records from a
separate process (typically while the user watches the agent, app backgrounded),
and a Stop-hook label / retire changes nothing in git status — so the panel held
stale cards, and a live selection's `highlight` was captured at click time so it
painted against the renumbered diff (the "new card highlights old region"
symptom).

**Fix (frontend):**
- `views/changesLogic.ts`: `reconcileSelection(groups, selectedGroupId,
  selectedPath)` (pure, 5 tests) → skip / update / clear.
- `views/useChangesModel.ts`: two effects — `visibilitychange` → `refreshAll`
  (fresh cards the moment the window returns), and reconcile the selected card's
  highlight/hunks against freshly-loaded groups (de-churned via `sameIndices`).

## 3. "Loading intents" flashing every 2s

**Root cause:** `refreshIntent` set `intentLoading` true→false on every call, and
the poll calls it every 2s — so the indicator flashed constantly even when the
de-churn found no change.

**Fix (`useChangesModel.ts`):** toggle the loading flag only on a *first load* —
`intentSignature.current === null`, which holds on initial mount and after a mode
switch (the `[mode]` effect nulls it). Background poll/refresh ticks with data
already committed never touch it. Plumbing (React state timing); no unit test.

## Gates
`cargo test -p cb-core --lib intents::hook::` 85 pass; `cargo fmt --check` clean;
`cargo check -p cb-app` clean; `pnpm typecheck` clean; `changesLogic.test.ts`
26 pass.
