# Diff view: whole-file green + non-intent highlighting (2026-08-17)

## Symptoms
Viewing an intent card's diff: (1) the **entire right pane was green**; (2) even
after toggling "Ignore whitespace", the green covered **every change in the
file**, not just the intent's hunk.

## Root causes
Both in the side-by-side path. The green comes from `@codemirror/merge`'s
`MergeView`, which runs **its own** diff over the two full documents — the
libgit2 `FileDiff` only feeds the marker strip and line-selection (which is why
selection was correct).

1. **Line endings (#1).** `repo.rs::head_content` returns the raw HEAD blob
   (LF); `working_content` returns raw disk bytes (CRLF on Windows under
   `autocrlf=true`). MergeView diffs the two raw strings, so every line differs
   by a trailing `\r` → all green. git filters this (autocrlf); the app didn't.
   Confirmed: a minimal `presentableDiff("a\n…","a\r\n…")` returns one change per
   line. "Ignore whitespace" worked around it because `normaliseWhitespace`
   strips `\r`.
2. **Scope (#2).** `ChangesView` computed `shownDiff = onlyHunks(diff,
   groupHunks)` (scopes the strip + selectable lines) but still passed the
   **whole** `baseline`/`working` to `DiffView`, so MergeView greened every hunk.

## Fix (frontend, pure logic in diffLogic.ts + vitest)
- `normaliseEndings(s)` — `/\r\n?/g → \n`. `DiffView` normalises both
  `baseline`/`working` before building the editor, so MergeView always compares
  LF-vs-LF (fixes #1 for every caller, not just Changes).
- `focusedBaseline(working, hunks)` — rebuilds the baseline as the working copy
  with only the given hunks reverted (splice working-side lines → baseline-side
  lines, bottom-up). `ChangesView` passes this as the baseline (memoised on
  `[grouping, groupHunks, diff, contents]`) when an intent card is open, so every
  region outside the card's hunk is identical on both sides and only that hunk
  shows green — with real line numbers, others foldable via collapse. (#2, the
  user chose "focused hunk view".)

Files: `src/components/diffLogic.ts` (+ `.test.ts`, 7 new tests),
`src/components/DiffView.tsx` (normalise both docs in the build effect),
`src/views/ChangesView.tsx` (`viewBaseline` memo → `baseline` prop).

## Notes / gotchas
- **Memoise the focused baseline.** `DiffView`'s build effect has `baseline` in
  its deps; a fresh string identity each render tears down and rebuilds the
  editor (losing cursor/scroll). Memo depends on the raw state (`diff`,
  `groupHunks`, `contents`), NOT on `shownDiff` (a new object every render).
- Staging/revert are unaffected — they act on `shownDiff` indices, not the
  baseline string. The MergeView "revert chunk to baseline" now restores the
  intent's baseline version, which is correct.
- **Latent, not fixed:** CodeMirror normalises CRLF→LF on load, so saving an
  edited CRLF file through the diff view writes it back as **LF** (an ending
  flip on save). Separate from this bug; flagged to the user.
- `cargo`/Rust untouched. `pnpm typecheck` clean, `pnpm test` 636 green.
  Verified live via Vite HMR in the running app.
