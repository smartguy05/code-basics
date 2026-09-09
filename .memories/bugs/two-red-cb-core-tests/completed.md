# Completed — 2026-09-02

## Files

| File | Change |
|---|---|
| `crates/core/src/sql/driver/sqlite_tests.rs` | normalise `\r\n` before the source pin; doc comment explains the `include_str!` trap and why the sibling pins are safe |
| `crates/core/src/git/coverage_tests.rs` | test rewritten and renamed `two_declared_labels_covering_one_file_become_one_card_each`; pins the current one-card-per-intent rule |

**No product code changed.** `sqlite.rs` and `grouping.rs` are untouched.

## Neither test was weakened — both were mutation-tested

A test that starts passing proves nothing, so each was broken deliberately to
confirm it still catches the regression it exists for:

- **sqlite pin:** lifted `connect_with` out of `within_connect_deadline` (in a
  form that still compiles) → test FAILED with its own message. Reverted.
- **coverage:** changed the per-reason loop to `covering.iter().take(1)` so only
  the first reason gets a card → test FAILED with `got ["first reason"]`.
  Reverted.

The first attempt at each mutation was itself wrong and had to be redone — the
sqlite one broke compilation, and the grouping one silently no-opped because the
script assumed CRLF on an LF file. **In both cases the test run then looked
green.** Check that a mutation actually applied and actually compiled before
reading anything into the result.

## Verification

- `cargo test -p cb-core` — **2963 passed, 0 failed** (lib), every integration
  suite green. Was 2961/2 before.
- `cargo fmt --check` clean; `cargo fmt` touched only the two test files.

## Finding not acted on: `IntentGroup.candidates` is dead

The `else` branch in `grouping.rs` that builds an `intent-ambiguous:` key, an
empty label and a populated `candidates` list is **unreachable**. The guard
above it — `if !matches!(kind, Intent | Formatting) && covering.len() > 1 { …
continue; }` — has the *identical* condition and `continue`s first, with nothing
in between that could change `kind` or `covering`.

Confirmed two ways: by reading the conditions, and empirically — a `panic!`
placed in that branch compiled and **never fired across all 2963 tests**.

Consequence: nothing populates `candidates`, so it is always an empty `Vec` for
every consumer. It is a public field on `IntentGroup`, so it crosses IPC and is
mirrored in `src/ipc/types.ts` and rendered in the Intent UI.

Left alone deliberately — removing it touches the `model.rs` key-pinning test,
`types.ts` and the UI together, which is a decision to take deliberately rather
than as a side effect of fixing a test. See `todos.md`.
