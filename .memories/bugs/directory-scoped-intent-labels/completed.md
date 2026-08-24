# Directory-scoped `Intent(...)` labels never attached (2026-08-17)

## Symptom
A declared intent `Intent(ONEflight.Client.OPS135.Components): cancel superseded
table reads` was recorded (label + 4 edit records present in
`.code-basics/intents/`) but never showed as a card; the Changes panel showed
"Capture is on, but nothing here matches a recorded intent…".

## Root cause
`Intents::label_for` (`crates/core/src/intents/mod.rs`) matched a scope entry to
a record only by **exact whole-path equality**. The scope was a real directory
(`ONEflight.Client.OPS135.Components`, a .NET project folder), and the edited
file lived beneath it (`.../Pages/Reports/Trips/FlightsReportPage.razor`), so it
never equalled the scope. A non-empty scope is also disqualified from the
empty-paths turn-wide fallback, so the reason orphaned → no `GroupKind::Intent`
(`git/grouping.rs`) → banner (`src/components/intentPanelLogic.ts` looks for
`kind === "intent"`).

## Fix
Added private `scope_covers(scope, record_path)` beside `normalise_path`: a scope
covers a record when it names the file exactly OR is an ancestor **directory**
(segment-boundary prefix, so `foo` never covers `foobar/x`). `label_for` now uses
it. Decidable/abstain-safe — the scope is author-declared, containment is a fact.
No changes needed in grouping/attribution/TS; the Intent card forms once
`label_for` returns the label.

Files:
- `crates/core/src/intents/mod.rs` — `scope_covers` + `label_for` call.
- `crates/core/src/intents/intents_tests.rs` — 5 tests (covers-beneath, outside,
  sibling-prefix guard, backslash dir, exact still works).
- `crates/core/src/intents/providers/instructions.rs` + root `CLAUDE.md` —
  guidance now says a scope entry may be a file or a directory; prefer specific
  files.

## Gotcha for the guidance test
`instructions_tests.rs::the_requested_form_is_one_the_hook_can_parse` grabs the
FIRST line starting with `Intent(` and asserts it parses to
`["src/api.ts", "src/apiLogic.test.ts"]`. Keep that canonical example first; the
directory example is inline-backticked prose (`\`Intent(src/components): …\``) so
its line starts with a backtick and is skipped by the finder.

## Verified
`cargo test -p cb-core` green (the lone `process::cancel_stops_a_long_running_process`
failure is the known flake — passes on isolated rerun). fmt clean; clippy clean
for touched files; `pnpm`/`node scripts/check-docs.mjs` passes.
Not yet re-verified live in the app (needs the OneFlight workspace open on branch
`users/anthony/19778-dev-slow-trips-report`, since records are branch-filtered).
