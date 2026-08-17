# Bug — `didChange` measured its range from the new text

Found 2026-08-17 **by running the app**, not by a test. Every test in the
subsystem passed, before and after.

## Symptom

Editing a file in the Run tab breaks find-usages for that file, in one of two
ways depending on which direction the edit went:

| edit | range vs the server's buffer | what the user sees |
|---|---|---|
| same length | exact | correct, **by coincidence** |
| longer (even one character) | overruns the end | usages die for that file; tsserver throws `Cannot read properties of undefined (reading 'charCount')` |
| shorter | stops short | **a confident wrong answer** |

The shrink case is the serious one. Measured live: a 5640-character buffer
replaced by a 39-character document containing exactly one function reported
**ten** symbols — `OnlyMe` plus nine that existed only in the deleted text
(`canRejectInMode`, `importFeedback`, `rejectFeedback`, …) — as
`outcome: "ready"`, no caveat. The server's buffer had silently stopped being
the file, and it kept answering.

That is precisely the failure this subsystem exists to refuse: a wrong answer
wearing the clothes of a right one.

## Root cause

`DidChangeTextDocumentParams::whole_document` built its range with
`document_range(text)` — the **new** text. A range is an instruction about the
buffer the server *currently holds*; the text is what to put there. The two
describe different documents whenever an edit changes the length.

Proven by reverting the fix with the wire-level test in place: the old code sent
ranges `(3,4)` then `(0,1)` where the correct ones are `(2,5)` then `(3,4)` —
i.e. the range was always **one edit ahead** of the server.

## Why every test missed it

Each existing `whole_document` test passed **the same string** as both the range
source and the content. A fixture that conflates the old and new documents cannot
distinguish them, so the whole family was green against a function that ignored
the distinction. Same shape as the `anchor()` builder noted in the Phase 5 review
(`selectionLine: over.line`) — a builder deriving one field from another removes
the very difference the test exists to catch.

## Fix

`whole_document(uri, version, previous_end, text)`. `Client` records the extent
of what it last sent alongside the version, in one `Sent { version, end }` value
so the two cannot drift, and hands `end` back on the next edit. Position only —
eight bytes per document, not the text.

## What was considered and rejected

Sending a **rangeless** (Full-sync) change event, which needs no knowledge of the
old text at all. Probed against real servers rather than assumed:

- **tsserver** advertises `textDocumentSync: 2` (Incremental) and **accepts** it —
  9 symbols → 1, correctly. So the old comment's claim that Incremental forbids a
  rangeless event is false *for that server*.
- **Roslyn hung** on the follow-up request after a rangeless change.

One server accepting it is not licence to overturn a design decision the other
server's behaviour supports, so ranged changes stay. The fix above is correct on
every server either way.

## Coverage added

- `protocol_tests::the_replaced_range_describes_the_document_the_server_holds_not_the_new_one`
  — pure, passes a shorter text than the range, which the old signature could not
  even express.
- `lsp_client::the_replaced_range_is_the_extent_of_what_the_server_was_last_told`
  — reads the ranges **off the wire** via the fake's `notificationJournal`, over a
  grow then a hard shrink, so a range taken from the new text differs every time.
