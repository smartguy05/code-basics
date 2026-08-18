# Todos — stash-management

- [x] Core: StashEntry struct + re-export (repo.rs, mod.rs)
- [x] Core: stash_list / stash_apply / stash_drop / stash_clear; indexed stash_pop
- [x] Core: integration tests in tests/git_operations.rs (6, written first)
- [x] Core: StashEntry key-pinning + parse_stash_branch tests
- [x] Commands: git_stash_list/apply/drop/clear; indexed git_stash_pop (git.rs)
- [x] Register commands in lib.rs
- [x] IPC: StashEntry in types.ts; wrappers in api.ts (indexed pop)
- [x] Frontend: StashPanel.tsx + stashLogic.ts + stashLogic.test.ts
- [x] ChangesView: Stashes segmented option + panel mount; Grouping union
- [x] HistoryView: remove the two old stash buttons
- [x] Docs: commands.md, core-crate.md; INDEX regenerated
- [x] Verify: cargo test (1962), pnpm test (662), typecheck, build, docs:check, clippy/fmt
- [ ] Remaining: live `pnpm tauri dev` smoke test (not yet run)

## Note (unrelated)
Codex Pre/PostToolUse "hook exited with code 1" errors the user saw are the
USB LCD Dashboard emit hook (device 1a86:5722 unplugged, 2s timeout) in
~/.codex/hooks.json — NOT code-basics (no intent hook installed in Codex).
User chose to leave the dashboard config as-is.
