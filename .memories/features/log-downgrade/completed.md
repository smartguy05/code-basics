# Completed: log-level downgrade erosion detector

## What was done
Added a paired "log-level downgrade" erosion detector: a log statement whose
severity was lowered across a removed+added pair in one hunk (e.g. Error->Debug).

## Files touched
- `crates/core/src/erosion/rules.rs`
  - New `ErosionCategory::LogDowngrade` variant (after `DroppedLog`), doc
    "A log statement whose severity was lowered."
  - Rewrote the ~189 header comment: single-line rules here; downgrade pairing
    lives in `scan::detect_log_downgrades`.
- `crates/core/src/erosion/rules_tests.rs`
  - `categories_serialise_in_camel_case`: added `LogDowngrade` / `"logDowngrade"`
    after `droppedLog`.
- `crates/core/src/erosion/scan.rs`
  - Split the old inline regex loop into `run_regex_pass`.
  - `scan_diffs` now runs the regex pass (when rules apply) AND always runs
    `detect_log_downgrades(file)` per file (independent of the rule set).
  - New: `detect_log_downgrades`, `log_signal`, `log_rank`, `log_ecosystem`
    (LogEcosystem Cs/TsJs/Rs), `message_key`, `first_string_literal`.
  - Ranks: .cs LogError/LogCritical/Log.Error=3, LogWarning/Log.Warn=2,
    LogInformation/LogDebug/LogTrace/Log.Info/Log.Debug=1; .ts/.js .error(/.fatal(
    =3, .warn(=2, .log(/.info(/.debug(/.trace(=1; .rs error!=3 warn!=2
    info!/debug!/trace!=1.
  - Pairing: per hunk, exact message_key match, added rank < removed rank, one
    flag on the ADDED line (line=new_lineno, index, origin=Addition,
    rule_id="log-downgrade"). ABSTAINS when a key has !=1 removed or !=1 added
    candidate, or the added line has no new_lineno.
- `crates/core/src/erosion/scan_tests.rs` — 4 new tests (see below).
- `src/components/erosionLogic.ts` — `logDowngrade` added to CATEGORY_ORDER
  (after droppedLog) + `logDowngrade: "Log level lowered"` in CATEGORY_LABEL.
- `src/ipc/types.ts` — `| "logDowngrade"` added to ErosionCategory union (after
  droppedLog), mirroring the Rust enum order.

## Tests (all pass)
Rust (`cargo test -p cb-core erosion`): 33 passed. New:
- `an_error_to_debug_downgrade_of_the_same_statement_is_flagged_once`
- `a_downgrade_with_a_different_message_does_not_flag`
- `an_ambiguous_downgrade_does_not_flag`
- `an_upgrade_is_not_a_downgrade`
Frontend: `pnpm test` 959 passed (41 files); `pnpm typecheck` clean.

## Notes
- The existing `*-dropped-log` regex rules ALSO fire (DroppedLog) on the removed
  higher-severity line of a real downgrade. Different category/line/side — by
  design, not a double-report bug.
- No IPC struct shape changed (only an enum variant added); the ErosionFlag /
  ErosionReport key-pinning tests were untouched and still pass.
