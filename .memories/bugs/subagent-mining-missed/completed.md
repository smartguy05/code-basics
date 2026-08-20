# Completed: Subagent transcripts were never mined

## Root cause
`claude_code::find_sessions` read only `*.jsonl` directly in
`~/.claude/projects/<enc>/` via a flat `std::fs::read_dir`. This Claude Code
version stores subagent work in separate files at
`projects/<enc>/<session>/subagents/agent-*.jsonl` (every line
`isSidechain:true`, carrying `cwd`), which the flat read never reached — so a
file edited only by a subagent got no mined geometry.

## Verified before fixing
A real `agent-*.jsonl` top line has `parentUuid: null`, so
`resolve_subagent_root` resolves its lineage; the files carry `cwd`, so
`transcript_cwd` passes. The only gap was discovery.

## Fix
`find_sessions` now walks each project dir with `walkdir::WalkDir` (already used
by `codex.rs`) at `max_depth(3)` — reaching both the flat `<session>.jsonl`
(depth 1) and `<session>/subagents/agent-*.jsonl` (depth 3), filtered by
`transcript_cwd`. No change to `resolve_subagent_root` (null top parent
resolves). File: `crates/core/src/intents/providers/claude_code.rs`.

## Test
`claude_code_tests.rs::a_subagent_transcript_is_discovered_and_its_edit_is_mined`
+ `Fixture::subagent_session` helper: asserts the flat main session is still
mined AND the subagent's Edit is now mined.

## Limitations (documented, not fixed)
- Mined subagent edits carry transcript-derived turn ids, so they bind to a
  live declared label only cross-turn — surfaced by [[intent-labels-not-binding]]
  (single → title, many → candidates).
- `read_tool_use` has no `apply_patch` arm (fine for Claude Edit/Write).

## Verified
Full cb-core lib 2195 pass; claude_code 71 pass.
