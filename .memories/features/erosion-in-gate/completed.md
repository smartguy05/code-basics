# Completed: erosion scan as a non-blocking quality-gate reminder

## What shipped
Erosion signals now print at turn end from the `quality-gate` Stop hook as a
NON-BLOCKING advisory (stderr, exit 0), beside the memory reminder. Not gated
behind CB_GATE_FULL.

## Files changed
- crates/core/src/qgate/mod.rs
  - Added `use crate::erosion::ErosionReport;`
  - Added `const EROSION_FLAG_CAP: usize = 20;`
  - Added pure `pub fn erosion_reminder(report: &ErosionReport) -> Option<String>`:
    None when `flags` empty; else header `"N potential erosion signal(s):"`
    (singular/plural), one `  path:line  Category  message` line per flag
    (category via `{:?}`), capped at 20 with a `  ...and N more` tail.
- crates/core/src/qgate/decide_tests.rs
  - Imports: `crate::erosion::scan::{ErosionFlag, ErosionReport}`,
    `crate::erosion::ErosionCategory`, `crate::git::patch::LineOrigin`.
  - `flag(...)` helper + 4 tests:
    `erosion_reminder_none_when_no_flags`,
    `erosion_reminder_lists_each_flag`,
    `erosion_reminder_singular_header_for_one_flag`,
    `erosion_reminder_caps_and_counts_the_remainder`.
- src-tauri/src/qgate_run.rs
  - Imports `cb_core::erosion` and `cb_core::git::{ComparisonMode, Repo}`.
  - New I/O helper `fn erosion_reminder(root: &Path) -> Option<String>`:
    Repo::open -> diff_all(WorkingToHead) -> all_rules -> scan_diffs ->
    qgate::erosion_reminder; abstains (None) on any error via `.ok()?`.
  - In `gate()`, after the AI-REJECTED block and before the memory reminder:
    `if let Some(msg) = erosion_reminder(&root) { eprintln!("{msg}"); }`.
    Blocking gates (exit 2) still short-circuit first. changed_paths untouched.

## No IPC change
No Rust IPC type was added/changed here, so no model.rs key-pinning test or
types.ts edit was needed. ErosionReport/ErosionFlag already cross IPC via the
erosion module (not owned/edited here).

## Verification (verbatim)
- `cargo check --workspace` -> `Finished dev profile ... target(s)` (clean).
- `cargo test -p cb-core --lib qgate` -> `test result: ok. 28 passed; 0 failed`
  (includes the 4 new erosion_reminder tests).
- `cargo fmt --check` on the three owned files: clean (workspace-wide fmt
  reports other files, but none of qgate/mod.rs, decide_tests.rs, qgate_run.rs).

## IMPORTANT deploy note
The gate ships as `target/release/cb-app.exe`. The installed Stop hook only
picks up this change after a RELEASE rebuild (`cargo build --release`). That
rebuild is OUT OF SCOPE and was NOT run (the running app may hold the exe ->
"Access is denied (os error 5)").

## Environment note
The shared working tree had concurrent agents mid-edit. The erosion module was
transiently non-compiling (scan.rs unbalanced brace; scan_tests.rs referenced a
not-yet-added `LogDowngrade` variant). Waited (~1 min) for it to settle before
verifying. My three files are independent of that churn.
