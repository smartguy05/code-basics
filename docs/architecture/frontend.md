# The frontend (`src/`)

React 19 + TypeScript, built with Vite, rendered inside the native Tauri window via the platform WebView. No router and no state library — the app is a single window with four tabs, and each view owns its own state. Everything the frontend "does" is an `invoke` call into the [Tauri shell](tauri-shell.md).

## Structure

```
src/
├── main.tsx              entry: mounts <App/> in an ErrorBoundary; suppresses the
│                         webview context menu (editable fields keep the native one)
├── App.tsx               titlebar (branch widget, run-config slot) + tab row (Run
│                         first), workspace open/reopen, recents (localStorage);
│                         Run + Tests stay mounted while hidden so running
│                         processes keep their consoles
├── views/
│   ├── RunView.tsx       file-tree sidebar, editor pane over per-run console tabs,
│   │                     config dropdown (portaled to the titlebar), env picker,
│   │                     build actions, secrets
│   ├── TestsView.tsx     test configs, run / re-run failed, live progress + tree
│   ├── ChangesView.tsx   git status, comparison modes, side-by-side/inline diff
│   └── HistoryView.tsx   commit log, per-commit diffs, branches, push/pull/fetch
├── components/
│   ├── OutputConsole.tsx xterm.js terminal: links, severity colours, search/filter,
│   │                     copy-on-select, context menu with Copy diagnostics
│   ├── TestTree.tsx      collapsible outcome tree with text/outcome filters
│   ├── DiffView.tsx      CodeMirror diff (side-by-side MergeView or unified),
│   │                     per-line selection
│   ├── ConfigEditor.tsx  RunConfig form (project, launch profile dropdown, args,
│   │                     env, cwd, ...; Delete lives in its footer)
│   ├── BranchMenu.tsx    titlebar branch widget: tree, sections, fetch/pull/push,
│   │                     right-click create-from / merge-into, abort-merge
│   ├── RunConfigMenu.tsx titlebar run-config dropdown: status dots, favourites,
│   │                     reorder, new/import items (portal from RunView)
│   ├── FileTree.tsx      lazy workspace directory tree (one fs_list_dir per expand)
│   ├── FileEditor.tsx    CodeMirror editor over one file; Ctrl+S saves, reports dirty
│   ├── language.ts       file-extension → CodeMirror language mode, plus the
│   │                     shared syntax-colour theme and bracket matching
│   ├── EnvironmentPicker.tsx  ASPNETCORE_ENVIRONMENT dropdown with in-menu add/remove
│   ├── SecretsEditor.tsx .NET user-secrets modal
│   ├── Sidebar.tsx       the resizable left column (shared stored width)
│   ├── ErrorBoundary.tsx last-resort error screen instead of a blank window
│   └── RiderImportDialog.tsx  review step before an import is saved
└── ipc/
    ├── api.ts            typed wrappers over every Tauri command
    └── types.ts          hand-written mirrors of the Rust model types
```

## The IPC layer

`ipc/api.ts` is the only file that calls `invoke`. Each command gets a typed wrapper, so views never spell command names or argument shapes themselves. Streaming commands (`startRun`, `runTests`, `gitNetwork`) create a Tauri `Channel<ProcessEvent>` and hand the caller's `onEvent` to it; the returned promise resolves when the process exits, so callers keep the UI responsive by not awaiting before rendering. `errorMessage()` normalises the plain-string errors the backend returns.

`ipc/types.ts` mirrors the Rust types by hand — see [the IPC contract](ipc-contract.md) for how drift is caught.

## Components worth knowing

- **OutputConsole** wraps xterm.js (fit, web-links, and search addons) behind a `ConsoleHandle` (`write` / `clear` / `handle(event)`) exposed via `useImperativeHandle`. A real terminal matters because runners redraw progress with bare `\r` and ANSI escapes — the backend deliberately preserves those ([core crate](core-crate.md#process)), and xterm renders them faithfully. On top: URLs open in the system browser, unstyled severity markers are coloured client-side, Ctrl+F opens a find/filter bar (severity threshold + text, rebuilt from a raw-text tail of the output), selection copies, and the right-click menu offers Copy all / Copy diagnostics (command line + exit + last 100 lines). Panes hosting a terminal must be `overflow: hidden` — an outer scrollbar fights the fit addon.
- **DiffView** builds on CodeMirror 6's merge package with per-language syntax highlighting (JS/TS, JSON, CSS, HTML, Python, Rust, XML, C++): side-by-side `MergeView` by default (editors auto-size, `.diff-host` scrolls — the revert buttons are positioned in document coordinates) or the unified `unifiedMergeView`. It renders the `FileDiff` hunks from the backend and lets the user select individual changed lines; selections become the `lines: number[]` passed to `git_stage_lines` / `git_revert_lines`. `allChangedIndices` selects a whole file's changes at once.
- **TestTree** renders `TestNode` hierarchies with worst-outcome colouring, duration formatting, expansion state, and combined text + outcome filtering.
- **ConfigEditor** edits a `RunConfig`. Environment variables are typed as `KEY=value` lines and split on the *first* `=` only, so connection strings, base64, and JWTs survive intact.
- **RiderImportDialog** shows the conversion preview — including per-config warnings — and writes nothing until the user confirms ([Rider import](../guides/rider-import.md)).
- **RunConfigMenu** is rendered by `RunView` (which owns selection, favourites and process status) but displayed in the titlebar via `createPortal` into `#run-config-slot` — the state stays in the view without being lifted to `App`.
- **FileEditor** instances stay mounted while hidden — like console sessions — so undo history and unsaved edits survive switching file tabs. Files load via `fs_read_file` and save with Ctrl+S via `fs_write_file`; the editor/console split fraction persists in `localStorage` (`code-basics.editorSplit`).

## Conventions

- Views receive the `Workspace` as a prop from `App` and call back with `onWorkspaceChange` when a command returns an updated one (saving a config, importing, rescanning).
- `App` restores the backend's open workspace on mount (`currentWorkspace`), so a window reload does not lose state; recents live in `localStorage` under `code-basics.recentWorkspaces`.
- `pnpm typecheck` must pass; there are no frontend unit tests — logic that needs testing belongs in `cb-core`.
