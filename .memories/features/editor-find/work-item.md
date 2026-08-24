# Ctrl+F find-in-file

QoL: Ctrl+F searches within a file's CodeMirror editor (was only available in the xterm console).

## Done (2026-08-18)
- Added dep `@codemirror/search` 6.7.1 (pnpm add).
- Wired `search({top:true})` + `highlightSelectionMatches()` + `...searchKeymap` into:
  - src/components/FileEditor.tsx (plain file editor)
  - src/components/DiffView.tsx (unified + side-by-side; both the working `extensions` array AND
    the read-only baseline `a` pane)
  - src/views/architecture/DiagramEditor.tsx
- Themed the panel + match highlights in src/styles.css (.cm-panels/.cm-search/.cm-searchMatch*).
- No conflict with the console's Ctrl+F: that handler is window-capture but guarded by
  `hostRef.offsetParent === null` (only fires when the console is visible), so a focused file
  editor gets Ctrl+F. See OutputConsole.tsx:203-214.
- Verified: pnpm typecheck clean, pnpm test 737 pass.
