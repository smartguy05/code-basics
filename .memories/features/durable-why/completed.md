# Durable why — completed

A git note maps each committed line to the agent's turn, label, and the user's
prompt; surfaced in a History "Why these lines exist" panel and a hover tooltip.

## Core (crates/core/src/git/why.rs + repo.rs)
- `WHY_NOTES_REF = refs/notes/code-basics-intents`; types `DurableWhy`/`FileWhy`/
  `WhyEntry`/`LineIntent`. `build_for_commit(diffs, attributions, prompts)` keys each
  **added** anchor line by `content_hash` (FNV-1a over the skeleton form via
  `attribution::anchor_key`); ambiguous key across turns is dropped. `resolve_lines`
  maps blob lines back. `record_note(repo, root, commit)` is the ONE pipeline both
  commit paths funnel through (loads intents + `load_prompts`, attributes, builds, writes).
- `Repo::{write_why_note, read_why_note, why_for_file}` (git2 notes, force=idempotent).
- `WhyEntry.prompt` / `LineIntent.prompt` (serde default+skip, back-compat).

## Commit paths
- In-app: `git_commit` calls `record_why` → `record_note` (best-effort, swallows errors).
- CLI/agent: `post-commit` hook (`intents/whyhook.rs`, marker-bounded/previewed, installed
  on enable) → recorder `HookEvent::PostCommit` → `record_note` for HEAD.

## Prompt capture (session mining — Claude only)
- Store: `intents/mod.rs` `prompts.jsonl`, `IntentPrompt {provider,turnId,prompt}`,
  `append_prompt`/`load_prompts`/`prompt_for(prompts, turn)` (free fn — NOT on `Intents`,
  to avoid churning ~30 struct literals). In `clear()`.
- Miner: `providers::history` returns `HistoryMined {records,labels,prompts}`.
  `claude_code.rs::read_transcript` mines `type=="user"` lines via `user_prompt_text`
  (rejects tool_result echoes), keyed to the SAME `claude-history-{session}-{block}` turn
  as that block's edits — the correctness invariant, proven by
  `the_user_prompt_is_mined_and_keyed_to_the_edits_turn`. Codex returns empty prompts
  (rollout user-turn shape uncertain — documented follow-up).
- `import_intent_history` appends mined prompts.

## Frontend
- `historyLogic.ts`: `intentForLine` (line→intent), `whyTooltip(intent)` (label + source
  + prompt, null when no intent) — tested.
- `DiffView.tsx`: `lineWhy?: (line)=>string|null` prop → CodeMirror `hoverTooltip` on the
  working-side extensions, read via handlers ref (no rebuild). `.cm-why-tooltip` in styles.css.
- `HistoryView.tsx`: `lineWhy={(l)=>whyTooltip(intentForLine(why,l))}`; panel also shows the prompt.

## Deferred (documented in the guide)
- Codex prompt mining. Live `UserPromptSubmit` hook (keys by session, not per-turn).
  Rebase/amend note survival needs `notes.rewriteRef`; notes need a refspec to push.

## Verified
`cargo test -p cb-core` (why 9, durable_why 4, intents 47, claude_code 65 incl. new joins)
green; `cargo check -p cb-app` clean; `pnpm typecheck` clean; `pnpm test` 716; docs clean.
