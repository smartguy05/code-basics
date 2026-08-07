# Notes

## Baseline coverage (2026-08-07, cargo llvm-cov 0.8.7)

`cargo llvm-cov --workspace --summary-only --ignore-filename-regex "src.tauri.src.main\.rs|process.kill\.rs"` (run from Git Bash — see sh note below):

**TOTAL: 78.30% lines (10734 lines, 2329 missed), 77.94% regions, 66.49% functions — already above the 70% target.**

Low files (everything else in cb-core is 83–100%):
- `intents/providers/claude_code.rs` — 15.58%
- `intents/providers/codex.rs` — 20.93%
- `intents/providers/mod.rs` — 53.12%
- ALL of src-tauri 0% except `state.rs` (87.85%): commands/* (git 258 lines, run 248, inspect 186, intents 162, workspace 124), invocation.rs 137, recorder.rs 35, lib.rs 32.

Consequence: Phase 2 (repo.rs/trx/junit/solution backfill) is NOT needed — repo.rs 91.56%, trx 94.31%, junit 95.49%, solution 94.27%. Skipped.

## process:: tests hang/fail when `sh` is not on PATH (2026-08-07)

**Root cause found (not coverage-specific):** the `crates/core/src/process/mod.rs` tests spawn `sh -c ...`. In a PowerShell session without Git Bash's `sh` on PATH, the spawn fails, so:

- `streams_stdout_and_reports_success`, `separates_stderr_from_stdout`, `reports_nonzero_exit_as_failure`, `env_and_cwd_are_applied` fail fast (no output, `None` exit code), and
- `a_running_process_can_be_found_by_id_with_its_pid`, `cancel_kills_the_whole_process_tree`, `cancel_stops_a_long_running_process` **hang forever** — they spin on `loop { if sup.is_running(id) }` with no timeout, and a failed spawn means `is_running` never becomes true. Observed as a ~2 h wedge during the first `cargo llvm-cov` baseline; initially misdiagnosed as instrumentation-specific.

**Rule: run all `cargo test`/`cargo llvm-cov` invocations from Git Bash (the Bash tool), never PowerShell.** `which sh` → `/usr/bin/sh` there; `Get-Command sh` in PowerShell finds nothing.

**Proper fix (todo, cheap):** bound those three spin loops with `tokio::time::timeout` so a failed spawn fails the test with a message instead of hanging. Do this while touching cb-core in Phase 1/2. Candidate CLAUDE.md gotcha entry once fixed.

## Killing stuck coverage processes

Direct `Stop-Process` was denied by the permission classifier; `TaskStop` on the background task id kills the tree cleanly — use that.

## Watchdog

Session cron `9cbdbdbc` checks the coverage run every 5 min for the hang pattern (user requested). Delete with CronDelete once baseline lands.
