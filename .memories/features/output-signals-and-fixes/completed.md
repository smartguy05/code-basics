# Completed

## 1. Severity filter for launched-app output — done

- `src/components/consoleLogic.ts` — the classifier already existed (`Severity`,
  `SEVERITY_RANK`, `lineSeverity`, `filterLines`); it was buried behind Ctrl+F and had no
  idea which stream a line came from. Added `LineStream`/`ConsoleLine`,
  `appendConsoleLines`, `joinConsoleLines`, `filterConsoleLines`, and an optional
  `stream` argument to `lineSeverity`.
- `src/components/OutputConsole.tsx` — `rawRef: string` became `linesRef: ConsoleLine[]`
  (`LINE_CAP` replaces `RAW_CAP`). New optional `severity`/`onSeverityChange` props make
  the threshold controllable from outside; omitted, the console behaves as before.
- `src/components/appOutputLogic.ts` — `AppTab.severity` + `setTabSeverity`.
- `src/components/AppOutputPanel.tsx` — the picker, in the toolbar, per tab.
- `src/App.tsx` — `onSeverityChange` wired to `setTabSeverity`.

## 2. Folder-tab signalling — done

- `src/components/workspaceTabsLogic.ts` — `TabSignal`, `mergeSignal`, `tabSignalClass`.
  `shouldFlashWorkspaceTab` kept: `tabSignalClass` delegates the background-only rule to it.
- `TerminalPanel` gained `onCompleted` (fired only while minimized); `WorkspaceTab` gained
  `onSignal(root, signal)`; `RunView` gained `onBuildResult`.
- `src/App.tsx` — `signalByRoot` (latched) beside `attentionByRoot` (live), plus
  `raiseSignal`, `clearSignal`, and `DONE_SIGNAL_MS` for the transient `done` signal.
- `src/styles.css` — one keyframe, four colour classes via custom properties.

## 3. Run output panel stuck minimized — fixed

`consolePanelLogic.shouldForceExpand` plus an effect in `RunView`. The persisted flag is
rewritten too, or the next open re-enters the trap.

## 4. Apps-panel scrollbar not draggable — fixed (needs a visual confirm)

`styles.css`: `.xterm .xterm-viewport::-webkit-scrollbar` given a real 10px width.
See `notes.md` for why an overlay scrollbar made the thumb unclickable.

## 5. Successful build closes its output tab — done

`consolePanelLogic.isBuildSession` / `shouldCloseBuildSession`, consumed in `RunView`'s
`exited` case. `closeSession` gained a `keepStatus` option so the sidebar green build dot
outlives the tab.

## 6. secrets.json "not valid JSON" — fixed

Root cause was NOT the comments: a UTF-8 BOM. See `notes.md`.
`crates/core/src/secrets.rs` — `strip_jsonc` now tolerates a leading BOM, and a new
`jsonc_error` quotes the offending line.

## Docs updated with the change

- `CLAUDE.md` — the `src/` bullet: the four tab signals (over the older bell-only
  `shouldFlashWorkspaceTab` sentence), and the xterm-viewport scrollbar rule beside the
  existing "terminal panes are `overflow: hidden`" one.
- `README.md` — Terminals, Multiple codebases, Apps panel, Console, and the Edit bullet
  (the console re-expands when the last file closes).
- `docs/getting-started/using-the-app.md` — tab signals in the multi-codebase intro, the
  severity picker in the Apps panel, build-tab auto-close on the Run toolbar, the
  console-expands-on-last-file-close rule, and the amber/green wording for terminals.
- `docs/architecture/frontend.md` — a new **Tab signals** section (the live-state vs.
  latched-event split, and why `mergeSignal` exists), the `ConsoleLine[]` store and the
  overlay-scrollbar explanation on `OutputConsole`, plus the file-tree entries.
- `docs/architecture/core-crate.md` — what `strip_jsonc` actually defines as the dialect,
  including the BOM and the line-quoting error.
- `docs/reference/configuration.md` — the same, for the user-facing secrets section.
- `docs/INDEX.md` — regenerated.

**`scripts/generate-index.mjs` had a one-line bug** worth knowing about: `summary()` broke
out of its search on any non-empty line that did not itself yield text, so a file whose
header starts with a bare `/**` line (the text being on the *next* line) got no purpose in
the index at all. 16 source files were affected. Fixed by ending the search only on real
code. That plus a `//!` header on `consoleLogic.ts`, which had none, took the index from
115 purposeless non-test rows to 95 — the rest genuinely have no header comment.

## Verification actually run

- `cargo test -p cb-core` — 2386 passed. (`process::tests::restarting_an_id_...` is a
  load-sensitive flake: it passes in isolation and with `process::` alone, and no Rust
  outside `secrets.rs` was touched.)
- `cargo fmt --check` — clean.
- Full-project `tsc --noEmit` — 0 errors in every changed file.
- All frontend test files except `historyLogic.test.ts` and `language.test.ts` —
  1047 passed, 0 failed. Those two need async/mock support the local harness lacks;
  neither covers anything changed here.
