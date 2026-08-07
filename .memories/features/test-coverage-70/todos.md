# Todos

All phases complete (Phase 2 cancelled as unnecessary). Remaining follow-ups / tech debt:

- [ ] Commit the work (user has not asked yet). Everything is uncommitted on claude/lightweight-ide-replacement-52cp2n, mixed with pre-existing uncommitted docs edits.
- [ ] RunView.tsx should import preferApplicationProcess from views/inspectLogic directly; then drop the compatibility re-export in InspectView.tsx. Same for TestsView.tsx / stripAnsi via components/consoleLogic vs OutputConsole re-export.
- [ ] Watch for the one-off flaky cb-core lib test failure seen during a coverage run (details lost; rerun was green).
- [ ] Optional stretch (out of metric): Wire.cs serialization round-trip tests in sidecar/inspector.
- [ ] Behaviour deltas awaiting user sign-off (see completed.md Phase 3): CLI error wording, compound error aggregation.
