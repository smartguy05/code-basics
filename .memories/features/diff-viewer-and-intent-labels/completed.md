# Completed — diff viewer and intent labels (2026-08-13)

All twelve items in `plan.md` landed. Nothing was dropped or scaled down.

## Part 1 — the diff viewer

| What | Where |
|---|---|
| Dark diff palette | `DiffView.tsx` (`EditorView.darkTheme.of(true)` on both panes) + `.cm-changed*` / `.cm-deleted*` / `.cm-inserted*` / `.cm-collapsedLines` rules in `styles.css` |
| App-wide font size | `editorFontSizeLogic.ts` (+ test) for the decisions, `editorFontSize.ts` for the DOM, `--editor-font-size` in `styles.css`; `A−`/`A+` in the toolbar and `Ctrl+-` / `Ctrl+=` / `Ctrl+0` in `App.tsx`. `DiffView`, `FileEditor` and `DiagramEditor` each `requestMeasure()` on the change event |
| Synced horizontal scrollbar | `scrollThumb` / `scrollLeftForThumb` in `diffLogic.ts`; the sticky `.diff-hscroll` in `DiffView` drives `scrollLeft` on **both** panes, and a `scroll` listener keeps them equal when CodeMirror moves one itself |
| Change-marker strip | `changeMarks` in `diffLogic.ts`, `.diff-markers` on the right edge of the new `.diff-frame` grid |
| Next/previous change | `nextChangeLine` in `diffLogic.ts`, exposed as `DiffViewHandle.goToChange`; toolbar ↑/↓ and F7 / Shift+F7 in both `ChangesView` and `HistoryView` |
| Difference counter, collapse-unchanged | `ChangesView` toolbar; `collapseUnchanged` passed to both `MergeView` and `unifiedMergeView` |
| Whitespace-ignore (display only) | `normaliseWhitespace` / `mapOffset` in `diffLogic.ts`, fed to `DiffConfig.override` as `whitespaceInsensitiveDiff`. Staging and reverting still use the exact `FileDiff` |
| History on the same viewer | `Repo::commit_file_contents` + `git_commit_file_contents` + `gitCommitFileContents`; `HistoryView` renders `DiffView` read-only over a file-chip strip. `unifiedText` and its tests were deleted with their last caller |

`FileContents` moved from `src-tauri/src/commands/git.rs` into `cb-core`
(`git/repo.rs`) so both commands return one type.

## Part 2 — intent labels

- **`hook::looks_like_narration`** — one shared gate refusing tooling talk,
  pleasantries, next-step announcements and questions. Used by the Stop hook's
  fallback *and* by `claude_code::summarise`, so the live path and the history
  importer cannot disagree. A declared `Intent:` line bypasses it entirely.
- **`IntentLabel::source`** (`Declared` / `Inferred`, defaulting to `Inferred`)
  flows through `AttributedSpan::label_source` into `grouping.rs`. Only a
  declared label yields `GroupKind::Intent`. Everything else becomes the new
  **`GroupKind::SameTurn`**: still grouped by turn, titled from the mined
  sentence if one survived, otherwise derived (`Bucket::derived_title` —
  "N files changed together", or the shared enclosing symbol).
- **The gate runs on read as well as on write.** `intents::load` re-judges
  inferred labels through `hook::is_usable_inferred_label`, because this
  workspace already held 261 labels recorded before the gate existed and those
  were the ones titling cards. Also fixed there: `contains_word` did not match
  plurals, so "Running — 10 agents" was never seen as tooling talk, and the
  inferred-label floor rose to 12 characters ("Running", "Both done").
- **`hook::ask_for_intent`** + `recorder.rs` exit 2 — the Stop hook asks for a
  real `Intent:` line. Gated on Claude Code, capture-enabled workspace, a turn
  that actually edited files, no declared line, and **asked at most once per
  turn** via `.code-basics/intents/asked.jsonl`. `config.askForIntent` turns it
  off.

Two `summarise` tests changed their *sample* (not their rule): they used
"Let me read the failing test:" and "Should the adapter own this?" to exercise
colon-stripping and the `?` terminator, and the gate now refuses both shapes.

## Verified

- `cargo test -p cb-core` — 1630 passed. `cargo clippy` adds no new warnings
  (two pre-existing remain in `rider.rs` and `workspace.rs`). `cargo fmt` clean.
- `pnpm test` — 467 passed. `pnpm typecheck`, `pnpm docs:index`, `pnpm docs:check`
  clean. Coverage 99.6% lines over the logic modules (gate is 70%).
- **In the running app**: every intent card honestly titled — no card claims a
  stated intent, and the tooling narration is replaced by derived titles like
  "11 files changed together"; both panes scrolling together, legible colours with
  word-level marks, `A+` resizing every editor with alignment intact, F7 and the
  toolbar arrows stepping through changes, the marker strip, "2 differences",
  and the History tab rendering a commit in the new viewer.

## Not verified

The Stop-hook request has unit tests and the blocking contract was confirmed
against the official docs (exit 2 = "prevents Claude from stopping"), but it was
not exercised in a live agent session — the installed hook points at
`target/release/cb-app.exe`, so it does nothing until a release build.
