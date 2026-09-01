# The frontend (`src/`)

React 19 + TypeScript, built with Vite, rendered inside the native Tauri window via the platform WebView. No router and no state library — the app is a single window with six tabs, and each view owns its own state. Everything the frontend "does" is an `invoke` call into the [Tauri shell](tauri-shell.md).

## Structure

```
src/
├── main.tsx              entry: mounts <App/> in an ErrorBoundary; suppresses the
│                         webview context menu (editable fields keep the native one)
├── App.tsx               titlebar (menu bar, branch widget) + workspace-tab strip +
│                         tab row (Run first) + bottom status bar (active
│                         workspace folder + path), workspace open/reopen, recents
│                         (localStorage); Run + Tests stay mounted while hidden so
│                         running processes keep their consoles
├── views/
│   ├── RunView.tsx       file-tree sidebar, editor pane (back/forward file
│   │                     history + pinnable tabs) over per-run console tabs,
│   │                     config dropdown and env picker (both in the toolbar),
│   │                     build actions, and .NET secrets opened as an editor tab
│   │                     (a project picker when the workspace has several)
│   ├── editorNavLogic.ts editor back/forward stack + tab pin/partition (pure)
│   ├── TestsView.tsx     test configs, run / re-run failed, live progress + tree
│   ├── ChangesView.tsx   git status, comparison modes, side-by-side/inline diff;
│   │                     a Files / Intent / Stashes / Erosion toggle over the file
│   │                     list (IntentPanel with staged badges + before/after
│   │                     behavioral evidence, StashPanel stash manager, ErosionPanel
│   │                     rules scan)
│   ├── behavioralPanelLogic.ts  the Intent view's runtime-evidence decisions:
│   │                     per-card badge, score line, delta/status tone, and the
│   │                     evidence under each delta: deltaDetail (console +/- lines,
│   │                     HTTP status/header/body changes, capped at
│   │                     EVIDENCE_LINE_CAP with a "+N more" row), testCaseRows,
│   │                     deltaConfidenceNote, unattributedReason (all tested)
│   ├── ErosionPanel.tsx  the cb_core::erosion scan grouped by category, each flag
│   │                     clicking through to its diff line; a bad-rule warning banner
│   ├── erosionLogic.ts   the erosion decisions (tested): groupByCategory, badgeCount,
│   │                     categoryLabel, category order
│   ├── HistoryView.tsx   commit log, per-commit diffs, a branch folder tree
│   │                     (Local/Remote, multi-select bulk delete), push/pull/fetch
│   ├── historyLogic.ts   its decisions: commit-time formatting, and the
│   │                     sequential best-effort bulk branch delete + its summary
│   ├── ArchitectureView.tsx  the Architecture tab: diagram list (two built-ins +
│   │                     saved files), canvas, editor, save-a-copy
│   ├── architecture/     that tab's parts — see below
│   │   ├── DiagramCanvas.tsx   lazy-imports mermaid, renders, pans/zooms, and
│   │   │                 turns a click on a box into an open-file request
│   │   ├── DiagramEditor.tsx   CodeMirror over one stored diagram, validating
│   │   │                 the Mermaid as you type
│   │   ├── architectureLogic.ts  the diagram list, and the derivation/warning
│   │   │                 labels beside one
│   │   ├── nodeTargets.ts      which file a clicked node opens — a lookup for a
│   │   │                 derived diagram, a strict symbol match for a saved one
│   │   ├── panZoomLogic.ts     the single affine transform, as arithmetic
│   │   ├── viewportLogic.ts    where each diagram was panned/zoomed to, kept
│   │   │                 across the tab's remount
│   │   └── copyLogic.ts        what "save a copy" names the copy, and whether
│   │                     that name is already taken
│   └── InspectView.tsx   the Objects tab: crash dumps, root picker, object tree
│                         over the sidecar's console ([live inspection](live-inspection.md))
├── components/
│   ├── OutputConsole.tsx xterm.js terminal: links, severity colours, search/filter
│   │                     (stream-aware, host-controllable), copy-on-select,
│   │                     context menu with Copy diagnostics
│   ├── TerminalPanel.tsx floating interactive terminal (drag/resize/minimize-to-
│   │                     pill, flash on the bell) over a PTY session
│   ├── TerminalView.tsx  raw xterm wrapper: bytes in, keystrokes out — NOT
│   │                     OutputConsole (its filtering corrupts a live TUI)
│   ├── LauncherPicker.tsx  app launcher overlay: a command box plus the commands
│   │                     you have run before (pin/rename/forget)
│   ├── launcherLogic.ts  pure picker decisions: needsShell, labels, filter,
│   │                     cwd hint, key table (tested; no DOM)
│   ├── AppOutputPanel.tsx  one floating panel with a tab per launched app, each
│   │                     an OutputConsole kept mounted while its tab exists
│   ├── appOutputLogic.ts pure tab ops: add/close, active-after-close, status from
│   │                     a ProcessEvent, Stop enablement, per-tab severity
│   │                     threshold (tested; no DOM)
│   ├── NotesPanel.tsx    floating notes/scratchpad (drag/resize/minimize-to-bar):
│   │                     named-note tabs, autosave, send-to-agent, save-as-instruction
│   ├── notesLogic.ts     pure note ops (add/rename/delete/updateBody), active-tab
│   │                     selection, persistence keys (tested; no DOM)
│   ├── TestTree.tsx      collapsible outcome tree with text/outcome filters
│   ├── ObjectTree.tsx    inspected object graph: one distinct rendering per
│                         ObjectValue, so "null" never looks like "unreadable"
│   ├── DiffView.tsx      CodeMirror diff (side-by-side MergeView or unified),
│   │                     per-line selection, Ctrl+F in-file search (both panes)
│   ├── ConfigEditor.tsx  RunConfig form (project, launch profile dropdown, args,
│   │                     env, cwd, ...; Delete lives in its footer)
│   ├── BranchMenu.tsx    titlebar branch widget: tree, sections, fetch/pull/push,
│   │                     right-click create-from / merge-into, abort-merge
│   ├── treeLogic.ts      the slash-name → folder-tree builder (buildTree /
│   │                     ancestorPaths), shared by BranchMenu and HistoryView
│   ├── MenuBar.tsx       menu bar: File (Open/Rescan/Exit) + Enhancements with
│   │                     fly-out Add Instructions/Run Agent submenus (enhancementsLogic.ts)
│   ├── enhancementsLogic.ts  the Enhancements decisions: add/remove action, badges,
│   │                     empty-state text, run-once click/badge + confirm messages
│   ├── RunConfigMenu.tsx Run-toolbar run-config dropdown: status dots, favourites,
│   │                     reorder, new/import items (rendered inline by RunView)
│   ├── FileTree.tsx      lazy workspace directory tree (one fs_list_dir per expand)
│   ├── FileEditor.tsx    CodeMirror editor over one EditorSource (a workspace file
│   │                     or a project's secrets); Ctrl+S saves, Ctrl+F finds,
│   │                     Ctrl+G goes to a line, Ctrl+/ toggles comments
│   ├── editorSourceLogic.ts  what backs an editor tab — workspace file vs. secrets:
│   │                     id, label, language hint, whether LSP applies (pure)
│   │                     in-file (@codemirror/search), reports dirty,
│   │                     reveals a requested line (clamped, token-guarded), and owns
│   │                     the whole language-server client (didOpen/didChange/didClose,
│   │                     the inline usages rows, both overlays)
│   ├── usagesExtension.ts  the CodeMirror half: a block-widget row above each
│   │                     declaration, a middle-click handler, a viewport notifier —
│   │                     and no decisions
│   ├── usagesLogic.ts    every decision the usages feature makes: row text, grouping,
│   │                     what a middle-click licenses, which anchors to ask about,
│   │                     the cache key, queue pruning, overlay placement
│   ├── LspStatus.tsx     titlebar indicator for what the servers are doing — a poll
│   │                     loop and markup, nothing else
│   ├── lspStatusLogic.ts when to say something about a server, what to say, and how
│   │                     often to look again
│   ├── SearchEverywhere.tsx  the search palette: an overlay over the whole app that
│   │                     finds a file, a symbol or a run configuration; mounted
│   │                     whenever a workspace is, because its window-level key
│   │                     listener is the only way to reach it
│   ├── searchLogic.ts    the palette's decisions: the keybinding table, arrow-key
│   │                     index math, label highlighting, line clamping
│   ├── language.ts       file-extension → CodeMirror language mode, plus the
│   │                     shared syntax-colour theme and bracket matching
│   ├── EnvironmentPicker.tsx  ASPNETCORE_ENVIRONMENT dropdown with in-menu add/remove
│   ├── Sidebar.tsx       the resizable left column (shared stored width)
│   ├── ErrorBoundary.tsx last-resort error screen instead of a blank window
│   └── RiderImportDialog.tsx  review step before an import is saved
└── ipc/
    ├── api.ts            typed wrappers over every Tauri command
    └── types.ts          hand-written mirrors of the Rust model types
```

## The two pieces of cross-view state

Views do not talk to each other. There are exactly two exceptions, and both are the same shape for the same reason, which is worth knowing before a third is added.

### `inspectRequest` — inspect what just crashed

A crashed run and a failed test both want an **Inspect** button, and the view that serves it — `InspectView` — is their sibling, not their child. So `App` holds a single `inspectRequest: InspectRequest | null`: the target, the root, and a `reason` string shown above the resulting capture so the user knows what they clicked. `RunView` and `TestsView` receive `onInspect`, which sets it and switches to the Objects tab in one call; `InspectView` takes it as `pendingRequest`, runs the capture, and calls `onRequestConsumed` so a tab switch does not fire the same capture twice.

Two deliberate details:

- The type is `App`'s own, not the backend's `InspectRequest` from `ipc/types.ts`. Caps and suspension are the backend's business — all a red test knows is what to look at and why.
- It is held **only until consumed**. Nothing accumulates, and no view reads another view's state; the request is a message that happens to be routed through the common parent because that is the only place both siblings can see.

### `openRequest` / `selectRequest` — what the search palette chose

The search palette is an overlay rendered by `App`, not by any tab, because it is reachable from all of them. The Architecture tab raises the same `openRequest` when a box in a diagram is clicked — a second producer, deliberately, rather than a second mechanism. What either one finds is acted on by the Run tab: a file opens in that tab's editor, and a run configuration is selected in that tab's dropdown. So the palette hands its choice to `App`, which holds `openRequest: OpenFileRequest | null` and `selectRequest: SelectConfigRequest | null` and passes them to `RunView` as `pendingOpen` / `pendingSelect`, exactly as `inspectRequest` is passed to `InspectView` — set it, switch tab, and let the consumer call `onOpenConsumed` / `onSelectConsumed`.

**Why this was allowed rather than lifting the editor.** The alternative is moving `openFiles`, `activeFile` and `openFile` up into `App` so the palette can call them directly. That is a pane's worth of state — tab order, dirty files, focus — lifted out of the view that renders it to serve one keystroke, and `App` would then be the place two components had to agree about all of it. Passing a request keeps the editor's state where the editor is, and it reuses an arrangement that already exists and is already understood.

Three details that differ from `inspectRequest`, all forced by the same fact — the Run tab stays mounted while hidden:

- Each request carries a monotonic **`token`**. Choosing a symbol in a file that is *already open* changes neither the path nor the mount, so an equality check on the fields would decide nothing had happened and leave the user on the line they jumped from. A number that only goes up cannot collide with itself. `FileEditor` takes `revealLine` + `revealToken` and reacts to the token; it also remembers the last token it served, so an unrelated re-render cannot replay an old jump and drag the cursor back.
- `RunView` consumes each request **by object identity** in a ref, the way `InspectView` does. Without that guard every process event, tab switch or status tick would re-open the file.
- An action hit **selects** its configuration and never starts it. Starting a process off a fuzzy-matched keystroke is a guess whose cost is a build, a port, or a service talking to something real. `RunView` also checks the id against its own list first — the palette ranks over every configuration, that list is app configurations only, and setting a selection to an id it does not hold would empty the toolbar and look like the app breaking.

### Editor navigation history and pinned tabs

The Run tab's file tabs behave like a browser's, and both behaviours keep their decisions in `views/editorNavLogic.ts` (pure, tested in `editorNavLogic.test.ts`) with `RunView` as the rendering shell — the same rule as everywhere else.

- **Back/forward.** A back/forward stack (`NavHistory` = entries + an index; `pushNav` truncates the forward entries, dedupes the current one, and caps at 50 by evicting from the front) records every navigation into the editor: a file opened from the tree, a file tab clicked, and — crucially — the `pendingOpen` consume, which is where the palette, the architecture diagram **and middle-click go-to-definition** all land. So one recording point covers all three producers. `navBack`/`navForward` move the index and hand back the entry; opening a file does *not* record (it is also how Back reopens a closed file, browser-style), and closing a tab does not record either. The browser side mouse buttons drive it (`navMouseAction` maps `button === 3` → back, `4` → forward) through a window-level, capture-phase `mousedown`+`auxclick` listener — `preventDefault` on mousedown, the same guard the middle-click handlers use, which also suppresses any WebView2 back/forward. It is armed only while the Run tab is on screen (`active` prop from `App`): this view stays mounted when hidden, and moving the active file behind another tab would change what the user sees with no visible cause. The mouse handler reads the history through a ref (`navHistoryRef`, mirrored by `writeNav`) so a listener captured once never goes stale — the `inspectInfoRef`/`writeInspect` idiom.
- **Reveal tokens.** All reveals now draw their token from one `RunView`-owned counter (`revealSeq`) rather than `App`'s `requestToken`, so a history jump and a palette open cannot mint colliding tokens — a reveal only has to *differ* from the last one the editor applied to fire.
- **Pinned tabs.** `pinnedFiles` (an in-memory `Set`, matching `openFiles` — neither persists) partitions the tabs (`partitionTabs`, order-preserving) into a pinned row above the normal strip; with nothing pinned it is byte-for-byte the old single strip. The 📌 control (`togglePin`) sits beside the × on every tab, and closing a file clears its pin.

## The IPC layer

`ipc/api.ts` is the only file that calls `invoke`. Each command gets a typed wrapper, so views never spell command names or argument shapes themselves. Streaming commands (`startRun`, `runTests`, `gitNetwork`) create a Tauri `Channel<ProcessEvent>` and hand the caller's `onEvent` to it; the returned promise resolves when the process exits, so callers keep the UI responsive by not awaiting before rendering. `errorMessage()` normalises the plain-string errors the backend returns.

`ipc/types.ts` mirrors the Rust types by hand — see [the IPC contract](ipc-contract.md) for how drift is caught.

## Components worth knowing

- **OutputConsole** wraps xterm.js (fit, web-links, and search addons) behind a `ConsoleHandle` (`write` / `clear` / `handle(event)`) exposed via `useImperativeHandle`. A real terminal matters because runners redraw progress with bare `\r` and ANSI escapes — the backend deliberately preserves those ([core crate](core-crate.md#process)), and xterm renders them faithfully. On top: URLs open in the system browser, unstyled severity markers are coloured client-side, Ctrl+F opens a find/filter bar (severity threshold + text), selection copies, and the right-click menu offers Copy all / Copy diagnostics (command line + exit + last 100 lines).

  Output is kept as a **line list, not one string** (`ConsoleLine[]` in `consoleLogic.ts`, bounded by line count), because the severity filter has to know which *stream* each line came from and a flat buffer cannot say. Chunks arrive with no relation to line boundaries, so the last entry is always the current unterminated tail and the next chunk continues into it — otherwise a line split across two writes would be classified twice, on fragments neither of which need carry the marker that decides the severity of either. Ranking is `lineSeverity`: **a level marker the tool wrote always wins**, only an unmarked line falls back to its stream (`stderr` ⇒ error), and indented and blank lines are left unranked so `filterConsoleLines` can let a stack trace **inherit** the `fail:` line it hangs off rather than counting frames as failures. The threshold is normally the console's own, behind Ctrl+F; supplying the optional `severity` prop makes it **controlled** by the host instead, which is how the Apps panel's toolbar picker drives it.

  Panes hosting a terminal must be `overflow: hidden` — an outer scrollbar fights the fit addon. The xterm viewport's **own** scrollbar is the exception and is styled explicitly to a real, layout-taking one: WebView2's default is an *overlay* bar that takes no width, so `FitAddon` measures a scrollbar width of zero and lays `.xterm-screen` — a later, positioned sibling — across the strip the bar is painted in, giving a scrollbar you can see and cannot drag.
- **WorkspaceTab** is one open codebase. `App` holds `openWorkspaces` + `activeRoot` and renders one `WorkspaceTab` per root wrapped in `hidden={!active}` — backgrounding is `display:none`, **never an unmount**, so a background codebase's running processes, terminals and language server stay live. Everything per-codebase lives here (the inner Run/Tests/… selection, the agent and before/after panels, the palette, the setup prompt, this codebase's terminals); the global chrome (branch widget, bottom status bar, Notes) stays in `App`, and per-workspace *actions* route back to the foreground tab through a `WorkspaceTabHandle` registered via `onRegister(root, handle)`. Switching awaits `set_active_workspace` before flipping `activeRoot`, so a newly-foregrounded view never queries the previous workspace. The backend shards state per root (`AppState` → a per-slot supervisor, symbol index and LSP session; the review/behavioral supervisor and the PTY manager stay app-global). The pure tab decisions — add/dedupe, which neighbour inherits focus on close, label disambiguation when two share a name — are in the tested `workspaceTabsLogic.ts`.
- **TerminalPanel / TerminalView** are the [floating interactive terminals](../getting-started/using-the-app.md#terminals), hosted **per open codebase** (inside each `WorkspaceTab`, bound to that workspace's root as the PTY cwd) so a running session survives a tab switch. `TerminalPanel` is the draggable, resizable, minimize-to-pill shell modelled on `ReviewPanel`; it reuses `reviewLayoutLogic.ts` (its own persistence key + a cascade offset so several do not stack) and flashes the pill **only on the terminal bell (`\x07`)** while minimized — ordinary output does not flash, since a build or a TUI streams constantly and flashing on any of it would mean nothing — and also decides copy/paste (`Ctrl+Shift+C`/`Ctrl+Insert` copy the selection, `Ctrl+V`/`Ctrl+Shift+V`/`Shift+Insert` paste, `Ctrl+C` stays the shell interrupt); every such decision is in the tested `terminalLogic.ts`. A minimized terminal bubbles two things up to its **workspace tab** when it is not the foreground one: the bell, as long as it is unacknowledged (`onAttentionChange`, live state a restore clears), and a one-shot `onCompleted` when its process exits — see [tab signals](#tab-signals) below. `TerminalView` is a deliberately thin, **raw** xterm wrapper: PTY bytes written straight in, keystrokes straight out via `onData`, copy/paste through `attachCustomKeyEventHandler` + `navigator.clipboard`, and none of `OutputConsole`'s re-colouring/filtering/rebuild, which would corrupt a program that redraws its own screen (Claude Code's TUI, a shell line editor). It relies on xterm answering Device Status Report queries itself, so an interactive shell in a PTY does not hang. Backed by [`cb_core::pty`](core-crate.md#pty) through the `terminal_*` commands.
- **NotesPanel** is the floating [notes / scratchpad](../getting-started/using-the-app.md#notes), hosted at the app level (one global instance, opened from the titlebar **Notes** button) so it survives a tab switch. It reuses `reviewLayoutLogic.ts` — with its own persistence key — for the drag/resize/minimize shell, minimizing to a thin labeled bar rather than a pill. A tab strip switches between several named notes; edits autosave (debounced, with a max-wait cap so continuous typing still lands — `flushDelay`) through the `read_notes`/`write_notes` commands over [`cb_core::notes`](core-crate.md#notes), and the panel flushes any pending write on close **and on `pagehide`**. `notes::save` writes **atomically** (temp file + rename, and a `.bak` before an empty overwrite), so notes survive an app crash or restart without an explicit save. Every decision — create/rename/delete, which tab is active after a delete, the persistence keys — lives in the tested `notesLogic.ts`; the component decides nothing. **Send to agent** opens the shared `ReviewPanel` with the note's text as `initialPromptBody` (an inline prompt in place of a library one — the prompt picker hides), and **Save as instruction** calls `save_note_as_instruction` to add the note to the Enhancements instruction library. Notes are **user-global**, not per-workspace, so unlike the other views the panel reads and writes a file under the user config directory rather than `.code-basics/`.
- **LauncherPicker / AppOutputPanel** are the [app launcher](../getting-started/using-the-app.md#running-other-apps): a titlebar **Launch** button opens an overlay with a command box and the commands you have run before, and **one shared floating panel** holds a tab per launched app. Both are app-level, not per-codebase, because a launched app belongs to no repository — closing the codebase it was started from must not take it down (its process lives in the *global* supervisor, recorded as `RunKind::External`). There is no "add an entry" form: running a command is what remembers it, and an entry can then be pinned or renamed. The toolbar's **severity picker** narrows the active tab's console to *All levels* / *Info+* / *Warn+* / *Errors*, and the threshold lives on the `AppTab` rather than inside the console — per tab, because two services running at once are usually being watched for two different reasons, and on the tab so it survives the panel being hidden. Two lifetime rules the panel exists to keep: every console **stays mounted** while its tab exists (hidden tabs included — `OutputConsole` already skips fitting when it has no `offsetParent`), and a **tab outlives its process**, because the Running panel drops a row the instant a process exits, leaving the tab as the only place the exit code and the output survive. Output that arrives before a console has mounted is buffered in `App` and flushed on registration, so the lines explaining a mistyped command are never lost. Whether the shell checkbox starts ticked is `needsShell` in `launcherLogic.ts`, whose quoting rules mirror the Rust tokeniser exactly — it is only the *default*, since [`cb_core::launcher`](core-crate.md#launcher) refuses an unquoted `|`/`>`/`&&` rather than passing it through as an argument. The Running panel gains a **View** action on those rows only (`hasOutput`), which focuses the app's output tab.
- **BehavioralPanel** streams a [before/after run](../getting-started/using-the-app.md#changes) and then shows what it found, behind a two-tab strip: **Console** (the live run) and **Evidence** (the assembled `BehavioralReport`). The panel switches itself to Evidence when the report lands — that is what the run was for — but the console **stays mounted** behind it rather than being unmounted, the same rule `AppOutputPanel` keeps: the scrollback is the only record of a run that has already finished. Evidence renders the scorecard, the test summary, and then every delta as a collapsible row: `deltaLine` is the always-visible header, `deltaDetail` the rows underneath — the actual console lines that appeared and disappeared, an HTTP delta's status, header and body changes (rendered nowhere before), and a note naming the masking when a comparison was normalised. Each unattributed delta also carries `unattributedReason`, because "0 attributed" is usually a documented abstain rather than a failure: a test case is not mapped to a source file and an `.http` request's handler is not derivable, so those deltas can *never* pin to a card, and a console delta pins only when its lines name exactly one card's files. Rows auto-expand only while the whole report has at most `AUTO_EXPAND_LIMIT` (3) deltas — the one-delta case must not open folded, or the panel reproduces the bug it exists to fix — and each side of a diff is capped at `EVIDENCE_LINE_CAP` (20) lines with a `+N more` row rather than silently truncating. Every one of those decisions is in the tested `behavioralPanelLogic.ts`; the component only renders them. `claimVerifyLogic.describeDelta` reuses the same `deltaDetail`, so the verifying agent and the reader can never be looking at different evidence.
- **ContextMenu** is the shared right-click shell: a click-catching backdrop plus a panel positioned at the pointer, with the items passed in as ordinary `dropdown-item` children so the existing CSS applies unchanged. It exists because the shape had already been hand-rolled three times (`ChangesView`'s file menu, `OutputConsole`, `BranchMenu`) and the file tree and the intent move menu were about to be the fourth and fifth. `ChangesView`'s copy has since been migrated, leaving `OutputConsole` and `BranchMenu`. It adds two things those copies still lack — **Escape closes it**, and it is **kept on screen**, shifted back inside the viewport after measuring rather than being clipped, which is where a right-click in a narrow sidebar most often lands. Measurement needs a frame, so the panel is `visibility: hidden` until then and is never seen in the wrong place first. Migrate one of the two remaining copies when you next touch it rather than adding another.
- **FileTree** is the Run tab's lazy workspace tree — one `fs_list_dir` per directory, expanded on demand. **Right-clicking** a row (or the empty space below, which targets the root) opens New file / New folder / Rename / Delete over the `fs_create_file` / `fs_create_dir` / `fs_rename` / `fs_delete` commands. Every decision is in the tested `fileTreeLogic.ts`: `targetDir` (a folder means *in here*, a file means *beside this*), `validateName` (a nested name is allowed so the folders can be typed in one go, while `..`, an absolute path, an empty segment, a Windows-reserved character and a segment Windows would silently rename by stripping a trailing dot or space are all refused with a message, before the round trip), `createPath` / `renamePath` (which fold `\` to `/` and replace only the last segment, so a name with slashes in it *moves* the file — the only move gesture the tree has), and `isRenameWorthSending`. After a mutation it re-reads the affected directory **and everything loaded beneath it**, because a nested create adds folders several levels down and a delete removes a subtree. A removed path is handed up as `onPathGone`, and `RunView.closePathAndDescendants` closes the editor tabs for it and its descendants: the editor saves on a flush timer, so a tab left pointing at a deleted file would **recreate** it, and a dirty tab is closed with its edits lost because there is nowhere left to save them — which is what the confirmation before the delete is for.
- **The Run toolbar's Stop is a split button.** Clicking it cancels the selected configuration by config id, as before; the caret reads `list_running` **on open** — clearing the previous result first, because a list left over from last time is worse than no list when the user is about to act on it — and routes each row through `killRunning(killRequest(row.record, row.orphan))`, which is the only call that knows a terminal belongs to the PTY manager, a run or build to its codebase's supervisor, and a launched app or agent panel to the global one. The grouping by kind (runs and launched apps first — what people start and forget), the this-codebase-first ordering inside each group, the orphan group at the end with its extra confirmation, and the row label that names the codebase only when it is *not* this one, are all in `runningLogic.ts` beside the Running panel's own helpers.
- **DiffView** builds on CodeMirror 6's merge package with per-language syntax highlighting (JS/TS, JSON, CSS, HTML, Python, Rust, XML, C++): side-by-side `MergeView` by default (editors auto-size, `.diff-host` scrolls — the revert buttons are positioned in document coordinates) or the unified `unifiedMergeView`. It renders the `FileDiff` hunks from the backend and lets the user select individual changed lines; selections become the `lines: number[]` passed to `git_stage_lines` / `git_revert_lines`. `allChangedIndices` selects a whole file's changes at once.
- **IntentPanel** renders the intent cards, and since this change also lets you **regroup them by hand**: right-clicking a card moves all of it, right-clicking one of its file rows moves just that file, into any other card or a new one you name (`move_card_edits`). `intentPanelLogic.moveTargets` decides what may be offered — never the source card, never an unnamed *ambiguous* card (there is nothing to pick it out by), but deliberately including the cards the tooling titled itself, since overriding exactly those is the point — and `moveDescription` states the consequence the UI would otherwise hide: moving into an agent's card makes **both ends** of the move your note, so the recorded reason stops titling it. The "New card…" prompt reuses the intent-editing prompt markup.
- **TestTree** renders `TestNode` hierarchies with worst-outcome colouring, duration formatting, expansion state, and combined text + outcome filtering.
- **ConfigEditor** edits a `RunConfig`. Environment variables are typed as `KEY=value` lines and split on the *first* `=` only, so connection strings, base64, and JWTs survive intact.
- **RiderImportDialog** shows the conversion preview — including per-config warnings — and writes nothing until the user confirms ([Rider import](../guides/rider-import.md)).
- **RunConfigMenu** is rendered **inline by `RunView`** in the Run toolbar, beside the environment picker (it owns the selection, favourites and process status the menu shows). It used to portal into a titlebar slot; now that terminals and the config dropdown live in each workspace's own view, the portal is gone — background `WorkspaceTab`s are simply `hidden`, so nothing stacks.
- **SearchEverywhere** is an overlay with one control: an input, four scope buttons, and a ranked list grouped Files / Symbols / Actions. Every decision it looks like it makes is a call into `searchLogic.ts` — `recogniseShortcut` (the whole keybinding table as one expression), `nextIndex` (wrapping arrow-key movement, and normalising a selection left over from a longer list), `highlightSpans` (`SearchHit.positions` are **character** indices, so the label is decomposed with `Array.from` and never sliced as a raw string), `lineToPos` (clamping, because CodeMirror's `doc.line()` throws out of range and the index is a snapshot). The ranking, the scope filtering and the `Foo:123` line suffix are `cb-core`'s and are not re-implemented here; the raw query text is passed through and the line is read off the hit. Keystrokes are debounced 80 ms and each search carries a sequence number, so a slow reply to an older query cannot land on top of a newer one.
- **FileEditor** instances stay mounted while hidden — like console sessions — so undo history and unsaved edits survive switching file tabs. Each is keyed by an **`EditorSource`** (`editorSourceLogic.ts`), not a bare path: a `workspace` file loads/saves via `fs_read_file`/`fs_write_file` and drives the language server, while a .NET project's **user secrets open as a `secrets.json` tab** — source `secrets`, backed by `read_project_secrets`/`write_project_secrets`, with the whole language-server surface switched off (no usages rows, no `didOpen`). The open-file model keys on `file.id`; the editor/console split fraction persists in `localStorage` (`code-basics.editorSplit`). Because the editor is the whole client of the language-server surface, the `didOpen`/`didClose` pair a server is owed is exactly a workspace tab's lifetime — see [below](#find-usages-and-go-to-definition). Two CodeMirror specifics it pins: `drawSelection()` hides the native caret, so the drawn caret is forced visible with `&.cm-focused .cm-cursor { display: block }` (the base reveal selector does not match in the WebView, and without this there is no caret); and **Ctrl+/** (`toggleComment`) is bound explicitly with `preventDefault` ahead of `defaultKeymap` so the WebView cannot swallow it.

## Tab signals

A background codebase's tab says what happened in it, and the rules are all in the tested
`workspaceTabsLogic.ts` — `TabSignal`, `mergeSignal`, `tabSignalClass` — over the older
`shouldFlashWorkspaceTab`, which still decides the one part that has not changed: **only a
background tab signals**. The active codebase is on screen, its terminals flash their own
pills and its build output is right there, so re-flashing its tab would be noise.

| Source | Signal | Appearance |
|---|---|---|
| build / rebuild / clean exits non-zero | `error` | red outline, pulses until clicked |
| a minimized terminal rings the bell | `attention` | amber outline, pulses until clicked |
| build / rebuild / clean exits zero | `success` | green outline, pulses until clicked |
| a minimized terminal's process exits | `done` | green, pulses twice, then stops |

Two distinctions carry the design.

**Live state versus latched events.** The bell is *state* — a terminal is asking for you
until it is restored — so it is pushed up as a boolean that can go back down, and the
display stays purely derived. The other three are *events*: nothing about the codebase is
still true a second later, so `App` **latches** them in `signalByRoot` and
`activateWorkspace` drops the entry. That explicit clear is what "until clicked" means; a
`success` that survived being looked at would flash again the moment you switched away.

**`mergeSignal` never lets a weaker signal mask a stronger one.** A terminal finishing
after the build broke must not turn the tab from red to green — the build is still broken,
and the tab is the only place that is being said. `done` is the exception that expires on
its own: "it finished" is worth a glance and not an outline that would still be there
tomorrow, so `App` schedules a clear two animation runs later, and only if the signal is
*still* `done`.

The two new events reach `App` through `WorkspaceTab.onSignal(root, signal)`:
`RunView.onBuildResult` (fired from the `exited` case, the only place the exit code is
known — `runBuild`'s `finally` sees a failed build resolve exactly like a successful one)
and `TerminalPanel.onCompleted`. One CSS keyframe serves all four states, with the colours
as custom properties, so there is a single cadence to keep in step rather than four blocks
that can drift apart.

## Find usages and go to definition

Three files, and the split between them is the whole design: `FileEditor.tsx` owns the lifecycle, `usagesExtension.ts` owns the DOM, and **`usagesLogic.ts` owns every decision** — because vitest runs in the node environment, so an `EditorView`, a `WidgetType` and a `MouseEvent` are all unreachable from a test, and anything that decides something inside the other two files is unmeasured by definition. `usagesExtension.ts` also sits outside `vite.config.ts`'s coverage glob (`src/**/*Logic.ts` plus two named exceptions), which is why even its pure helpers — `toneClass`, `actionDetail` — live in the logic module and are re-exported.

**A count is produced only for `outcome === "ready"`.** `cb-core` keeps six `Availability` variants apart and types `total` as `number | null` so that a number which might still change cannot be rendered at all; `usageRowView` is the single place a result becomes text, and for every other outcome the number is simply not in what it returns. So: `starting`/`loading` say the server is not ready, `notConfigured` shows the backend's own install hint, `failed` shows why, `unsupported` says the server *cannot answer* and never the word "none", and `ready` with `total: 0` says "No usages" in its own tone. The same rule governs the goto picker: an empty group licenses "None." only when `message` is `null` (one message covers three lists and names its group in prose), and a picker built from a non-`ready` outcome carries `partialAnswerNote` saying the list may be short.

**Nothing is asked about text the server has not been told about.** The editor holds two version counters — `docVersion`, bumped on every `docChanged`, and `syncedVersion`, the version a `lspChangeDocument` actually delivered — and `requestVisible` refuses to issue anything while they differ or while the debounce is still owed. The guard is in `requestVisible` itself rather than in its callers: there are three, and a query issued in that window is answered about the previous text while being filed under the current version's key, where nothing would ever correct it. Answers are cached under `usageCacheKey(path, anchorId, docVersion)` — joined with `\0`, written as an escape, because a space separator collides (Windows paths and Roslyn anchor ids both contain spaces) — and the whole cache is dropped when the version moves, since no key of an older version can ever be read again.

**A failure is visible as itself.** If the buffer cannot be sent, the rows are taken *down* rather than left showing their idle text above declarations whose line numbers came from the pre-edit document, and the corner badge says what happened. If the anchors come back `starting`/`loading` they are retried every 2 s for 60 attempts — deliberately past `lsp/client.rs`'s 90 s `READINESS_CEILING`, pinned by a test — and when the retries do run out the badge says waiting has *stopped* and how to restart it, because "loading…" with nothing polling behind it is a false claim in the present tense.

**Middle-click is `preventDefault`ed twice.** Once in the extension's `domEventHandlers` and once on the widget row, because CodeMirror's `eventBelongsToEditor` discards an event whose target lies inside a widget with `ignoreEvent() === true` *before* any registered handler runs — and on Windows a middle mousedown that reaches the browser starts the autoscroll cursor. The row forwards nothing: it is not a document position and guessing one from it would aim a goto request at whatever was nearest.

**`LspStatus`** shows nothing when every server is ready, and keeps re-reading slowly (5 s, via `lspPollDelay`) for as long as a file is open rather than stopping when things settle — a server that was ready and then died would otherwise be invisible on this surface until the open-file set happened to change. The collapsed tooltip carries the headline, the detail *and* the hint, since the hint is the only actionable line.

## The Architecture tab

`views/ArchitectureView.tsx` under the tab id `architecture`. **The id matches the label deliberately** — `inspect`/"Objects" is the one place in `TABS` where they differ, and grepping the tree for "Objects" never finds the view that draws it. One such trap is enough.

It is mounted conditionally, like Changes and History: it owns no process and everything it shows is files on disk, so remounting re-reads them. Nothing is cached on the way in either — every selection re-derives, because the inputs are manifests the user edits while the workspace stays open and a stale arrow asserts a dependency that may since have been deleted (`commands/architecture.rs` refuses to cache for the same reason).

**The list is two named built-ins, not a level selector.** "Project map" and "Component map" are two *questions* rather than two magnifications — what is in this repository, versus what the system consists of at run time — and the second drops every `projectReference` arrow and adds data stores that appear nowhere in the first. Saved diagrams follow underneath. The argument, the ordering and the sentence each carries live in `architectureLogic.ts`, where they are tested.

**Three not-ready states, kept apart** — the `InspectView` pattern, for the same reason. Loading is a spinner; an error is whatever the command said through `api.errorMessage`; and **empty is an answer**: `arch_component_graph` returns nothing when no HIGH-strength signal exists and never falls back to the project map, so a repository of class libraries *has* no components, and that is said in those words with the reason rather than rendered as a failure.

**The warnings are part of the diagram.** `ArchGraph.warnings` collects every reference the deriver read and refused to draw — an unresolvable project reference, a workspace membership it would not infer, a relation no edge kind can express, and on the component map every candidate the signal gate turned down. They previously reached a person only as `%%` comments in the Mermaid source, which Mermaid does not render, so the picture looked complete and was not. `DiagramCanvas` counts them in its toolbar and lists them under the picture — beside the diagram they qualify, not in a band at the top of the tab — and the view's job is only to make sure they arrive, including a stored file's own `DiagramFile.warning` for front matter that could not be read. Duplicating the panel would give the same list two places to disagree.

**Mermaid is loaded with `await import("mermaid")`** and must stay in a lazy chunk — the package is large and the other five tabs must not pay for it. Under the app's CSP (`default-src 'self'`, no `unsafe-eval`, no external hosts) it needs `securityLevel: "strict"` and a **top-level** `htmlLabels: false`; the per-diagram `flowchart.htmlLabels` alone is not enough, and class/ER/state diagrams still emit `foreignObject` without it. Mermaid's `click … call` is never used: it requires `securityLevel: "loose"`, i.e. arbitrary callbacks named by a file an agent or a user wrote.

**Pan and zoom are arithmetic, not a dependency** (`panZoomLogic.ts`): a diagram is one SVG element and everything the user does to it is a single affine transform `{ x, y, k }`. `viewportLogic.ts` persists that per diagram, so leaving the tab and coming back does not throw away a zoom into the one corner that mattered.

**Clicking a box opens the file, and usually refuses to** (`nodeTargets.ts`). For a derived diagram the answer is exact — `cb-core` minted the node ids and a project node's id *is* the scan's `Project.id` — so it is a lookup that either finds the path or says no. For a saved diagram the ids are whatever their author typed, so it matches against the symbol index instead, exactly and uniquely or not at all. The view has no editor of its own: it takes `onOpenFile` and passes `App`'s `requestOpenFile` straight through to the one the Run tab already owns.

## Conventions

- Views receive the `Workspace` as a prop from `App` and call back with `onWorkspaceChange` when a command returns an updated one (saving a config, importing, rescanning).
- `App` restores the backend's open workspace on mount (`currentWorkspace`), so a window reload does not lose state; recents live in `localStorage` under `code-basics.recentWorkspaces`.
- `pnpm typecheck` and `pnpm test` must pass. Frontend unit tests (vitest, node environment) cover the pure `*Logic.ts` modules extracted from components — parsing, classification, formatting, index math — with a co-located `.test.ts` per module; components themselves are untested rendering shells. Anything bigger than a display decision still belongs in `cb-core` ([development guide](../guides/development.md)).
