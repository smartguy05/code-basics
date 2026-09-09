# Two long-standing red tests in `cb-core`

`cargo test -p cb-core` had been reporting **2961 passed / 2 failed** for some
time, and both failures had been carried across sessions as "pre-existing, not
mine". Fixed 2026-09-02.

**Neither was a product defect. Both were defects in the tests**, and each
failed while reporting that the *code* had broken a rule it had not broken —
which is the worst failure mode a test has, because it trains the reader to
ignore it.

## 1. `sqlite::tests::the_only_call_that_opens_a_handle_is_the_one_under_the_deadline`

A source pin: `include_str!("sqlite.rs")` and a `contains` needle spelling the
line break as `\n`.

`include_str!` embeds the file's bytes **verbatim** — it does not normalise line
endings — and this repository's `.gitattributes` covers only
`crates/core/fixtures/**`. With `core.autocrlf=true` (this machine) `sqlite.rs`
is checked out CRLF, so the needle could never match. Verified at byte level:
607 CRLF, 0 bare LF; needle matches with CRLF, not with LF.

The working tree is **mixed** — `grouping.rs` is LF, `sqlite.rs` is CRLF — which
is why exactly one test broke and why a mutation script written for one file
silently no-ops on the other.

## 2. `git::coverage::tests::two_declared_labels_covering_one_file_surface_as_candidates`

The test pinned a **superseded design**. It was written in `d9eb2ab` ("Surface
declared intents on their cards (single title or candidates)"), where two
declared reasons scoping one file produced a *single untitled* card listing both
in `candidates`. `22e7a78` replaced that with one card per declared intent, and
did not update the test.

The replacement is the rule `CLAUDE.md` states: "An Intent card is one declared
intent ... only genuinely ambiguous lines repeat across plausible intent cards."
So the code was right and current; the test was stale.
