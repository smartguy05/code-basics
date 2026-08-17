# Completed — `didChange` range measured from the wrong document

Fixed 2026-08-17. Cause, evidence and the rejected alternative are in
`work-item.md`.

## Files changed

| File | Change |
|---|---|
| `crates/core/src/lsp/protocol.rs` | `whole_document` takes `previous_end: Position`; `document_end` made public for the client to record |
| `crates/core/src/lsp/client.rs` | `documents` map holds `Sent { version, end }` instead of a bare version; `did_change` uses the stored extent and then replaces it |
| `crates/core/src/lsp/protocol_tests.rs` | the new pure test, plus a note on the fixture coincidence that hid this |
| `crates/core/tests/lsp_client.rs` | the wire-level test over the fake's notification journal |

## Verified in the running app, against real tsserver

Same file, same sequence that produced the bad answer:

| step | before the fix | after |
|---|---|---|
| open (9 symbols) | ready, 9 | ready, 9 |
| replace with 1-symbol document | **ready, 10 names — 9 stale** | **ready, `OnlyMe`** |
| grow to 2 symbols | **failed**, tsserver exception | **ready, `AndMe`, `OnlyMe`** |

## Gate

`cargo test -p cb-core` **1932** lib + 49 / 2 (1 ignored) / **34** / 1 (5 ignored)
/ 17 / 26 / 11. `cargo test -p cb-app` 41. All **five real-server oracles** pass
(99.9 s). Clippy at exactly the two documented baseline warnings.
`cargo fmt --check` clean.

## Two things worth carrying

**The notification journal earned its keep immediately.** It was added earlier the
same day to catch the document-version bug; it is what made this range bug
provable on the wire rather than by argument, in a layer where nothing answers.
Reach for it for any future `didOpen`/`didChange`/`didClose` question.

**A test fixture that passes one value for two roles proves nothing about the
distinction.** Every `whole_document` test passed the same string as both the old
and the new document. Third instance of this shape in this work item — after the
`anchor()` builder deriving `selectionLine` from `line`, and `truncated` never
being pinned `false`. When a function takes two things that are usually equal,
the test must make them **unequal**.
