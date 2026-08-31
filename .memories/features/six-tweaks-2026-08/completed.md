# Completed

## 1. Changes Files view defaults to Tree — done

- `src/views/folderTreeLogic.ts` — added `FilesLayout`, `defaultFilesLayout`
  (absent/garbage ⇒ `tree`, explicit `flat` honoured), and
  `encodeCollapsedFolders`/`decodeCollapsedFolders` (corrupt ⇒ empty set).
- `src/views/ChangesView.tsx` — `loadFilesLayout` delegates to it; the local
  `FilesLayout` type moved out; `collapsedFolders` now seeds from and persists to
  `code-basics.collapsedFolders:<root>`, written from an **effect** so the state
  updater stays pure.
- Tests added in `folderTreeLogic.test.ts`.

## 3. Build configuration is a picker, not a config per configuration — done

- `crates/core/src/adapters/dotnet.rs::configs_for_project` — dropped the
  `for configuration in configurations` loop. One entry per (project, framework),
  defaulted to `debug_configuration`, labelled `App` / `App (net8.0)`.
  **The id keeps its `:run:debug` spelling** so saved favourites still match; the
  orphaned `:run:release` ids need no migration because `config::sort_configs`
  already ignores an id that names nothing.
- `src-tauri/src/commands/run.rs` — `start_run` and `build_project` take an
  optional `build_configuration` override, applied to a clone before
  `invocation::build`. An empty string is ignored (it is a lost picker value, not
  a default, and would emit a bare `-c`).
- `src/components/configLogic.ts` — `buildConfigurationsFor` (empty for a
  non-.NET ecosystem; the default pair only for an unreadable .NET project) and
  `selectedBuildConfiguration` (drops a remembered value the project no longer
  declares). Tests added.
- `src/views/RunView.tsx` — a `<select>` before the `EnvironmentPicker`, inside
  the existing `ecosystem === "dotnet"` guard; choice persisted per config id
  under `code-basics.buildConfigurations:<root>`. The read-only
  ` · Debug` metadata text was removed — it would go stale against the picker.
- Two `workspace.rs` tests asserted the old fanout and were rewritten (not
  deleted): they now assert one entry per project and that
  `Project.configurations` is where the picker reads its options.

## 4. File tree right-click — done

- `crates/core/src/files.rs` — `create_file`, `create_dir`, `rename`, `delete`,
  all through a new `resolve_new` (refuses the empty path, an existing target,
  and anything `resolve` already refuses). 11 new tests.
- `crates/core/src/symbols/index.rs` — new **`remove_file`**. `replace_file`
  deliberately keeps the `files` entry when the file is missing on disk (an
  unreadable file is not a deleted one), so a delete needed its own verb or the
  palette would offer paths that no longer resolve.
- `src-tauri/src/commands/files.rs` — `fs_create_file`, `fs_create_dir`,
  `fs_rename`, `fs_delete`, plus `unindex_moved_path` (the mirror of
  `reindex_saved_file`). **Known limit, documented in the doc comment:** a
  deleted *directory*'s descendants keep their index entries until the next
  rescan, because a prefix sweep cannot tell `src/app` from `src/apple`.
- `src/components/ContextMenu.tsx` — new shared menu (backdrop + panel), adding
  Escape-to-close and edge-clamping that the three hand-rolled copies lack.
- `src/components/fileTreeLogic.ts` + tests — `targetDir`, `validateName`,
  `createPath`, `renamePath`, `isRenameWorthSending`.
- `src/components/FileTree.tsx` — the menu, an inline name box, targeted
  `reloadUnder`, a delete confirmation, and `onPathGone`.
- `src/views/RunView.tsx` — `closePathAndDescendants` closes the editor tabs of a
  deleted path *and its descendants*: the editor saves on a flush timer, so a
  stale tab would recreate the file the user just deleted.

## 5. Stop button dropdown — done

- `src/components/runningLogic.ts` — `sameRoot`, `stopMenuGroups` (grouped by
  kind in a fixed order, this codebase first inside each group, orphans in their
  own trailing group), `stopMenuCount`, `stopRowLabel`. Tests added.
- `src/views/RunView.tsx` — "Stop All" replaced by a `▾` split button that reads
  `api.listRunning()` **on open** (cleared first, so a stale list is never shown)
  and routes each row through `killRunning(killRequest(...))`, which is the only
  call that knows a terminal belongs to the PTY manager. Orphans confirm first.
  "Stop all applications" moved into the menu foot.

## 2. Manual intent grouping — done

- `crates/core/src/intents/user.rs` — `MoveRequest`, `move_edits`,
  `note_id_of_group`, and the private `remove_lines` / `merge_edit`.
  **The reason `upsert` was not enough:** it drops an overlapping note *whole*,
  which is right for a replacement and wrong for a partial move — the lines
  staying behind would lose their note with the ones that left. `move_edits`
  therefore strips only the moved lines, drops a note left empty, rebuilds the
  destination from its own lines plus the moved ones, and gives it the highest
  seq so a line cannot be won back by the note it just left. 17 tests.
- `src-tauri/src/commands/intents.rs` — `move_card_edits`, with the resolution
  in a free `move_request` function (10 tests; a `#[tauri::command]` body is
  untestable, per the crate's rule). It refuses a vanished source, a vanished
  destination, and a move into the card the selection is already in.
  **The subtle part:** moving into an *ordinary* card carries that card's own
  edits over too (`destination_edits`), or the destination's hunks stay with the
  agent and the moved ones become a second card with the same title.
- `src/components/intentPanelLogic.ts` — `moveTargets` (excludes the source and
  any unnamed/ambiguous card, keeps tooling-titled cards because overriding
  those is the point), `moveDescription`, `canMove`. Tests added.
- `src/components/IntentPanel.tsx` — right-click a card (whole card) or one of
  its file rows (that file) opens the shared `ContextMenu`; **New card…** shows
  an inline name box reusing `.intent-edit-prompt`.
- `src/views/ChangesView.tsx` + `src/ipc/api.ts` — `moveCardEdits` wired through.
- `docs/guides/agent-intent-capture.md` — a new "Grouping the cards yourself"
  section, including the retirement consequence (an override is a user record,
  so a commit absorbing its lines archives it).

## 6. Debugging (DAP) — the pure layers only

`crates/core/src/dap/`, 104 tests, no process work. What is there:

- `protocol.rs` — the base messages. **`Response.request_seq` carries no
  `rename_all`**: DAP spells that one field in snake case, alone in the base
  protocol, and camel-casing it decodes every response with `request_seq: 0`,
  so every correlation fails and the debugger appears to hang on its first
  request. A test caught it, which is the only reason it is not still there.
- `sequence.rs` — `seq` (minted) versus `request_seq` (only read), an unknown
  and a duplicate response reported rather than dropped, and `needs_reply` —
  a reverse request (`runInTerminal`) blocks the adapter until answered.
- `positions.rs` — the 1-based convention `initialize` declares. **Nothing
  corrects an adapter that ignores it**: the only evidence is a line number
  consistent with both conventions, and a correction is indistinguishable from
  undoing a legitimate move of a breakpoint to the next executable line.
- `breakpoints.rs` — requested vs bound vs *rejected*. Pending and rejected are
  separate states because pending resolves itself and rejected will not; a
  short `setBreakpoints` response applies its prefix and reports how far it got
  rather than zipping a binding onto the wrong line.
- `registry.rs` — verified against this machine: vsdbg lives at
  `~/.vscode/extensions/ms-dotnettools.csharp-<ver>-win32-x64/.debugger/x86_64/vsdbg-ui.exe`.
  Editor order then version (numeric, so `2.140.9` beats `2.9.0`), then
  `netcoredbg` on PATH. `CB_DAP_DOTNET` pins it and never falls through.
- `model.rs` — the six states, mirrored into `src/ipc/types.ts`.

**Framing is not duplicated.** DAP frames exactly as LSP does, so
`lsp::framing` is reused; `lsp::registry`'s `Probe`, `EDITOR_DIRS`,
`CSHARP_EXTENSION_PREFIX` and `parse_extension_version` were made `pub(crate)`
and reused too.

**Node is deliberately unsupported and says so.** The js-debug bundled with VS
Code has no standalone entry point (checked: no `dapDebugServer.js` under
`resources/app/extensions/ms-vscode.js-debug`), and the standalone build serves
DAP over a **TCP port**, which this app has no transport for. `resolve` returns
`NotFound` naming that, rather than inventing a path that fails at spawn.
