# Completed — the Stop hook's char-boundary panic

Fixed 2026-08-17. Root cause and the reproduction are in `work-item.md`.

## What changed

| File | Change |
|---|---|
| `crates/core/src/intents/hook.rs` | `strip_prefix_ignoring_case` uses `str::get` instead of a byte slice guarded by a byte length; the doc comment records the crashing input |
| `crates/core/src/intents/hook_tests.rs` | `a_line_with_a_multibyte_character_where_the_keyword_would_end_does_not_panic` — reproduces the reported panic, sweeps every offset, and pins that a *declared* intent with a multi-byte label still parses |

## Two things worth carrying forward

**The test had to target `parse_declared_labels`, not `parse_labels`.** The first
draft asserted `parse_labels(..).is_empty()` and failed on `"abcd— dash"` —
correctly, because `parse_labels` falls back to the first sentence when nothing is
declared. Asserting on the fallback path would have hidden the actual claim.

**The release binary is what the hook runs, so the fix is not live until
`cargo build --release -p cb-app`.** `cargo test` proving the fix says nothing
about the hook on disk; `target/release/cb-app.exe` still carried the panicking
code until it was rebuilt (7 min). Verified by piping the exact crashing payload
into the real binary:

```sh
CODE_BASICS_DEBUG_HOOKS=1 ./target/release/cb-app.exe \
    record-intent --event Stop --provider claude-code < payload.json
```

Exit 0, and `labels.jsonl` gained
`{"label":"probe the char boundary fix","source":"declared"}`. The synthetic probe
line was then removed from `labels.jsonl` so it could not title a real card.

Note `CODE_BASICS_DEBUG_HOOKS=1` — without it the recorder swallows every failure
by design, so a broken hook is invisible. Set it when diagnosing one. Also: the
payload has to go through a **file**, since a here-string with Windows
backslashes in `cwd` is rejected as *"invalid escape at line 1 column 12"* before
anything interesting happens.

## Gate

`cargo test -p cb-core` **1931** lib + 49 / 2 (1 ignored) / 33 / 1 (5 ignored) /
17 / 26 / 11 — all green. Clippy at exactly the two documented baseline warnings
(`importers/rider.rs:65`, `workspace.rs:1729`). `cargo fmt --check` clean.
