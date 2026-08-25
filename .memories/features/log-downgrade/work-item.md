# Feature: log-level downgrade erosion detector

## Goal
Add a "log-level downgrade" erosion detector: a log statement whose severity was
lowered across a removed+added pair (e.g. Error -> Debug on the same statement).

## Acceptance criteria
- New `ErosionCategory::LogDowngrade` variant (serialises as `"logDowngrade"`).
- A paired detector `scan::detect_log_downgrades` runs per file, independent of
  the regex rule set (built-in, no TOML).
- Pairs a removed log line with an added log line ONLY when the message key
  matches EXACTLY and the added severity rank < removed rank. One flag per pair,
  on the ADDED (lower-severity) line.
- ABSTAIN on ambiguity (a key with more than one removed or more than one added
  candidate does not flag).
- An upgrade (debug -> error) never flags.
- Ranks per ecosystem: .cs / .ts,.tsx,.js,.jsx / .rs (error/critical=3, warn=2,
  info/debug/trace=1).
- Frontend: `logDowngrade` in CATEGORY_ORDER + a label; types.ts union updated.

## Constraints
- cb-core holds all decisions, no Tauri dep. Tests first.
- The existing `*-dropped-log` rules ALSO fire on the removed higher-severity
  line (DroppedLog category). Different category/line/side — acceptable, not a bug.
- Files owned: erosion/{rules,rules_tests,scan,scan_tests}.rs,
  components/erosionLogic.ts, ipc/types.ts (ErosionCategory union only).
