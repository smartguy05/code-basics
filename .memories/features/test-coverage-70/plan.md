# Plan summary

Canonical plan: C:\Users\AnthonyJames\.claude\plans\review-the-current-code-moonlit-book.md

- Phase 0: tooling + baseline (`cargo llvm-cov --workspace --summary-only`, exclusions `src.tauri.src.main\.rs|process.kill\.rs`)
- Phase 1: cb-core pure gaps — codex.rs (0 tests/494 lines), claude_code.rs, ~20 zero-ref public fns
- Phase 2: threshold-driven backfill — git repo.rs via tests/git_operations.rs helpers, trx/junit fixtures, solution.rs
- Phase 3: refactor src-tauri decision logic into cb-core (no IPC shape changes; NetworkKind moves with serde attrs verbatim + pinning test; single-dispatch-point rule preserved by moving invocation.rs wholesale)
- Phase 4: vitest (node env, no jsdom), extract private helpers to co-located *Logic.ts modules, coverage include-list = those modules, thresholds.lines 70
- Phase 5: docs (CLAUDE.md lines re "no JS suite", development.md:21, frontend.md:83), docs:index, docs:check

Verification: fail-under-lines 70 gate; pnpm test/coverage; typecheck/build/clippy/fmt; docs checks; tauri dev smoke.
Execution: one Workflow per phase, Opus agents, own CARGO_TARGET_DIR per parallel cargo agent, adversarial verify stage re-runs gates.
