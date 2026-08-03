# The frontend (`src/`)

React 19 + TypeScript, built with Vite, rendered inside the native Tauri window via the platform WebView. No router and no state library — the app is a single window with four tabs, and each view owns its own state. Everything the frontend "does" is an `invoke` call into the [Tauri shell](tauri-shell.md).

## Structure

```
src/
├── main.tsx              entry: mounts <App/> plus global + xterm CSS
├── App.tsx               tab bar, workspace open/reopen, recents (localStorage)
├── views/
│   ├── TestsView.tsx     test configs, run / re-run failed, output + result tree
│   ├── RunView.tsx       app configs, launch/cancel, config editor, Rider import
│   ├── ChangesView.tsx   git status, comparison modes, diff with line staging
│   └── HistoryView.tsx   commit log, per-commit diffs, branches, push/pull/fetch
├── components/
│   ├── OutputConsole.tsx xterm.js terminal for streamed ProcessEvents
│   ├── TestTree.tsx      collapsible outcome tree with text/outcome filters
│   ├── DiffView.tsx      CodeMirror-based diff with per-line selection
│   ├── ConfigEditor.tsx  RunConfig form (args, env, cwd, framework, ...)
│   └── RiderImportDialog.tsx  review step before an import is saved
└── ipc/
    ├── api.ts            typed wrappers over every Tauri command
    └── types.ts          hand-written mirrors of the Rust model types
```

## The IPC layer

`ipc/api.ts` is the only file that calls `invoke`. Each command gets a typed wrapper, so views never spell command names or argument shapes themselves. Streaming commands (`startRun`, `runTests`, `gitNetwork`) create a Tauri `Channel<ProcessEvent>` and hand the caller's `onEvent` to it; the returned promise resolves when the process exits, so callers keep the UI responsive by not awaiting before rendering. `errorMessage()` normalises the plain-string errors the backend returns.

`ipc/types.ts` mirrors the Rust types by hand — see [the IPC contract](ipc-contract.md) for how drift is caught.

## Components worth knowing

- **OutputConsole** wraps xterm.js (with the fit addon) behind a `ConsoleHandle` (`write` / `clear` / `handle(event)`) exposed via `useImperativeHandle`. A real terminal matters because runners redraw progress with bare `\r` and ANSI escapes — the backend deliberately preserves those ([core crate](core-crate.md#process)), and xterm renders them faithfully.
- **DiffView** builds on CodeMirror 6's `unifiedMergeView` with per-language syntax highlighting (JS/TS, JSON, CSS, HTML, Python, Rust, XML, C++). It renders the `FileDiff` hunks from the backend and lets the user select individual changed lines; selections become the `lines: number[]` passed to `git_stage_lines` / `git_revert_lines`. `allChangedIndices` selects a whole file's changes at once.
- **TestTree** renders `TestNode` hierarchies with worst-outcome colouring, duration formatting, expansion state, and combined text + outcome filtering.
- **ConfigEditor** edits a `RunConfig`. Environment variables are typed as `KEY=value` lines and split on the *first* `=` only, so connection strings, base64, and JWTs survive intact.
- **RiderImportDialog** shows the conversion preview — including per-config warnings — and writes nothing until the user confirms ([Rider import](../guides/rider-import.md)).

## Conventions

- Views receive the `Workspace` as a prop from `App` and call back with `onWorkspaceChange` when a command returns an updated one (saving a config, importing, rescanning).
- `App` restores the backend's open workspace on mount (`currentWorkspace`), so a window reload does not lose state; recents live in `localStorage` under `code-basics.recentWorkspaces`.
- `pnpm typecheck` must pass; there are no frontend unit tests — logic that needs testing belongs in `cb-core`.
