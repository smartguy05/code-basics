# Completed — Changes tab folder-tree layout

## What
Added a "List / Tree" layout toggle to the **Files** view of the Changes tab so
the changed files can be shown as a collapsible folder tree instead of one flat
list of full paths.

## Files touched
- `src/views/folderTreeLogic.ts` (new) — pure `buildFileTree` / `flattenFileTree`.
  Builds a per-section folder tree; flatten emits folders-before-files, alpha
  sorted, with depth for indentation and a per-folder `collapsed` flag.
- `src/views/folderTreeLogic.test.ts` (new) — vitest for structure, recursive
  `fileCount`, ordering, depth, and collapse hiding descendants.
- `src/views/ChangesView.tsx` — `filesLayout` state (persisted under
  `code-basics.filesLayout`), `collapsedFolders` set (in-memory, keyed
  `${section.key}:${folderPath}`), `renderSectionTree`, `renderFileRow` extended
  with an optional `tree` arg (leaf label + depth indent), and a Layout
  segmented toggle shown only in the Files view when there are changes.
- `src/styles.css` — `.file-list .folder-row` twisty/label styling.

## Notes
- The tree is built **per section** (Staged / each group / Unstaged) so the git
  partition from `changesLogic.buildSections` is never reshaped — the tree only
  reorganises the files *within* a section.
- Selection, risk emphasis, and the right-click stage/group menu are unchanged
  in tree mode — the same `renderFileRow` button renders both layouts.
- Folder collapse is in-memory (matches the section `collapsed` behaviour); the
  chosen layout persists to localStorage.
