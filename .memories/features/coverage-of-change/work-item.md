# Feature: Coverage of change

Map a code-coverage report onto the current diff and report which **changed
new-side lines** the test run never executed, each carrying its `DiffLine.index`
so the UI can highlight the exact row in the diff pane.

## Scope (this work item = BACKEND only)

Rust (`cb-core` + `src-tauri` bridge) + the TypeScript IPC mirror
(`src/ipc/types.ts`, `src/ipc/api.ts`). The Changes-tab UI is a separate agent's
work — do not touch other `src/` files.

## Acceptance

- Cobertura + LCOV parsers producing a per-file line→hit map (only coverable
  lines present, so absence = abstain).
- A pure mapper (`changecov`) that classifies each changed Addition line as
  covered / uncovered / abstained, matching coverage↔diff by **unique
  whole-segment suffix** (warn + skip on 0 or ≥2 matches — never basename).
- A coverage-enabled test run (`run_tests { withCoverage }`) that collects
  coverage, maps it, and caches the result per workspace/config.
- A non-streaming `coverage_of_change(mode)` command returning the cached map
  (empty + warning when none collected).
- Ordinary (non-coverage) runs are byte-identical to before.

## Governing rule

Abstain rather than guess. A wrong "this line is uncovered" is worse than no
answer. Every threshold refuses.
