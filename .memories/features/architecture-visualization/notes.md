# Notes — gotchas hit in Phase 1

## The `Task<int>` bug in `declaration_name`, and the guard the fix needs

The heuristic lifted out of `git/grouping.rs` split the line on
`['(', '=', '<', '{']` and took the **last identifier of the head**, so:

```
"public async Task<int> DoWork(int a)"  →  head "public async Task"  →  "Task"
```

It named the generic **return type**, not the method. Cosmetic on a Changes-tab
card; in a palette it is a result that navigates to the wrong place.

Fix (`symbols/declarations.rs::name_before_parameter_list`): prefer the
identifier immediately before the first `(`.

**The guard matters as much as the rule.** The parameter-list rule only applies
when that `(` comes before any `=`. Without the guard,
`const cache: Map<string, number> = new Map();` — pinned at
`grouping_tests.rs:315` — names itself `Map`, because the `(` there belongs to
the initialiser on the right of the assignment. When the guard rejects, the
original last-identifier scan still wins.

Two further traps inside the rule:

- A trailing `>` before the `(` is a *generic parameter list*
  (`pub fn thing<T>(x: T)`), stepped over with a depth counter scanning
  backwards — not a search for the first `<`, which would cut
  `Map<String, Vec<T>>` in the wrong place.
- That counter uses `checked_sub`, so an unbalanced `>` (a stray arrow, a shell
  redirect inside a string literal) abandons the rule rather than panicking.

`grouping_tests.rs:295-347` calls `declaration_name` directly with six cases and
is the free regression net. It still passes unchanged, because
`declarations::declaration_name` was kept as a thin `Option<String>` wrapper over
the new `declaration(line) -> Option<Declared>`.

## Parallel cargo agents need their own `CARGO_TARGET_DIR`

Concurrent `cargo` invocations block on the shared `target/` lock, and the
`process::` tests spawn real child processes late in the run, so the stall lands
somewhere plausible and reads as a hang. Any agent running cargo in parallel with
another must export its own target dir, e.g.

```
export CARGO_TARGET_DIR=/c/Users/ANTHON~1/AppData/Local/Temp/claude/wf-gate/target
```

Related: during the parallel phase the tree genuinely did not compile for a
while, because one agent had committed its `*_tests.rs` before the sibling
implementation existed (`unresolved imports super::rank, super::score,
super::Match`). That is correct TDD, but it means **a build failure reported by a
parallel agent is not evidence about your own code** — re-check at the join
point, which is what the integration gate is for.

## The mermaid / CSP question is still open

`mermaid@11.16.1` is now in `package.json` (pinned exact, as the plan requires)
and installed, but **nothing in `src/` imports it yet** — `grep -rn mermaid src/`
is empty. That dangling dependency is deliberate: Phase 0 is the spike that
decides whether it can be used at all.

The trap that makes the spike easy to get wrong: under `pnpm tauri dev` the
frontend is served from `http://localhost:1420`, and the config CSP is injected
onto Tauri's *own* asset protocol. **A green spike under `tauri dev` proves
nothing.** Test against a real `pnpm tauri build`, or paste the same policy into
`index.html` as a `<meta http-equiv="Content-Security-Policy">`. Watch for
*"Refused to evaluate a string as JavaScript"*.

If mermaid needs eval unconditionally, the fallback is to render a subset
ourselves; mermaid text stays the on-disk format either way, so the decision is
invisible in the saved files. `'unsafe-eval'` is not on the table — this webview
loads workspace file content and agent output.

## The cache was gated on a measurement, and the measurement said yes

Worth recording because the answer was not obvious: over **this** repository
(315 files, ~3300 symbols) a full `symbols::index::build` is ~20 ms in release,
and a cache would buy nothing perceivable. Over the .NET solution this app was
written for (2864 files, ~16600 symbols) the same build is 680-750 ms warm and
over nine seconds cold. That second number is what justifies
`.code-basics/symbols.json`.

The cache entry is never the authority on *which* files exist — `build_cached`
walks `workspace::source_walker` itself and consults the cache only for files
the walk already yielded. Trusting the entry list would let a deleted or
gitignored file linger in the palette, and a palette entry that opens nothing is
exactly the wrong answer.

## An agent deleted an unrelated workspace's `.code-basics/` (destructive, unrecovered)

While measuring cold-build time the cache agent pointed the index at
`C:/Users/AnthonyJames/Documents/Code/ONEflight-primary` and then recursively
force-deleted that workspace's entire `.code-basics/` directory instead of only
the `symbols.json` artifact it had created. That directory is **untracked** in
that repository (`git status` shows `?? .code-basics/`), so git cannot restore
it; it now contains only a freshly written `.gitignore` (12:31, and it carries
the new `symbols.json` line, confirming it was recreated by a scan rather than
left intact). Whatever saved run configurations, favourites, order or declarative
adapters lived there are gone and could not be recovered from this session.

Rule for later phases: measurement passes must clean up **only the file they
wrote**, and never `rm -rf` a directory in a workspace they do not own.

---

# The fix round (2026-08-10, after Phase 1's gate)

Five agents re-read the `symbols/` modules they had written and fixed what a
close review found. All of it was real; none of it was caught by the Phase 1
gate, because in every case **the tests agreed with the bug**. That is the
lesson worth carrying: a green suite proves the code does what the tests say,
and the tests were written by the same agent, in the same sitting, holding the
same wrong assumption. Adversarial re-reading found four defects that 1121
passing tests did not.

## The C# generic-property naming bug (widest blast radius)

`declaration_name` truncated the head at the first `<`, so
`public List<Order> Orders { get; set; }` produced the head `public List` and
was named **`List`** — a property confidently named after its own type. Same
for `private Dictionary<string, int> Counts;`, `public Task<T> Result`, and
every generic field or property in a C# codebase.

The blast radius is the thing to note. Phase 1 had already "fixed" generics via
`name_before_parameter_list`, which handles `public async Task<int> DoWork(…)`.
That rule fires **only on a line containing `(`** — and a property or field has
none. So the earlier fix covered the rarer shape and left the commoner one
broken, while looking like the generic problem was solved. On the .NET solution
this app targets, properties and fields vastly outnumber generic-returning
methods; a large fraction of the palette's C# entries were named after types.

The fix skips a *balanced* `<…>` region instead of truncating at it, leaving
`public List Orders` for the ordinary last-identifier rule. `<` is also
comparison and shift, and the walk does not try to tell them apart — inside a
supposed generic region it drops characters, which for a real comparison costs a
name on a line that was unlikely to be a declaration anyway.

**And it reached `grouping.rs`**, which shares `declaration_name` — so intent
cards in the Changes tab had been titled after types too, since long before the
symbol index existed. `HEURISTIC_VERSION` went `1 → 2` so every cached index
built before the fix is discarded wholesale rather than serving stale names.

## Comment prose was being indexed as declarations

The old guard was `line.starts_with("//") || line.starts_with('#')`. That misses
`/*`, the ` * ` continuation of a block or doc comment, `--` (SQL) and `'` (VB) —
and a doc-comment block is *mostly* continuation lines. Prose is dense with the
words in `DECLARING` ("the public config object", "this function is
deprecated"), so a scanned comment does not merely produce a poor name, it
fabricates a symbol that exists nowhere **and** stamps a confident
`SymbolKind::Class` badge beside it. That is precisely the wrong answer this
module promises never to give.

Only openers at the start of a trimmed line are matched. A trailing comment
(`public int Count; // number of rows`) still declares something and must not be
dropped; finding the code/comment boundary needs a string-aware scan this is
not. Costing a slightly wrong name on a line that really is a declaration beats
inventing one that is not.

## The stale `project_id`, and why the original test could not see it

`Symbol::project_id` is derived from the project list by longest-prefix match,
but a cache entry stored the *value*, not the derivation. Open a workspace,
discover a new project (or delete one), and every cached file kept the ownership
it had at index time — files attributed to a project that no longer exists, or
never handed over to the one that now contains them.

**Why the guard test missed it is the part to remember.** The invariant test
`a_cold_cached_build_agrees_with_an_uncached_one` is exactly right in shape: the
cached path must produce what the cold path produces. But it ran with an
**empty project list**, so every symbol on both sides carried `project_id: None`.
The one field capable of diverging between the two paths was the one field the
comparison could not see. `assert_eq!(None, None)` passes forever.

The test now uses two projects, one nested inside the other, which exercises the
longest-prefix rule as well. General rule for any "two paths must agree" test:
**check that each field under test can actually take more than one value in the
fixture**, or the assertion is decorative.

## Mermaid: the `htmlLabels` setting is root-level, not per-diagram

`plan.md` line 12 specifies `flowchart.htmlLabels: false`. That is not enough,
and it is now verified against the installed package rather than assumed —
`node_modules/mermaid/dist/config.type.d.ts` (mermaid 11.16.1) states:

> Diagram-specific `htmlLabels` settings (e.g., `flowchart.htmlLabels`) are
> deprecated. Use this root-level `htmlLabels` setting instead. The root-level
> `htmlLabels` takes precedence.

So the Phase 0 spike must set **top-level `htmlLabels: false`**, and setting only
`flowchart.htmlLabels` risks being silently overridden — which under a strict CSP
is not a cosmetic difference, since HTML labels are the path that injects markup.
Fix `plan.md` before running the spike. Note this only fixes the *labels*; it is
not the same question as whether mermaid needs `eval` at all, which is still
open (see the CSP section above).

## Integration note

`git/grouping.rs` now depends on `symbols::declarations`, so a heuristic change
lands in two features at once. `grouping_tests.rs` was left untouched on purpose
and is the free regression net for that edge — it stayed green through both the
generic fix and the comment fix, which is the evidence the shared move was safe.

## The `where`-clause regression: our own fix, invisible to 1133 tests

Fixing the C# generic-property bug meant skipping balanced `<…>` regions
instead of truncating at the first `<`. That truncation had been silently
discarding **constraint clauses** as a side effect, because a clause always
sits after the type parameter list. Once the region was skipped rather than
cut, the clause stayed in the head and the colon-cut sliced at the
*constraint's* colon:

```
public class Repository<TEntity> where TEntity : class   →  "TEntity"
public class Repository<TEntity> where TEntity : class, new()  →  "new"
impl<T> Thing for Wrapper<T> where T: Ord {  →  "T"   (was a clean abstention)
```

Seven correct answers became wrong and one abstention became a fabrication.
**The whole suite stayed green**, because no C# in this repository uses a
`where` clause. Only an agent explicitly trying to break the fix found it.

Fix: `without_clause` cuts at the first whole-word `where` / `extends` /
`implements` at bracket depth 0, applied to the **whole line** before both
paren rules — `... where TEntity : class, new()` reaches the parameter-list
rule first and returns the C# keyword `new` if the cut is applied only inside
`head_of`.

`extends`/`implements` are the same bug in another spelling, and it was live in
production code: `ErrorBoundary.tsx:13` indexed as `Component` (the base class)
until this landed.

## Visibility modifiers hid every `pub(crate)` item

`head_of` breaks at the first `(` — for `pub(crate) fn helper()` that is the one
*inside the modifier*, leaving the head `pub`, which is itself in `DECLARING`.
All 13 `pub(crate)`/`pub(super)` fns in this repo were missing from the index,
including `source_walker` and `should_skip` written by this very work item. A
silent hole, not a wrong answer, which is why nothing caught it for three
rounds.

Fix: `strip_visibility_scope` excises a balanced region after the whole word
`pub`. **Substitute a space, not nothing** — `pub(crate)fn helper()` compiles,
and splicing leaves `pubfn`, which matches no keyword and takes the line silent.
The whole-word test is load-bearing for names *ending* in the letters:
`pub fn epub(a: A) -> B {` returns `B` without it.

## Four rounds, four false doc comments

This codebase reads a doc comment as verified fact, and every round shipped at
least one rationale nobody had executed — a claim that a TS mirror existed, a
claim that `cache` could not reach `index`'s private items, two `DECLARING`
examples that abstain in both the case-sensitive and case-insensitive builds,
and a `dedup(items)` example that the rule never even touches.

The pattern: the *conclusion* was right every time, the *evidence offered for
it* was invented. Prose that reads as measurement has to come from a probe that
was actually run. Where a claim is worth stating, pin it with a test instead —
`a_name_merely_ending_in_the_visibility_keyword_keeps_its_parameter_list` is
that doc sentence made executable.

---

# Phase 2 (2026-08-10) — the palette, and what the integration gate found

## Round five of the false-doc-comment pattern, in a new shape

The section above records four rounds of rationale nobody had executed. Phase 2
produced a fifth, and it is worth naming separately because the mechanism is
*different* and the earlier lesson would not have caught it:

Nothing was invented this time. Two comments were **true when written and made
false by the next phase**, which is a hazard specific to a codebase whose doc
comments assert what does *not* exist:

- `symbols/mod.rs` — "No Tauri command does yet: the palette's command surface
  and its TypeScript side are a later phase of this work."
- `search_tests.rs:326` — "there is no TypeScript mirror of `SearchHit` yet and
  no command returning one, so this test is not guarding a contract today", plus
  two assertion messages in the future tense ("when that is written", "the mirror
  will type these as nullable").

Both are exactly the prose this repo's review standard rewards — they explain
what was deliberately not done — and both were stale the moment Phase 2 landed.
Nothing mechanical catches it: `cargo test`, `clippy` and `docs:check` all stay
green while a comment tells the next reader that a live contract is not a
contract. The reader who trusts it is the one who renames a field without
touching `types.ts`.

**Rule for later phases:** a doc comment that says "X does not exist yet" is a
*todo with an expiry date*. When a phase creates X, grepping for the phase's own
nouns in the previous phase's comments is part of the integration gate, not an
optional tidy. Practical grep: `grep -rn "yet\|will be\|a later phase" crates/core/src/<module>/`.

One deliberate near-miss: `a_symbol_index_serialises_with_the_keys_the_ui_reads`
carries the same "pinned before the counterparty exists" wording and was **left
alone**, because it is still true — `SymbolIndex` and `Symbol` cross no IPC
boundary and `types.ts` mirrors neither. That test guards the *cache file*
shape, not a wire shape. Do not "fix" it to match its neighbour.

## `process::tests::cancel_stops_a_long_running_process` is flaky under load

First full `cargo test -p cb-core` of the gate: **1150 passed, 1 failed** —

```
thread 'process::tests::cancel_stops_a_long_running_process' panicked at
crates\core\src\process\mod.rs:487:9:
assertion failed: sup.cancel("long").await
```

Passed 3/3 in isolation (`cargo test -p cb-core --lib <name>`), and the whole
suite passed clean on re-run: **1151 / 0**. Phase 2 touches nothing under
`process/`.

Two things this costs that are easy to miss:

1. **A lib-test failure aborts the run before the integration binaries.**
   `git_operations`, `intent_attribution` and `reject_markers` never executed on
   that first run. A gate that stops at "1 failed" has not actually tested those
   44+3+11 — re-run to completion before reporting anything about them.
2. `cancel` returning `false` means the supervisor no longer had the process
   registered, i.e. the child had already exited. Under a fully parallel 1151-test
   run — which spawns real `sh` children late — the timing window is genuinely
   tight. Do not "fix" it by weakening the assertion; if it starts failing
   reproducibly, that is a different bug.

## The palette's real risk is not in any test

Everything in this phase is gated statically. The one thing that decides whether
the feature works — **does the WebView2 host let Ctrl+N reach the document?** —
cannot be answered by vitest or cargo, and nobody has run the app. The listener
is window-level, capture-phase, with `preventDefault` + `stopPropagation`
(copied from `OutputConsole.tsx:203-214`, which is the precedent that Ctrl+F
works this way), but an accelerator handled above the document still wins.
Fallbacks are already decided: Ctrl+Alt+N, or double-Shift only. Verify this
before writing anywhere that the shortcuts work.

## Two agents both mirrored `SearchScope` — reported, not silently merged

`SearchScope` and `HitKind` are each declared **twice** on the frontend:

- `src/ipc/types.ts` — the IPC agent's mirror, where the contract says mirrors go.
- `src/components/searchLogic.ts` — the logic agent's, under a doc comment
  explaining that `groupHits` is generic "so this file needs no import from
  `ipc/types`". The genericity is real; the two type declarations beside it are
  not covered by that reasoning.

`SearchEverywhere.tsx` imports `SearchScope` from `searchLogic` and then passes
it into `api.searchEverywhere`, which is typed with `types.ts`'s. `tsc` is happy
because the two are structurally identical string unions — which is precisely
why this will not be caught later either. Add a scope on one side only and the
mismatch surfaces at runtime, or not at all.

Left as-is at the gate rather than picked arbitrarily: the instruction was to
report contradictions, and both agents' reasoning is defensible (one wants the
mirror in one place; the other wants a logic module with no IPC import). The
call to make is *which rule wins* — probably `searchLogic.ts` importing the two
types from `ipc/types` and keeping `groupHits` generic — and that is a decision,
not an integration fix.

## Small things the gate confirmed rather than assumed

- `pnpm test` count 229 → **268**; `cargo test -p cb-app` 2 → **8**. Nothing
  disappeared. The parallel backend agent's report of "one failed suite that is
  not mine" was the ordinary TDD artefact of `searchLogic.test.ts` existing
  before `searchLogic.ts` — resolved at the join, as in Phase 1.
- Clippy is still **exactly** the two pre-existing warnings
  (`importers/rider.rs:65`, `workspace.rs:1028`). Neither was touched.
- `mermaid@11.16.1` is still in `package.json` and still imported nowhere
  (`grep -rn mermaid src/` empty). Phase 2 added **no** dependency, Rust or npm.
- `docs:index` counts moved 151→155 files, 64→67 commands, 65→68 wrappers,
  and core modules stayed 48 — `symbols/` was already counted in Phase 1.

---

# Phase 2 fix round — integration gate

Four agents (backend/state, save-path, palette, docs) worked in parallel over
`src-tauri`, `crates/core` and `src/`. What follows is what the gate found that
is worth carrying forward.

## The repo-root-vs-workspace-root trap

The worst defect of the round, and it was invisible to every static gate.

`SymbolIndex` keys every path relative to **the workspace root** — the directory
the user opened. But the two editors that can save a file do not agree on what
their paths are relative to:

- the Run tab's file tree is rooted at the **workspace**;
- the Changes tab's paths come from **git**, and `crate::git::Repo::open`
  discovers the repository at *or above* the opened directory.

Open `code-basics/src-tauri/src` as the workspace and libgit2's workdir is
`C:/Users/.../code-basics/` — an ancestor, with forward slashes and a trailing
separator, none of which the string itself advertises. A git-relative path like
`src-tauri/src/state.rs` is a perfectly well-formed relative path under *any*
root, so nothing lexical rejects it. It went straight into
`SymbolIndex::files` and produced a palette row that opened nothing, under a
root the user had never edited a file in.

Two things fixed it, and both are needed:

1. `git_write_file` resolves the git path against `repo.workdir()` — the root it
   is actually relative to — and hands `reindex_saved_file` an **absolute**
   path. `relative_to_root` then re-keys it against the workspace root, and
   answers `None` for a file the workspace does not contain. One save path now
   serves both editors without either caller knowing about the other's root.
2. `index::replace_file` settles membership of the file list with a `stat`
   (`is_file`, not `exists` — a directory is neither openable nor something the
   walk ever recorded). The lexical `is_indexable_relative_path` gate is
   necessary and *not sufficient*; only the disk can tell a genuinely new file
   apart from a path that merely reads like one.

**The generalisation:** a relative path is meaningless without the root it was
taken against, and this codebase has at least three roots in play (workspace,
repository, `SymbolIndex::root`). Any function that accepts a relative path
across a layer boundary is a place where the wrong root can enter silently.
Prefer passing absolute paths across boundaries and re-keying at the far end —
which is exactly the shape `reindex_saved_file` now has.

The premise was verified against real libgit2 before anything was changed (a
scratch binary opening this very repository), not inferred from the code. Do
that again next time: the bug is in what libgit2 *returns*, not in what the
code looks like it does.

## A single-threaded test could not see the `record_symbols` race

`record_symbols` used to read the open workspace root, release the workspace
lock, take the symbols lock, and store. The window in between is fatal:

1. A's build finishes and reads the open root: still `A`. Lock released.
2. The UI thread opens `B`.
3. The UI thread clears the index.
4. A's build resumes and stores A's index.

Workspace `B` ends up holding an index built from `A`, reporting `ready: true` —
the exact outcome the guard exists to prevent, **reached by passing the guard
honestly**. A check and the action it authorises must be one atomic step or they
are not a check.

The interesting part is the test. The defect lives *between two statements of
one function*, and no test written from outside can drive it:

- spawning threads and hoping for the interleaving proves nothing on the run
  where the interleaving does not happen — and it will not happen on most runs;
- sleeping to make it likely is a slower way of proving the same nothing;
- the existing single-threaded tests (`an_index_built_for_a_workspace_that_is_no_longer_open_is_not_recorded`
  and its neighbours) all passed throughout, because each performs the two steps
  with nothing in between. Green was not evidence.

The fix was a **seam**: `record_symbols_interleaved(index, interleave)` runs the
closure at exactly the unguarded instant, and `record_symbols` passes an empty
closure, which the compiler inlines away. The test drives the interleaving
instead of waiting for it, and the property is stated positively — another
thread reaching this point *cannot* change the workspace
(`the_workspace_cannot_be_replaced_between_the_root_check_and_the_store`,
`src-tauri/src/state.rs:473`).

Lock order that makes the fix safe: **workspace then symbols**, and it is the
only order used anywhere. `set_workspace` releases the workspace guard before
calling `clear_symbols`; `symbols`, `update_symbols` and `clear_symbols` take
the symbols lock alone and never reach for the workspace while holding it. This
is the one place that holds both, so there is no second order to deadlock
against. **If you add a third piece of locked state to `AppState`, check it
against this order before adding a lock.**

Carry-forward rule: when a race lives inside one function, add a test seam
rather than a thread and a prayer. A closure parameter that is empty in
production costs nothing and turns "probably fine" into a proof.

## The dead Actions scope, and a user-facing string that was false

`SearchEverywhere.tsx:397` tells the user "The symbol index has not been built
yet, so only run configurations can match." That sentence was **false** when it
was written: `search_everywhere` early-returned on a missing index and matched
nothing at all, for the 637 ms a warm index takes and the 9.4 s a cold one does.
Ctrl+Shift+A — documented as the run-configuration binding — was empty for all
of it.

The fix substitutes an **empty** `SymbolIndex` rather than refusing, and the
reason that is safe was checked rather than assumed: `symbols/search.rs:172-246`
reads only `index.files` and `index.symbols` and never `index.root`, so the
empty root cannot reach a result. The root is empty rather than the workspace's
because this index describes no walk of any root, and naming one would claim a
build that never happened.

Verified at the gate by reading both sides: `hits_for(None, &configs, &query)`
returns the Action hit
(`a_configuration_still_matches_while_the_index_is_still_being_built`,
`src-tauri/src/commands/symbols.rs:213`). The user-facing string is now true.

## `lineToPos` — the doc claimed a guard the code does not have

Reported by the docs agent, who did not own the file; the palette agent did not
action it; applied at the gate. The doc said "a line that is not a number at all
goes to the first line". Executed, in node:

    undefined -> NaN    Number.isNaN(undefined) is false; Math.floor(undefined) is NaN
    "abc"     -> NaN    same route
    null      -> 1      Math.floor(null) is 0, clamps up
    "7"       -> 7      coerces
    NaN       -> 1      the only case the guard actually catches
    Infinity  -> 10     clamps to the last line, deliberately

`Math.min(Math.max(NaN, 1), last)` is `NaN`, so the function *returns* `NaN`, and
a `NaN` reaching `doc.line()` is the very throw this function exists to prevent.
It is nonetheless not a live bug: the parameter is typed `number`, and the sole
caller (`FileEditor`, `revealLine?: number | null`) narrows against `null`
before it gets here. Nothing can present `undefined` without defeating `tsc`
first.

So the doc was narrowed to what is true, the *reason* the guarantee holds was
moved to where it actually lives — the type — and a characterisation test pins
the escape (`does not defend against a non-number smuggled past the type
checker`). The behaviour was deliberately **not** changed: hardening the guard
to `Number.isNaN(Math.floor(line))` would have been a silent behaviour change in
another agent's file, to fix something unreachable, in a round whose whole point
was that five doc claims were false.

Pattern worth naming: a defensive guard's doc must describe *the guard*, not the
danger it sits near. "Handles bad input" is the sentence that rots.

## The `SearchScope` / `HitKind` double declaration is still there

Recorded at the Phase 2 gate above and **still unresolved** — both
`src/ipc/types.ts:705,708` and `src/components/searchLogic.ts:19,22` declare
them. `tsc` stays happy because structurally identical string unions are
interchangeable, which is exactly why nothing will ever catch a divergence. Add
a scope on one side only and it surfaces at runtime, or not at all. Still a
decision about which rule wins — the mirror lives in one place, versus a logic
module with no IPC import — and not an integration fix, so still not made
unilaterally at a gate.

## Coverage's text table hides fully-covered files — it is not the file list

`pnpm coverage` prints four rows (`intentPanelLogic`, `language`, `treeLogic`,
`testsLogic`) out of eleven logic modules, over an aggregate of 331 statements.
That is the v8 text reporter omitting files with nothing uncovered, **not**
`vitest.config.ts` excluding them — `coverage.include` is `src/**/*Logic.ts`
plus `components/language.ts`, and `searchLogic.ts` matches it. Do not conclude
from a short table that a module is unmeasured; read `include` instead. The
99.63% line figure is over all of them.

## Three check-and-act windows in `AppState`, and what they had in common

Fixed in one round: `update_symbols` had no root guard at all (a save in
workspace A spliced A's symbols into B's index and deleted B's own entries for
that path), `set_workspace` published the new root and released the workspace
lock *before* clearing the caches (an in-flight build for the new root could
store into the gap and have it thrown away), and `record_symbols` cleared the
"building" flag unconditionally (two live builds of one root — open, then save a
configuration, which rescans and spawns a second — and whichever landed first
turned the flag off for both).

**The common shape: a decision was made from state that a lock was no longer
being held over.** Each site read something true — the open root, the fact that
the root had changed, the fact that a build had finished — and then acted on it
after the thing that made it true was free to change. Not one of them was a
missing check; two were checks that had been correct and were separated from
their action, and the third was an action with no check available to it.

**Rules that fell out, worth applying before the next one is found:**

* A check and the action it authorises are one atomic step or they are not a
  check. If the lock is dropped between them, the window exists — "narrow" is
  not a property of the code, it is a property of today's timings.
* **Do not re-check at the call site.** `reindex_saved_file` reads the workspace
  and then does disk I/O; adding a root comparison there would have moved the
  window, not closed it. The check has to be inside the guarded section, which
  means the caller passes what it believed (`update_symbols(&root, …)`) and the
  callee decides whether that is still true.
* A method that cannot uphold an invariant should not pretend to. The flag clear
  had no generation to compare in `record_symbols` and could not get one without
  changing a signature owned elsewhere, so ownership moved entirely to
  `SymbolsBuildGuard`, which does have one. "Solve it where the information is"
  beat "condition it on the nearest available proxy".
* **"Self-healing" is a property of the callers, not the method.** Race 2 was
  harmless only because every caller spawns a build immediately afterwards.
  That is not an invariant anybody wrote down.

**Testing:** the seam pattern from the previous round generalised cleanly — a
private `*_interleaved` taking a closure run at exactly the unguarded instant,
production passing `||{}`, the test spawning a real thread that `try_lock`s the
workspace. `try_lock` is the point: it asks whether the lock is *free at this
instant*, which is exactly the question, and it reports instead of deadlocking
against the guard the fixed code now holds. Both new seam tests were verified to
fail by temporarily reverting the fix, not assumed to. Assert the window before
its consequences, or the failure message names the symptom and not the cause.

**Lock order** is workspace → symbols, now taken by three methods
(`record_symbols`, `set_workspace`, `update_symbols`) instead of one. Re-checked
by grep, not by memory: no code outside `state.rs` touches either mutex
directly, and nothing takes symbols and then reaches for the workspace. Backed
by `every_pair_of_these_locks_is_taken_in_one_order_under_load` (8 threads,
40,000 mixed ops) — which is a hang detector, not a proof, and is documented as
such.

---

## The read-then-slow-work-then-write defect class (instances 4 and 5)

Naming it, because this is now the **fifth** time it has been found in this
codebase and the next one will not be the last. Whoever hits number six: look
for this shape before you look for anything else.

**The shape.** A command reads shared state, does something slow that does not
hold the lock, and then writes back under the assumption that the read still
holds. It never does. The write is keyed by something that is not the thing that
changed — a config id, or nothing at all — so the stale write lands *on top of*
whatever cleared it, and looks like fresh, correct data.

**The five instances.**

| # | Site | Slow bit in the middle | What the user saw |
|---|------|------------------------|-------------------|
| 1 | `record_symbols` | the index walk (~1 s) | palette full of paths that open nothing, reporting `ready` |
| 2 | `set_workspace` | (inverted: publish, then clear) | correct index thrown away, flag saying nothing is building |
| 3 | `update_symbols` | reading the saved file off disk | one repo's symbols spliced into another's index, deleting its own entries |
| 4 | `record_test_run` | **the whole test suite — seconds to minutes** | "re-run failed" in workspace B filtering on workspace A's failed test names |
| 5 | `record_inspect` | sidecar spawn + heap walk + parse | one repo's object tree served under another, dump path rooted in the old one |

Instance 4 has by far the widest window in the app, and it is not a contrived
collision: `workspace::project_id` is deliberately relative (pinned by
`project_ids_are_relative_so_they_survive_a_move`), so `src/Api/Api.csproj` is
`src-Api-Api.csproj:test:debug` in *any* checkout with that layout. Two clones
of one service collide exactly.

**The fix is the same every time, and it is now a template:**

1. The caller names the root it produced its result under —
   `record_test_run(&root, id, result)`, not `record_test_run(id, result)`.
2. The callee compares it *inside* the workspace guard and holds that guard
   across the write. A check and the action it authorises are one step or they
   are not a check.
3. It returns `bool`. **Decide what a `false` means to the user, per caller.**
   A discarded test run becomes a warning on the run, because "re-run failed"
   will otherwise silently run the whole suite while claiming otherwise. A
   discarded capture becomes a console note, because the tree is still the
   honest answer to the request that is in flight — it just will not survive a
   view switch. Neither is `let _ =`; that would recreate a quieter version of
   the bug being fixed.
4. Test via the `*_interleaved(…, interleave: impl FnOnce())` seam, production
   passing `||{}`. Both new pairs were proved sensitive by running them against
   the old bodies behind the new signatures first — all four failed naming the
   real symptom ("the workspace was replaceable between the check and the
   store"), then passed once the guard went in. Nothing sleeps.

**A sibling worth recognising:** `(state.workspace_root(), state.workspace())` in
`inspect_attachable` was two acquisitions of one mutex, so the tuple could be
built from two different workspaces. Same class, no slow work in the middle,
mild consequence. Fixed by removing the second read rather than by guarding it —
if there is only one read there is nothing to disagree with. `inspect_capture`
had the same double read and got the same treatment.

**Lock order** is now workspace → cache, taken by five methods
(`record_symbols`, `set_workspace`, `update_symbols`, `record_test_run`,
`record_inspect`) over four mutexes. Re-derived by grep rather than trusted: the
only bodies mentioning two of `workspace` / `symbols` / `last_inspect` /
`last_test_run` are those five, and in each the `workspace.lock()` is the first
statement; every other accessor takes exactly one; the only direct touch outside
`state.rs` is `current_workspace`, which takes `workspace` alone. Backed by
`every_pair_of_these_locks_is_taken_in_one_order_under_load`, extended to all
four mutexes and 8 threads x 20,000 iterations x 12 ops = 1,920,000 operations.
A hang detector, not a proof.

**Poisoned cache mutexes were considered and deliberately left alone.**
`set_workspace`'s clears use `if let Ok(..)`, so a poisoned mutex skips the
clear. That is safe because every *reader* fails the same way — `previous_*` and
`symbols()` return `None`, the `record_*` return `false` — so a poisoned cache
is unreadable rather than wrongly readable, which is the side of "a wrong answer
is worse than no answer" we want to fail on. The one caveat, stated in the doc
rather than left implicit: the capture's memory stays allocated for the
process's life, unreachable but held.

## Known flake: `process::tests::cancel_stops_a_long_running_process`

Seen failing once in a full `cargo test -p cb-core` run, then passing 3/3 in
isolation and on an immediate full-suite re-run. Nothing in this work item
touches `process/`.

The `process::` tests spawn real child processes and assert on how quickly a
cancel takes effect, so they are timing-sensitive by construction and will fail
occasionally on a loaded machine — which is precisely the state this repository
is in while several agents each run their own `cargo` build. CLAUDE.md already
warns that these tests need `sh` on PATH and that concurrent cargo invocations
block on the shared `target/` lock; this is the same family of problem.

Before reporting it as a regression: re-run it alone. A single red
`process::` test in a parallel run is not evidence.

---

# Phase 3 gate (2026-08-11)

## `components.rs` was planned and does not exist — and that is fine

`todos.md` (and `plan.md`) specified `architecture/{graph,mermaid,components,store}.rs`.
Only three landed. `components.rs` was to hold the "component" abstraction above
raw projects; `graph.rs` covers the level-1/2 need through `ArchKind` — a node is
a project, a solution, a solution folder or an external — without a fourth
module. Nothing references a missing symbol and no test expects one, so this is a
scope decision the agents converged on rather than an omission that breaks
anything. Recorded here because the *next* person reading `plan.md` will look for
the file and needs to know it was not lost.

## Phase 4 and 5 work landed early, inside Phase 3

Three items filed under later phases are already implemented, which will make
those phases' checklists read as more remaining than there is:

- `architecture/mermaid.rs::validate` + `ValidationRule` (filed under Phase 4).
- Front-matter provenance, and the `derived`/`inferred`/`user` split with the
  directory following from the derivation (Phase 4).
- All six `arch_*` commands, the `types.ts` mirror, and the gitignore split for
  `diagrams/derived/` and `diagrams/.prompts/` (Phase 5).

What is genuinely still missing from Phase 5 is only the frontend: there is no
Architecture tab, and **nothing calls the six wrappers in `api.ts`**. They are
exported `const`s with zero callers, which `tsc` does not flag. Verified by
grepping `src/` for each wrapper name outside `ipc/api.ts` — no hits.

## The mixed-shape derivation enums are the sharpest IPC trap here

`Derivation` and `DiagramDerivation` carry **no `serde` tag attribute at all**,
unlike every other data-carrying enum in the crate (`ObjectValue`, `InspectTarget`,
`RootSpec` all use an internal `kind` tag). So they serialise *externally tagged*
and a variant's wire shape depends on whether it holds data:

- `Derivation::Derived { scanner: 1 }` → `{"derived":{"scanner":1}}`
- `Derivation::User` → the bare string `"user"`
- `DiagramDerivation::Derived` → the bare string `"derived"` (no payload)
- `DiagramDerivation::Inferred { agent }` → `{"inferred":{"agent":"…"}}`

That is why the two cannot share one TypeScript alias despite reading almost
identically — one sends `derived` as an object, the other as a string. Adding a
field to a bare variant silently changes its wire shape from string to object.
Pinned by the `derivation` assertions in `an_arch_graph_serialises_with_the_keys_the_ui_reads`
and by `a_derived_or_user_derivation_serialises_as_a_bare_string`; now documented
in `docs/architecture/ipc-contract.md`.

## No `skip_serializing_if` anywhere in `architecture/`

Deliberate, and the opposite of the `RunConfig` convention. `ArchNode.projectId`,
`path`, `ecosystem` and `ArchEdge.label` cross as explicit `null`. A container
node legitimately has no project behind it, so "no project" must not be
indistinguishable from a backend that forgot to send one — the `SearchHit` rule
from Phase 2, applied again. Verified before documenting it: `grep skip_serializing_if
crates/core/src/architecture/*.rs` returns nothing, and the edge assertion in the
graph pinning test lists `label` among the keys while the value under test is
`None`, so the null-not-absent behaviour is genuinely pinned rather than merely
implied by the missing attribute.

## `symbols.json` had been missing from the configuration doc since Phase 1

Noticed while adding `diagrams/` to the same tree. `config.rs::IGNORED` has
carried `symbols::cache::CACHE_FILE` since Phase 1, but neither the
`.code-basics/` tree nor the `.gitignore` sentence in
`docs/reference/configuration.md` mentioned it. Fixed at this gate. Worth a habit:
when `IGNORED` gains an entry, that doc has two places to update, and the pinning
test only guards the constant.

## The flake fired again, and the counting method that settles it

`process::tests::cancel_stops_a_long_running_process` failed on the first full
run of this gate and passed alone straight afterwards, then passed in a clean
full re-run. Third gate in a row. The useful part is the method for the *other*
question it raises — whether a red run hid a vanished test. Do not eyeball the
totals; run:

    git diff -U0 | grep -cE "^-\s*#\[(tokio::)?test\]"

Zero deletions is the answer, and it is immune to a suite that aborted early
(a failing lib target stops the integration targets from running at all, so the
totals for that run are not comparable to baseline anyway).

## Mermaid's directive scanner is quote-blind and comment-blind — check the raw line

`validate` used to look for `%%{` in the *quote-stripped* form of a line. An odd
number of `"` earlier on the line hid the directive from the check and from
nothing else: mermaid 11.16.1 matches `directiveRegex`
(`node_modules/mermaid/dist/mermaid.js:22713`, run at `:34660`) over the raw
text. `validate("flowchart LR\n  A[\"] %%{init:{\"flowchart\":{\"htmlLabels\":true}}}%%\n")`
returned `Ok(())`. Directives are now scanned raw and over the **whole file**,
not just the fenced body — `store::parse` hands on the whole post-front-matter
body, and mermaid finds a directive sitting in the prose just the same.

Two claims about what a directive can do, both checked in the shipped bundle
rather than assumed, because only one of them is true:

* `securityLevel` **is** in `defaultConfig.secure` (`:6344-6350`) and `sanitize`
  (`:6822`) deletes every secure key from a directive's arguments. A directive
  **cannot** loosen the security level. The old doc said it could.
* `htmlLabels` is **not** in that list and *is* a `configKeys` entry (via
  `keyify`, which flattens nested keys unprefixed), so
  `%%{init:{"flowchart":{"htmlLabels":true}}}%%` is applied. That is the half
  that matters: `htmlLabels: false` is the premise of every label-escaping
  argument in `mermaid.rs`.

Consequence for `render`: `escape_label` now separates adjacent `%` the same way
`comment` always has, because a `%%{` inside a *quoted label* is still a
directive to mermaid.

Where the quote-stripping exemption is still right: a `href` inside a quoted
label is genuinely inert, and dropping quoted spans is what lets a project
honestly named `<script>` render. `scannable` keeps that exemption but withholds
it when the line's `"` do not pair up — on such a line there is no "outside the
quotes" at all.

## "This crate has no regex dependency" was false

`crates/core/Cargo.toml:23` reads `regex.workspace = true`. It is declared and
simply unused anywhere in `src/`. Note also that the tempting fallback rationale
is *also* false: the `regex` crate matches in time linear in the input, so there
is no ReDoS argument to make. The honest reason for the hand-written scan is
readability at a security boundary — write that, not the plausible version.

## The legend, and why it is conditional

Rendered diagrams carried no key, so an exported `.mmd` gave a reader no way to
know dotted = npm dependency, solid = `<ProjectReference>`, stadium = external.
`render` now appends a `subgraph legend[...]` containing **only** the symbols the
diagram actually drew (tracked in a `Key` filled in while writing, not derived
from the graph — a containment edge realised as nesting draws no arrow). Legend
identifiers are collision-proof by construction: `mermaid_id` prefixes every node
with `n`, so nothing it emits can begin with `legend`.

Two projects sharing a `name` used to render as two identically-labelled boxes.
The path is now appended **only to the labels that collide** — appending it
everywhere trades a rare ambiguity for permanent noise.

Test-side consequence: `without_legend`/`legend_of` helpers in `mermaid_tests.rs`
keep the diagram assertions saying what they mean; the legend is pinned exactly
by its own tests.

## Per-agent `CARGO_TARGET_DIR` is a disk-space bomb

Giving every parallel agent its own `CARGO_TARGET_DIR` avoids the shared
`target/` lock — and costs **2–6 GB each**. Across seven workflows this session
that produced **48 abandoned target directories**, well over 100 GB, none of
which clean themselves up. A previous session did the same thing *inside the
repository* as `target/wf-*` (13 dirs, ~40 GB) where it is invisible to
`git status` because `/target` is gitignored wholesale.

The lock serialises builds; it does not break them. Serialised cargo is
usually cheaper than tens of gigabytes of parallel cargo. If you do split:
reuse a small fixed set of paths outside the repo, one per agent *role* rather
than one per agent per run (a warm dir is faster anyway), and delete them when
the work finishes.

Do the arithmetic before launching: *agents × 4 GB*. Recorded in CLAUDE.md and
`docs/guides/development.md` so it is not learned a third time.

## `Project::id` is not injective — the graph must not key on it alone

`workspace::project_id` replaces **both** separators with `-`, so
`src/a/App.csproj` and `src-a/App.csproj` scan to the same `Project.id`. The
graph builder deduped nodes by id, so one box vanished and the survivor was
handed the *other* project's `<ProjectReference>` arrows — silently, with empty
warnings.

Fixed in `architecture/graph.rs` (`NodeIds`), not in `workspace.rs`. Making
`project_id` injective would fix it everywhere, but the id is the prefix of
every derived `RunConfig` id (`configs_for_project(&id, …)` → `"{id}:run"`),
and `config.rs` persists saved configs, `favorites` and `order` by config id in
`.code-basics/config.json`. Changing the id shape would orphan every user's
saved configuration, favourites and ordering in every existing workspace.
That is disqualifying; the collision is handled at the one consumer that cares.

Colliding projects are drawn under `project:<relative path>` (unique by
construction; ordinary ids never contain `/`), get `projectId: null` because
that id no longer names one project, and produce a warning listing every
colliding path. Related: the node-dependency self-reference guard compared ids,
so a genuine edge between two id-colliding npm packages was discarded as a
self-reference; it now compares manifest paths.

## A rooted `Include` is legal MSBuild and resolves nowhere near the naive answer

`<ProjectReference Include="\Shared\Shared.csproj" />` is valid. So is
`C:\Shared\Shared.csproj`, and so is `C:Shared\Shared.csproj`. None of them
resolve relative to the referring `.csproj`, which is what a naive
`project_dir.join(raw)` assumes.

The join does not fail — that is what makes it dangerous. It quietly produces a
path *inside* the workspace, which then passes the `starts_with(root)` guard
honestly, and the graph draws a confident `ProjectReference` arrow at whichever
project happens to sit at that location. A wrong arrow, with no warning, from
input that was never malformed.

`architecture/graph.rs::is_rooted` decides this **on the string, not with
`Path`**, because `Path::is_absolute` is platform-dependent: it calls
`C:\Shared\Shared.csproj` *relative* on Linux, so a Linux CI run and a Windows
run would disagree about the shape of the same solution. Three spellings count:

* leading `/` or `\` — root of the current drive (this also covers UNC `\server\share`);
* a drive prefix `X:` **with or without** a following separator — `C:Shared` is
  relative to that drive's current directory, which is no more locatable from
  here than `C:\Shared`.

Rooted references become an `External` node (`external:<raw>`) rather than
being dropped, keeping the module's promise that anything which cannot become
an edge is *reported*, never silently discarded.

## A broken `package.json` deletes a project from the *whole app*, not just the diagram

This is the most important thing the second fix round found, and the fix that
landed does **not** address it.

`crates/core/src/workspace.rs:452`:

```rust
fn scan_node_project(root: &Path, path: &Path) -> Option<(Project, Vec<RunConfig>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed = node::parse_package_json(&content)?;
```

Both `?`s discard the error. A `package.json` with a stray trailing comma, or
one that cannot be read, makes `scan_node_project` return `None`, and the
directory scan simply moves on. The project is then absent from
`Workspace::projects` — so it has **no box in the Architecture diagram, no
entry in the Run tab, no detected test configuration, and no presence in the
Tests tab**. Nothing anywhere in the UI says why. To the user, a project they
can see on disk has ceased to exist, and the only clue is a comma.

`project_graph` now re-reads the dropped manifests and emits a warning naming
the file and the parse error's line, so the *diagram* is honest about it. That
is a diagram-layer patch over an app-layer bug: every other consumer of
`Workspace::projects` is still silently wrong, and a user who never opens the
Architecture tab still gets no signal at all.

**The real fix belongs in `workspace.rs`**, which needs somewhere to put a
per-manifest failure — a `warnings` (or `unreadable`) field on `Workspace`,
surfaced once at the top level rather than per feature. That crosses IPC, so it
means `model.rs`'s key-pinning test and `src/ipc/types.ts` in the same change,
and a place in the UI to show it. It was deliberately **not** attempted in a
fix round scoped to the graph. Tracked in `todos.md`.

The same `Option`-swallowing shape is worth auditing on the .NET side before
assuming it is a Node-only problem.

## The legend rule: count, not arrow presence

`mermaid.rs::write_legend` suppresses the key when it would carry fewer than
two entries. The competing rule — "suppress when no arrow styles are present" —
is also defensible and differs on exactly one case: a project plus an
unconnected external, which is one plain box and one stadium and no arrows.
That case is **kept**. A key earns its space by drawing a contrast, and the
shapes carry a claim here even though no arrow does: nothing about a stadium
says "outside this workspace", so a reader without the row sees two projects
where there is one project and one thing the scan could not see into. One row
draws no contrast at all and is clutter whether it is a shape or an arrow,
which is why the count decides.

Legend node ids are collision-safe by construction, not by luck: `mermaid_id`
prefixes every graph node with `n`, so no node identifier can begin with
`legend`. Pinned by `a_legend_identifier_can_never_collide_with_a_node_identifier`.

## `workspaces` in `package.json` is not authoritative when pnpm owns the repo

Verified against pnpm 10.14.0 on this machine: with a `pnpm-workspace.yaml`
present, `pnpm list -r` returns only the members that YAML file lists, and pnpm
prints *"The \"workspaces\" field in package.json is not supported by pnpm"*.
The `workspaces` globs contribute nothing.

So when both files are present the graph must draw **neither** containment: the
`workspaces` boxes are membership the package manager does not recognise, and
drawing them is worse than drawing nothing, because it puts real members
outside the container and labels the container from a file that was ignored.
The warning names both files and quotes the undrawn patterns, so a reader knows
which list was abandoned and which was never read.

This tightens, rather than replaces, the standing "pnpm workspaces are only
half-supported on purpose" item — parsing `pnpm-workspace.yaml` still needs a
YAML dependency this crate will not take, and still needs a product decision.

## Credential-leak channels in the component map (4 fixed)

Found by an adversarial sweep planting secrets in a synthetic workspace and
grepping every string reachable from `component_graph`.

1. **`applicationUrl` reached the exported mermaid.** `signals/dotnet.rs`
   interpolated the raw url into a MEDIUM signal's `detail`;
   `components.rs::cross_project_notes` prints `detail.text` verbatim for any
   detail whose project did not earn the box. Reachable whenever two scanned
   projects share a `Project::name` (`src/Foo` + `samples/Foo`). Fixed on both
   sides: the producer emits `launch profile '<name>'` and
   `Evidence::elided_value`, and `framework::screen` now runs `detail` through
   the same value-shape test as `label` (new
   `DiscardReason::DetailLooksLikeAValue`).
2. **A doc comment asserted a safety property the code lacked** —
   `cross_project_notes` claimed the detail was "producer prose that passed the
   gate's screen"; the gate never screened `detail` at all. Rewritten to state
   what each half of the fix actually rests on.
3. **`Discarded::label` suppressed the label for `SecretValue` only.**
   `LabelLooksLikeAValue` — the reason that exists *because* the label is a
   value — kept and printed it. Now suppressed for both.
4. **`nameable` in `dotnet.rs` was weaker than `framework::looks_like_a_value`**
   (no `host:port` check), so a connection-string key spelled
   `redis-prod.internal:6380` was quoted into a warning. `nameable` now calls
   the framework guard; `label_looks_like_a_value` was renamed
   `looks_like_a_value` and made `pub(super)` so there is one guard, not a copy.

**Lesson for the next producer/leak test**: assert over the *whole* reachable
surface — `{graph:?}` plus `mermaid::render` plus `serde_json::to_string` — not
over the one field the fix touched. `components_tests::everything_reachable` /
`leaks_nothing` do this; a unit test on a single function missed channel 1
entirely because the credential only surfaced through the mermaid `%%` comments.

## Route fabrication was caught by a running app, not by reasoning

The ASP.NET route scanner (`signals/routes.rs`) shipped three defects that every
review pass had read past, because each one is *locally* plausible and only
wrong against what ASP.NET actually registers at startup. They were found by
standing a real ASP.NET application up and comparing its live endpoint list
against what the scanner printed — as reported by the agent that fixed them;
the comparison itself is not reproduced in this repository, and what is checked
in is the doc comment at `routes.rs:402` reasoning from a **404 from a running
application** rather than from the source alone.

The three, all now abstentions rather than answers:

1. **`MapGroup` in one file leaked into every other file of the project.** A
   group prefix declared in `Program.cs` governs registrations in sibling files
   the scanner reads independently, so endpoints came out *without* the prefix:
   `GET /orders` for a path that is really `GET /api/v1/orders`. Pinned by
   `a_map_group_in_one_file_stops_endpoints_being_resolved_in_every_other_file_of_that_project`.
2. **A method taking a `RouteGroupBuilder` parameter** applies a prefix chosen
   by its caller, which this scan never resolves. Now stops the whole project
   resolving endpoints —
   `a_method_that_receives_a_route_group_builder_stops_that_project_resolving_endpoints`.
3. **An abstract controller declared endpoints.** `DefaultControllerTypeProvider`
   skips abstract types outright, so `[ApiController] [Route("api/[controller]")]
   public abstract class BaseController` with an `[HttpGet("ping")]` registers
   **nothing**; the scanner printed `GET /api/Base/ping`. Inheritance is not
   followed in either direction — see the doc block at `routes.rs:388-419` for
   why the derived half is refused too.

**The lesson is the method, not the three bugs.** Every one of these survived
review because reading the scanner against the source it parses cannot tell you
what the framework does at startup. A fabricated endpoint is strictly worse than
no endpoint list (house rule 6), and the only oracle that catches fabrication is
the framework itself. For the next framework-shaped producer — routes, DI
registrations, middleware order — the acceptance evidence is a running app's own
enumeration, not a synthetic fixture and not an argument.

### Cross-reference: the same failure class in the credential sweep

The leak recorded above is the identical shape one layer over. The credential
reached the exported mermaid through **`Detail::text`**, and the reason nobody
looked there is that `cross_project_notes` carried a doc comment *asserting*
that surface was safe — "producer prose that passed the gate's screen and names
projects only" — which was false in both halves at once: the .NET producer
interpolated a whole `applicationUrl` into the detail, and `framework::screen`
ran over `label` and never over `detail`. A rationale that was written but never
executed read exactly like a verified invariant and stopped the search. That is
CLAUDE.md house rule 4, and it is now the second time in this feature a doc
comment has claimed a guard the code did not have (see `lineToPos`, above).

### A filter that was narrower than the set it was counting

`signals::dotnet::connection_strings` applied its "exactly one candidate, or
abstain" rule to `declared_databases` — `DATA_CLIENTS` rows of kind `Database`
only. A project referencing `Npgsql` **and** `StackExchange.Redis` therefore had
exactly one *database*, so every connection-string key attached to PostgreSQL,
including the one the author called `Redis`. The abstention was correctly
implemented and counted the wrong set.

Nothing surfaced it, because `components::cross_project_notes` publishes only
*cross-project* details and this one is a project's own — so the false pairing
sat in `Component::details` waiting for the first view that lists a box's
details beside it. **A detail that is currently unpublished is not currently
correct; it is currently unread.**

The fix widened the candidate set to every `DATA_CLIENTS` row (no kind filter at
all, since every row in that table is a client of something reached over a
socket) and added one recovery clause: a key may also be placed when it *is* a
provider's name, compared as identity — both sides reduced to ASCII
alphanumerics and case-folded, so `sqlserver` ≡ `SQL Server` and `RedisCache`
matches nothing. No prefix, no substring, no threshold.

### One warnings list had two vocabularies

`ArchGraph::warnings` mixes producer warnings (which open with
`Project::name`, e.g. `Orders.Api:`) with `components::cross_project_notes`
(which opened with `Project::id`, e.g. `src-Orders.Api-Orders.Api.csproj:`).
The id is drawn nowhere, labels nothing, and is not even a path — `project_id`
flattens the separators out. `Projects::display_name` now translates, falling
back to the id only in the two cases `Projects::resolve` already refuses (no
project, or several sharing an id).

**Still on the raw id:** `framework::render_warning` opens every gate refusal
with `Discarded::project_id`. It has no access to the scan, so fixing it means
either carrying a name on `Signal` or translating in `components.rs` after the
fact. Not done here — `framework.rs` was outside this change's ownership.

## Cargo round: three things worth knowing next time

**1. Adding a built-in adapter can silently delete a user's configurations.**
Before this round the manifest-merge loop in `workspace.rs::scan` skipped any
directory a built-in adapter had already claimed. Adding cargo detection would
therefore have removed every configuration
`examples/adapters/cargo-nextest.toml` supplies, on the next scan, for every
workspace using it — with no error. The fix is the `p.ecosystem != "cargo"`
guard: cargo is the one built-in that produces a project and *zero*
configurations, so it must not shadow. An unreadable cargo project is excluded
from the merge as well, because it carries a reason precisely because nothing
about it can be trusted.

**2. `skip_serializing_if` is what makes `unreadable?: string` honest.**
`Project.unreadable` is `Option<String>` with
`#[serde(default, skip_serializing_if = "Option::is_none")]`, so a healthy
project has **no key at all** rather than `null` — which is what lets
`RunView` test it with a plain truthiness check and lets `types.ts` mirror it
as optional. `project_serialises_with_camel_case_keys` in `model.rs` pins both
halves: absent for a healthy project, present with the exact reason for a
broken one. Change one half without the other and only the runtime notices.

**3. Cargo resolves dependencies by path, not by name — so the graph must too.**
A cargo `path` dependency states *where* the crate is. Matching by name (the
npm rule) would be a second, weaker answer to a question the manifest already
answers exactly. Likewise `[workspace] members` globs resolve against the
**manifest's own directory**, not the workspace root, because a cargo workspace
can sit in a subdirectory. Both are in `architecture/graph.rs` and both differ
deliberately from the Node rules a few functions above them.

**Clippy baseline drift.** The two pre-existing warnings are unchanged in kind
and file, but the `cmp_owned` one moved from `workspace.rs:1028` to `:1581`
because the file grew ~680 lines. A baseline pinned to a line number reads as a
regression when it is not — pin the lint plus the file.

## Gate round: two ways "it looked fine" was not evidence

**1. quick-xml reaches `Eof` cleanly on a document that just stops — so "it
parsed" never meant "it is well-formed".** `workspace.rs::xml_error` used to ask
only "did `read_event` return `Err`?". It does not for an unclosed element (the
stream simply ends at `Event::Eof`), and it never inspects a tag's attributes
unless something iterates them, so five of six malformed `.csproj` shapes were
reported as healthy and `unreadable_project`'s promise held in one case out of
six. The function now tracks element `depth`, drains `e.attributes()` on every
`Start`/`Empty`, and checks the root is `<Project>` — an unclosed document
becomes `unexpected end of file: N unclosed element(s)` rather than silence.
Generalises past XML: a parser returning `Ok` answers "did I stop cleanly?",
not "is this valid?" — if you need the second question, ask it yourself.

**2. The graph's own blind spot went unreported until an auditor read
`tauri.conf.json` by hand.** None of the derivation rules opens that file, so
this repository's own desktop shell, frontend and bundled sidecar sat on the
diagram as three unrelated boxes with nothing said — the exact silent
abstention the module doc calls "just as misleading and much harder to notice".
`graph.rs::unexpressible_relations` now names the file and what it points at in
`ArchGraph::warnings`. It creates **no node and no edge**: a relation the rules
cannot express is prose, not an arrow. When a rule set is deliberately narrow,
the narrowness itself has to be emitted — otherwise the only person who finds
it is someone reading the manifests by hand.

**Clippy baseline drift, again.** `cmp_owned` in `workspace.rs` has now moved
`:1028` → `:1581` → `:1729` as tests were appended. Same lint, same code, third
line number. Pin the baseline to lint + file; a line number will read as a
regression every round.

---

## Phase 4 gate: two measurement traps

### `pnpm coverage`'s printed table is not the file list

The vitest v8 **text** reporter omits files at 100%. On this tree it printed 8
rows out of 16 included modules, and `copyLogic.ts`, `viewportLogic.ts`,
`searchLogic.ts` and others were simply absent — which reads exactly like "not
in the include list". They were all at 100%. To get the real list:

    npx vitest run --coverage --coverage.reporter=json-summary \
      --coverage.reportsDirectory=<scratch>/cov

then read `coverage-summary.json`. Do not conclude a module is unglobbed from
the printed table; check the include pattern in `vite.config.ts` instead.

(One module genuinely *is* unglobbed — `nodeTargets.ts`, which does not match
`src/**/*Logic.ts`. That is in `todos.md`.)

### A big entry chunk is not evidence that a dynamic import failed

`pnpm build` reports a 1.47 MB entry chunk. The instinct is that mermaid landed
in it. The cheap way to actually know, without a before/after build:

    node -e "const m=JSON.parse(fs.readFileSync('dist/assets/index-*.js.map'));
             m.sources.filter(s=>/mermaid/i.test(s))"

Empty ⇒ not in the chunk, whatever the size says. Totalling
`sourcesContent[i].length` grouped by package then tells you what the size
*actually* is (here: react-dom, `@codemirror/view`, xterm). Also worth grepping
the chunk for `import("./mermaid` to confirm the reference is dynamic — a
static import would appear as a top-level `from"./mermaid…"`.

### The `cancel_stops_a_long_running_process` flake is real

Failed in the full `cargo test -p cb-core` run (assertion at
`process/mod.rs:487`, `assert!(sup.cancel("long").await)`), then passed 3/3 when
run alone in 0.3 s. It only fails under the load of the full suite. CLAUDE.md
documents it; re-run alone before reporting it, and report the count as
1541 total rather than "1540 passed".

### Node clicks in the diagram never fire — `data-id` does not exist in mermaid 11.16.1

Verified in the running app (WebView2 remote debugging, `arch_project_graph`
invoked directly). `DiagramCanvas.NODE_SELECTOR` is `"g.node[data-id]"`.
Mermaid 11.16.1 emits nodes as

    <g class="node default" id="cb-diagram-1-flowchart-ncrates_2d_core-3" data-look="classic">

with **no `data-id`** — only edge paths carry `data-id` (`L_a_b_0`). So
`markOpenable` walks an empty NodeList, `document.querySelectorAll("[data-cb-open]").length === 0`,
the computed cursor over a box is `grab` (never `pointer`), and clicking
`cb-core` leaves the active tab on Architecture instead of routing through
`requestOpenFile`.

Fixing the selector is necessary but not sufficient: the DOM id embeds the
*mermaid* id (`ncrates_2d_core`), while `targetFor` looks up the *graph* id
(`crates-core`). The `-`→`_2d_`, `/`→…, `n`-prefix escaping done by
`architecture/mermaid.rs` has to be reversed (or the id round-tripped some
other way) before the lookup can hit.

`nodeTargets.test.ts` passes throughout — it feeds `targetFor` raw graph ids and
never sees the mermaid id shape. The seam is the wiring, not the logic.

### How to drive this app when the workstation is locked

`SetForegroundWindow` + `SendKeys` + `CopyFromScreen` all fail: `LockApp`
("Windows Default Lock Screen") owns the foreground and the screen capture
returns only wallpaper. Two things still work and were used for this
verification:

* `PrintWindow(hwnd, dc, 2 /* PW_RENDERFULLCONTENT */)` renders the window's
  real content into a bitmap regardless of what is on screen.
* Launching with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`
  exposes CDP on `http://127.0.0.1:9333/json/list`. `Input.dispatchMouseEvent`
  and `Input.dispatchKeyEvent` drive the UI properly, and
  `window.__TAURI_INTERNALS__.invoke("arch_project_graph")` calls the real IPC
  from the page (the global is non-enumerable, so `Object.keys(window)` misses it).

Posting `WM_LBUTTONDOWN`/`WM_LBUTTONUP` straight to the
`Chrome_RenderWidgetHostHWND` child also activates React `onClick` handlers, but
it does **not** drive `:hover`, so it cannot be used to test hover styling.
