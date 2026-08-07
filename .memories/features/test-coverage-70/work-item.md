# Work item: Raise test coverage to ≥70%

Goal: ≥70% line coverage over the code that makes sense to test.

Scope decisions (user, 2026-08-07):
- Rust: `cargo llvm-cov --workspace` (cb-core + cb-app) ≥70%, exclusions limited to `src-tauri/src/main.rs` and `crates/core/src/process/kill.rs`.
- Frontend: vitest + @vitest/coverage-v8 over extracted pure-logic `*Logic.ts` modules, 70% threshold on those modules (separate metric).
- C# sidecar excluded (ClrMD-bound; Wire.cs round-trips only as optional stretch).
- Thick src-tauri command logic refactors INTO cb-core (dependency rule), not tested in place.
- Execution: workflows with Opus agents, one workflow per phase, review between phases.

Full plan: C:\Users\AnthonyJames\.claude\plans\review-the-current-code-moonlit-book.md
