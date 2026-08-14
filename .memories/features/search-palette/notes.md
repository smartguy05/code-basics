# Search palette — notes

## Selection must be scrolled into view (fixed)

`.palette` is `max-height: 70vh` and `.palette-results` scrolls, but nothing
scrolled the selected row. With more than ~12 hits the `.selected` row walked
below the fold and Enter acted on a row nobody could see — worst at the wrap,
which puts the selection on row 0 with the viewport at the bottom.

Fix: a `rowRefs` array indexed like the flattened hit list, plus an effect that
calls `scrollIntoView({ block: "nearest" })` on the selected row. A
`pointerMoved` ref suppresses the scroll when the *mouse* moved the selection —
otherwise hovering slides the list under the cursor, which lands the cursor on
another row and moves the selection again.

## Only application configurations are actionable

The palette ranks over every `RunConfig`, but the one consumer of an action hit
is the Run tab, whose list is `kind === "app"` only. A test configuration was
therefore offered, chosen, and silently ignored. `actionableIds` +
`dropUnactionable` in `searchLogic.ts` filter action rows before they are drawn.

Routing a test configuration to the **Tests** tab instead is the better feature
and was not built: it needs a second pending-request slot in `App.tsx` and a
consumer in `TestsView`, and this work item did not own `App.tsx`.

## Hits must never outlive the search they answer

`close()` cleared neither `query` nor `hits`, so reopening under a different
scope showed the previous scope's rows for the debounce plus round trip — and
Enter in that window acted on one. The query is kept (Rider does, and `openAt`
selects the text so the next keystroke replaces it); the rows are not.
`searchKey(scope, query)` is recorded alongside the hits and `resultsState`
decides between prompt / pending / empty / hits, so "No matches." is only ever
printed after a search for what is on screen answered with none.

## A saved file has two roots, and only one of them is the index's

`Repo::open` is `git2::Repository::discover`, so a repository found *above* the
opened workspace makes every Changes-tab path repository-relative: open
`C:/repo/src/Api` and the tab lists `src/Api/Program.cs`. `git_write_file`
passed that string straight to the re-index, which read it as
workspace-relative — so the edited file kept its stale entry while the path that
named nothing joined `SymbolIndex::files` as a palette row that opened nothing.
Verified against real libgit2: `workdir()` comes back as
`"C:/Users/.../code-basics/"` (forward slashes, trailing separator) while the
workspace root is dunce-canonicalised with backslashes.

`reindex_saved_file` now takes an **absolute** path — the one thing both editors
can state unambiguously — and `index::relative_to_root` re-keys it, abstaining
(`None`) for anything not under the workspace. `replace_file` additionally
`stat`s before admitting a path to `files`: the lexical gate cannot tell a new
file from a well-formed path naming nothing.

## replace_file was re-sorting the whole table on every save

Measured on a synthesised 2,865-file / 17,184-symbol workspace: `retain` +
`extend` + full re-sort ran 3.1–5.0 ms per save, against 0.07–0.20 ms for a
splice over the contiguous run the file already occupies (~30x). The remaining
cost is not in `cb-core` at all — `AppState::update_symbols` goes through
`Arc::make_mut`, which deep-clones the ~2.2 MB index (2.5–3.6 ms) whenever a
search holds the same handle, and 100 ns when nothing does.
