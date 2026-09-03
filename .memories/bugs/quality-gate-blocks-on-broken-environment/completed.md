# Completed — 2026-09-02

## Files

| File | Change |
|---|---|
| `crates/core/src/qgate/mod.rs` | `GateVerdict`, `read_gate_output`, `unresolved_packages`, `declared_dependencies`, `UNRUNNABLE_DETAIL_LINES`, `package_of`, `capped`, `unrunnable`; module doc gains "A non-zero exit is not always a verdict" |
| `crates/core/src/qgate/decide_tests.rs` | +15 tests |
| `crates/core/src/qgate/install.rs` | `ABSTAIN_NOTE` and `SHARED_EXE_PATH_NOTE` constants, wired into both providers' caveats |
| `crates/core/src/qgate/install_tests.rs` | +2 tests pinning the caveats |
| `src-tauri/src/qgate_run.rs` | `failing_output` → `run_gate` returning a verdict; three-way routing; `package.json` read once and passed as `declared` |
| `CLAUDE.md` | gate paragraph rewritten; junction-block bullet updated with the `tsc` `paths` workaround |

## The rules, and why each exists

- **Environmental only if declared.** An unresolved package counts as an
  environment problem *only* when `package.json` already lists it. Importing
  something never installed stays an ordinary failure the agent can fix, and so
  does a missing relative path (`./sibling`) — pinned by two tests.
- **Never sift a broken run.** A real `TS2322` sitting among unreachable-module
  errors does *not* get extracted; the run abstains whole and warns that a real
  problem may be hidden. Sifting would mean telling `any`-poisoning apart from
  genuine errors, which is a guess.
- **Cap the noise, never the verdict.** `Unrunnable` output is capped at 20
  lines with an "...and N more lines" tail; `Failed` output is passed through
  **whole**, because the one line that matters could be anywhere in it. Both
  directions are pinned by tests.
- **Toolchain markers.** `untrusted mount point`, `os error 448`,
  `Access is denied. (os error 5)`, `Permission denied (os error 13)` are
  environmental for *any* gate, with no package list needed.

## Verification

- `cargo test -p cb-core --lib qgate::` — **45 passed**, 0 failed (15 + 2 new).
- `cargo fmt --check` clean; `cargo check --workspace --all-targets` clean.
  `cargo fmt` touched **only** the five files above (checked, because a
  workspace-wide format could otherwise have silently reformatted unrelated
  code — and `sql/driver/sqlite_tests.rs` pins `sqlite.rs`'s source text).
- `pnpm docs:check` — passed, 23 files.
- **End-to-end against the real broken environment**, using a debug build:
  - typecheck run → named all 32 unreachable declared packages, printed the
    "checked nothing" warning, capped at 20 lines + `...and 3843 more lines`,
    **exit 0**. Total output 36 lines, down from ~3,860.
  - with a deliberate `cargo fmt` violation added to `crates/core/src/lib.rs`
    (reverted immediately) → **exit 2**, block message, full untruncated diff.
    This is the check that proves the refactor did not break blocking.

## Two pre-existing failures, not caused by this change

`cargo test -p cb-core` shows 2961 passed / 2 failed. Both are independent:

- `sql::driver::sqlite::tests::the_only_call_that_opens_a_handle_is_the_one_under_the_deadline`
  — `include_str!("sqlite.rs")`, and that file is byte-identical to HEAD, so it
  fails on HEAD too. Its own message says to re-pin it after a reformat.
- `git::coverage::tests::two_declared_labels_covering_one_file_surface_as_candidates`
  — `left: "first reason", right: ""`.

Neither module references `qgate`, and neither file is modified. Left alone
rather than "fixed" — weakening a failing test to get green is forbidden, and
re-pinning someone else's test was not the task.
