# Todos — stash selected files

## Done
- [x] Core `Repo::stash_paths` + 17 integration tests
- [x] `git_stash_paths` command, registration, `gitStashPaths` wrapper
- [x] Multi-select in the Changes file list (Ctrl/Cmd-click, Shift-range)
- [x] Context menu item, with the message prompt
- [x] Migrated ChangesView's hand-rolled menu to `components/ContextMenu.tsx`
- [x] Docs: command table + regenerated INDEX.md

## Open / known limits
- [ ] **`stash_pop` refuses while anything is staged.** libgit2's
      `ensure_clean_index` checks the *whole* index, not just the stashed paths,
      so the natural flow "stash file A, keep working with staged file B, pop"
      errors. Pre-existing behaviour of `stash_pop`, stricter than the git CLI,
      but this feature makes it much easier to hit. Worth either surfacing in
      the error text or restoring through a merge that does not go through
      `git_stash_apply`.
- [ ] Only the Files view has the action. `IntentPanel`'s own right-click menu
      (already on `ContextMenu`) could offer "stash this card's files".
- [ ] Two remaining hand-rolled context menus — `OutputConsole`, `BranchMenu` —
      still lack Escape-to-close and viewport clamping. Migrate one next time
      either is touched.
- [ ] Conflicted files are filtered out in the frontend and refused in the core,
      but there is no integration test building a real conflict for the core
      refusal; the guard is covered only by the frontend's `stashablePaths`.
