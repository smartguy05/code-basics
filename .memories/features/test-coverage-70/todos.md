# Todos

All phases complete (Phase 2 cancelled as unnecessary). Remaining follow-ups / tech debt:

- [x] ~~Commit the work.~~ RESOLVED (reconciled 2026-08-21): the tree is clean (only the gitignored `src-tauri/.code-basics/` artifact is untracked); this work is committed and the branch has moved on.
- [x] ~~RunView.tsx should import preferApplicationProcess from views/inspectLogic directly; then drop the compatibility re-export in InspectView.tsx. Same for TestsView.tsx / stripAnsi via components/consoleLogic vs OutputConsole re-export.~~ **DONE (WF1, 2026-08-24, C3):** re-exports removed, consumers repointed, guarded by `src/reexportGuards.test.ts` (via `import.meta.glob` raw, no `node:fs`).
- [ ] Watch for the one-off flaky cb-core lib test failure seen during a coverage run (details lost; rerun was green).
- [ ] Optional stretch (out of metric): Wire.cs serialization round-trip tests in sidecar/inspector.
- [ ] Behaviour deltas awaiting user sign-off (see completed.md Phase 3): CLI error wording, compound error aggregation.
