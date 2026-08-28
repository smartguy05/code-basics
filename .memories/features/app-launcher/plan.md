# App launcher: run, see and manage arbitrary commands

## Context

The user sometimes needs to run other apps from a terminal — a local Redis, a
Python script, `docker compose up`, ngrok — and wants one place in the IDE to
launch them, see what's alive, and stop or read a running one.

Today none of that is possible from the app. Verified in the code:

- `RunConfig` (`crates/core/src/model.rs:125`) has **no `program` field**;
  `invocation::build_with` (`crates/core/src/invocation.rs:58`) always resolves
  the program through an ecosystem adapter, and `RunKind` is only `App`/`Test`.
  The only escape hatch is hand-authoring a `.code-basics/adapters/*.toml`
  manifest — and even that is gated behind a `detect` file
  (`adapters/manifest.rs:136`, plus the deliberate test at `:511`).
- `ConfigEditor` has no ecosystem/program field and derives the ecosystem from
  the selected project (`src/components/ConfigEditor.tsx:100-107`); RunView's
  "+ New configuration…" hardcodes `ecosystem: "dotnet"`
  (`src/views/RunView.tsx:1204-1212`).
- `PtySpec.shell` is documented as "the shell (**or any program**)"
  (`crates/core/src/pty/model.rs:20`) and `open_inner` already honours args and
  cwd (`pty/mod.rs:125-127`) — but `terminal_open` hardcodes
  `default_shell()` with empty args and exposes no parameter
  (`src-tauri/src/commands/terminal.rs:67-74`). The core can do this; the app
  cannot.

Outcome wanted: a titlebar **Launch** picker that runs any command line,
remembers what you ran (pin/rename), streams output into one shared tabbed
panel, and surfaces every live app in the existing Running panel with stop and
view-output.

## Decisions (confirmed with the user)

| Question | Decision |
|---|---|
| Where entries live | **One user-global store** `code-basics/launchers.json`, mirroring `notes.rs`. Each entry carries its `cwd`, so the picker groups **This codebase first, then everything else**. Nothing is written into a repo. |
| How it runs | **Background process via the existing `Supervisor`** (headless, no stdin) — not a PTY. |
| Surface | **Titlebar launcher button** + the existing **Running panel** for management. |
| How entries appear | **Running a command adds it to recents** (no separate authoring form). A recent keeps command + cwd, and can be **pinned and renamed**. |
| Output | **One shared floating panel with a tab strip**, one tab per launched app. |
| On exit | The Running-panel row **disappears immediately** (already the store's behaviour — no change needed). The output tab stays, showing the exit code, until closed. |

## Backend — new `cb-core` module `crates/core/src/launcher/`

Modelled directly on `notes.rs` (user-global store) and `running/` (records).

**`model.rs`**
```rust
pub struct Launchable {
    pub id: String,                        // uuid
    pub command: String,                   // the raw command line as typed
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub label: Option<String>,             // user rename; None => show command
    pub shell: bool,                       // run through the default shell
    pub pinned: bool,
    pub last_run_ms: u64,
    pub run_count: u32,
}
pub struct LauncherFile { pub version: u32, pub entries: Vec<Launchable> }
pub struct LauncherGroups { pub this_codebase: Vec<Launchable>, pub global: Vec<Launchable> }
```
`serde(rename_all = "camelCase")`, **no `skip_serializing_if`**, keys pinned by a
test (the `model.rs` convention — see `crates/core/src/model.rs` tests module and
`docs/architecture/ipc-contract.md`).

**`parse.rs` (pure)** — `split_command(&str) -> Result<(String, Vec<String>), String>`:
quote-aware tokeniser; empty input and an unterminated quote are **errors, not
guesses**. It never interprets shell metacharacters itself — a command needing
`|`/`>`/`&&` is run through the shell only because the entry's `shell` flag says
so, and that flag's UI default is decided in `launcherLogic.ts` (one rule, one
place). When `shell` is set, the program is `pty::default_shell()`
(`pty/shell.rs:41`) with `/C` on Windows and `-c` elsewhere.

**`store.rs`** — `launchers_path()` with `CB_LAUNCHERS_PATH` override then
`APPDATA → XDG_CONFIG_HOME → ~/.config → .` (copy `notes::notes_path`,
`notes.rs:72-91`); `load` turns missing/corrupt into an empty file rather than
erroring; `save` is atomic temp+rename with a `.bak` before an empty overwrite,
reusing the `notes::save` shape.

**`recents.rs` (pure)** — the whole policy, fully testable:
- `record_run(file, command, cwd, shell) -> LauncherFile` — dedupe on
  `(command, normalised cwd)`, bump `last_run_ms`/`run_count`, cap **unpinned**
  entries at 30 evicting oldest; pinned entries are never evicted.
- `group(entries, active_root) -> LauncherGroups` — `this_codebase` = entries
  whose `cwd` is inside `active_root` (reuse `symbols::index::relative_to_root`
  for the containment test rather than string prefixing), pinned first then most
  recent; `global` = the rest, same order.
- `rename`, `set_pinned`, `remove`.

**`running/record.rs`** — add `RunKind::External` (`:14`) for these processes,
with the glyph/label added in `runningLogic.ts` and the kill routing in
`src-tauri/src/commands/running.rs:35`: `External` → the **global**
`state.supervisor.cancel(&key)` (same branch as `Review`/`Behavioral`), so a
launched app survives closing the codebase it was started from.

## `src-tauri` — `commands/launcher.rs`

Per the "a command body must not decide anything" rule in CLAUDE.md, every
decision goes in a free function beside the command and is tested:

- `list_launchables(root: Option<String>) -> LauncherGroups` — thin; grouping is
  `launcher::recents::group`.
- `launch_command(command, cwd, shell, label, channel) -> LaunchedApp` —
  `launch_spec_for(...)` (free fn, tested) turns the request into an
  `Invocation { program, args, cwd, env, report: None, coverage: None, warnings }`
  (`model.rs:264`); then `state.supervisor.run_tracked(key, inv, ch,
  RunMeta { root: cwd, label: label.unwrap_or(command), kind: External })`
  (`process/mod.rs:155`), key `ext:<uuid>`; then `recents::record_run` + save.
  Reuse `forward` from `commands/run.rs:20` for the mpsc→`Channel` bridge.
- `stop_command(key)`, `save_launchable(id, label?, pinned?)`,
  `delete_launchable(id)` — these take **no `AppState`** (the store is
  user-global, like `read_notes`/`write_notes`).

Register in `src-tauri/src/lib.rs` `generate_handler!` and update
`docs/reference/commands.md` in the same edit.

## Frontend

- **`src/ipc/types.ts`** — hand-mirror `Launchable`/`LauncherGroups`; extend the
  `RunKind` union with `"external"`.
- **`src/ipc/api.ts`** — `listLaunchables`, `launchCommand(spec, onEvent)`
  (a `Channel<ProcessEvent>` resolving on exit, copying `startRun` at `:203`),
  `stopCommand`, `saveLaunchable`, `deleteLaunchable`.
- **`src/components/LauncherPicker.tsx`** — opened by a new titlebar **Launch**
  button in `src/App.tsx` beside `Notes` / `Running` / `+ Terminal`
  (`App.tsx:298-314`). A command input (cwd defaults to the active workspace
  root, editable; a "run through shell" checkbox pre-ticked by `needsShell`),
  then two labelled groups — *This codebase*, *All commands* — each row with
  Run / ★ pin / ✎ rename / ✕ delete. Enter runs the box, ↑/↓ navigate.
- **`src/components/launcherLogic.ts` + `.test.ts`** — every decision:
  `needsShell(command)`, `displayLabel(entry)` + truncation, filter-as-you-type
  over the two groups, whether Run is enabled, keyboard action table (the
  `searchLogic.ts` one-expression style).
- **`src/components/AppOutputPanel.tsx`** — one shared floating panel mounted at
  **App** level beside `NotesPanel` (`App.tsx:372-388`), so it is not tied to a
  workspace. Tab strip of launched apps (the `NotesPanel` tab-strip pattern),
  each tab an `OutputConsole` (`components/OutputConsole.tsx:46` — gives Ctrl+F,
  copy-all, copy-diagnostics for free) kept mounted with its `ConsoleHandle` in a
  ref map; per-tab Stop / Clear / close. Reuses `reviewLayoutLogic.ts` with its
  own persistence key `cb.launcher.layout`, minimising to a pill like
  `TerminalPanel`. The pane must be `overflow: hidden` (xterm fit addon).
- **`src/components/appOutputLogic.ts` + `.test.ts`** — tab add/close, which tab
  becomes active after a close, tab status from a `ProcessEvent`
  (`running` → `exited <code>` / `failed`), duplicate-title disambiguation,
  whether Stop is enabled.
- **`RunningPanel.tsx` / `runningLogic.ts`** — `kindIcon`/`kindLabel` for
  `external` (⚡ / "App"), and a **View** action on `external` rows that calls a
  new `onFocusOutput(key)` prop; `App` opens the output panel and selects that
  tab. Rows continue to vanish on exit — `RunningStore` already removes on exit
  (`running/store.rs:104`, `process/mod.rs:289-300`), so nothing changes there.

## Explicitly out of scope

Interactive stdin for launched apps (that is what the existing floating
terminals are for), auto-discovery of candidate commands, and any change to
`RunConfig`/`ConfigEditor`/the adapters. This feature does not touch the Run tab.

## Verification

Tests first, per CLAUDE.md — each of these is written failing before the
implementation:

1. `cargo test -p cb-core launcher::` — `parse_tests.rs` (quoting, empty input,
   unterminated quote, shell wrapping per-platform), `store_tests.rs` (missing
   and corrupt file → empty, `CB_LAUNCHERS_PATH` override, atomic save + `.bak`),
   `recents_tests.rs` (dedupe by command+cwd, 30-entry cap, pins immune to
   eviction, grouping by containment in root, ordering), plus the camelCase
   key-pinning test and `RunKind::External` serialisation.
2. `pnpm test` — `launcherLogic.test.ts`, `appOutputLogic.test.ts`, and the
   `runningLogic.test.ts` additions.
3. Full gate: `pnpm typecheck`, `pnpm test`, `pnpm coverage`,
   `cargo test -p cb-core` and `cargo fmt` **from Git Bash** (the `process::`
   tests spawn `sh`), `pnpm docs:index`, `pnpm docs:check`.
4. End-to-end in `pnpm tauri dev`: Launch → run `node -e "setInterval(()=>console.log(Date.now()),1000)"`
   → output streams into the panel; the Running panel shows one ⚡ row with a pid
   and Kill; **View** focuses its tab; Kill removes the row immediately while the
   output tab keeps the exit line. Reopen the picker: the command is in *This
   codebase*; pin and rename it, restart the app, confirm it persisted. Switch
   codebase tabs and confirm the app keeps running and its output keeps
   streaming. Finally run something that needs a shell
   (`echo hi | findstr hi`) with the checkbox ticked, and confirm it is refused
   with a clear message when unticked rather than silently misbehaving.

## Docs and memory

Create `.memories/features/app-launcher/` (`work-item.md`, `plan.md`,
`notes.md`, `todos.md`, `completed.md`) and keep them current as the work
proceeds. Update `docs/reference/commands.md` (must track `generate_handler!`),
`docs/architecture/frontend.md` (the new titlebar button and app-level panel),
the architecture paragraph in `CLAUDE.md` (the `launcher/` module and its
user-global store), and regenerate `docs/INDEX.md`.
