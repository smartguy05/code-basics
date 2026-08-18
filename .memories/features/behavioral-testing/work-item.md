# Behavioral before/after testing

## What
Run the same test suite / scenario against git HEAD and the working tree, then diff the
observable outcomes (test results, console output, HTTP responses) to give evidence a
change did what was claimed — the *runtime* counterpart to the static intent Scorecard
(`git/coverage.rs`). Surfaces in Changes → Intent view, per intent card.

## Decisions (locked with user)
- Outcomes: test results + console/stdout + HTTP responses.
- HEAD side: on-demand isolated `git worktree` rebuild; never disturb the working tree.
- HTTP scenarios: reuse existing `.http` / REST Client files.
- Readiness: HTTP probe with timeout; abstain if never ready.
- UI: Intent view, per-card (changed-files → card); ambiguous → unattributed bucket, never guessed.

## Governing rule
"A wrong label is worse than no label." Every comparison abstains rather than assert a
change from noise / un-ready server / ambiguous attribution.

## Full plan
`C:\Users\AnthonyJames\.claude\plans\an-important-feature-to-glistening-feigenbaum.md`
