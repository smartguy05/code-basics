# Todos

## Handoff — needs a human at a keyboard

Phases 1 and 2 are code-complete and verified as far as this environment allows.
The pnpm junction block means **real vitest never ran**; the shim is not vitest.

- [ ] `pnpm typecheck && pnpm test` — the actual gate.
- [ ] Manual pass in `pnpm tauri dev`:
      - **Phase 1:** icons in both trees; Run/Stop tooltips; branch filter +
        Escape; Alt+F1 on a deeply nested file; Alt+F1 with two codebases open
        (the `offsetParent` guard); whether `scrollIntoView` actually scrolls.
      - **Phase 2:** branch widget centred at several window widths; rename a
        tab and reopen the codebase (name survives); two codebases with the same
        basename (the un-renamed one still disambiguates); open a large C# file
        and watch the loading chip appear bottom-**right** then disappear; open
        the chip's menu from the bottom bar and confirm it opens **upward**;
        switch codebases with nothing open in either (the stale-list fix).
- [ ] Decide about `.claude/settings.json` — tracked, and now points the qgate
      hook at `target/release/cb-app.exe` rather than the Program Files install.

## Phase 3 — merge Run + Changes into a Project tab

**Code-complete 2026-09-02** (see plan.md). Typecheck clean, 1546 logic tests
pass. Every rendering claim is unverified — nothing has been run.

## Phase 4 — floating SQL, combined terminal button, command shortcuts

Not started. Only phase touching Rust (`Launchable` gains `shortcut`/
`persistent`/`headless`); update the `model.rs` key-pinning test **and**
`src/ipc/types.ts` together.

## Carried over, not caused by this work

- [x] Two pre-existing `cb-core` failures — **fixed 2026-09-02**, both were
      defects in the tests rather than in the code. See
      `.memories/bugs/two-red-cb-core-tests/`. `cargo test -p cb-core` is now
      2963 passed / 0 failed.
- [ ] `OutputConsole` and `BranchMenu` still hand-roll context menus instead of
      using `ContextMenu.tsx`. Phase 2 added a third proper caller (the codebase
      tab menu); migrate one of the two when next touching them.
- [ ] `CLAUDE.md` describes the titlebar buttons, the tab list and the Run
      toolbar in detail. Phase 2 moved the LSP indicator out of the Run toolbar
      and rearranged the titlebar — **update it in Phase 3**, together with
      `pnpm docs:index` / `docs:check`.
