# Bug — the Stop hook panics on an em dash near the start of a line

Reported 2026-08-17, from the running harness rather than from a test:

```text
Stop hook error: Failed with non-blocking status code:
thread 'main' (28688) panicked at crates\core\src\intents\hook.rs:504:40:
end byte index 6 is not a char boundary; it is inside '—' (bytes 4..7 of string)
```

## Symptom

Any assistant turn whose reply contains a line beginning with **four ASCII bytes
followed by a multi-byte character** crashes `record-intent`. The turn's recorded
intent is lost, and the error names a line that has nothing to do with intents.

The reply that triggered it opened with `Yes — verified three ways:`. Em dashes,
curly quotes and arrows near the start of a line are ordinary prose, so this is
frequent rather than exotic.

## Root cause

`hook::strip_prefix_ignoring_case` guarded with a **byte** length and then took a
**byte** slice:

```rust
(line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(prefix))
    .then(|| &line[prefix.len()..])
```

`"intent"` is six bytes. `"Yes "` is four, and the following `—` occupies bytes
4..7, so byte 6 is *inside* a character and the slice panics instead of simply
not matching.

## Fix

`line.get(..prefix.len())?` — `str::get` returns `None` for a non-boundary,
which is the correct answer for both reasons a caller can fail to match (the line
is shorter than the prefix, or it does not start with it). The prefix is ASCII at
every call site, so `eq_ignore_ascii_case` stays correct.

## Blast radius, and why it earned a named test

This hook is the *only* record of why a change was made, and it runs on every
turn. A panic there is silent data loss dressed as an unrelated error. Pinned by
`hook_tests::a_line_with_a_multibyte_character_where_the_keyword_would_end_does_not_panic`,
which reproduces the reported message byte-for-byte and also sweeps the shape
across every offset the check can land on.

## Audited alongside it

`hook::contains_word` also slices by byte (`haystack[..at]`,
`haystack[at + needle.len()..]`) but is **safe**: both offsets come from
`str::match_indices`, which only yields char boundaries. No other byte slice
exists in `hook.rs`.
