# Feature: Surface erosion scan in the quality gate (non-blocking)

## Goal
Add the erosion detector to the `quality-gate` Stop hook as a NON-BLOCKING
advisory (stderr, exit 0), sitting beside the existing memory reminder.

## Why non-blocking
The erosion detector ranks nothing and carries no severity — it is advisory.
The blocking gates (typecheck / fmt / AI-REJECTED) are deterministic-correctness
only. Erosion is NOT gated behind CB_GATE_FULL: that opt-in is for slow
relinking checks (clippy); erosion is cheap regex over a diff.

## Scope (files owned)
- crates/core/src/qgate/mod.rs (pure decision layer)
- crates/core/src/qgate/decide_tests.rs (test module, path = "decide_tests.rs")
- src-tauri/src/qgate_run.rs (thin I/O runner)

Do NOT edit any erosion file, git file, or src/ipc/types.ts.

## ACs
- Pure `erosion_reminder(&ErosionReport) -> Option<String>` in qgate/mod.rs,
  unit-tested (empty -> None; flags -> formatted; cap + "and N more").
- Runner opens Repo, diff_all(WorkingToHead), scan_diffs(all_rules), then prints
  the reminder to stderr; abstains (says nothing) on any error.
- Blocking checks still short-circuit before it.
