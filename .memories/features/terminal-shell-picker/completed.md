# Completed — terminal shell picker

## Phase 2a–2c: the Rust half and its IPC edges (done)

Tests were written first and watched failing: the whole set first failed to
compile (33 "cannot find function/type"), then against a deliberately naive
`detected_shells` (no dedupe, no batch guard, `is_wsl_bash_launcher` stubbed
`false`) exactly the six rule tests failed — dedupe by path, case-insensitive
dedupe, unique ids, the batch shim, the System32 bash, and the predicate itself.
Then implemented; 54/54 in `pty::` pass.

### Files touched

- `crates/core/src/pty/model.rs` — `ShellInfo` and `DetectedShells` beside
  `PtySpec`, same derives. Plus the two key-pinning tests
  (`shell_info_serialises_with_the_keys_the_ui_reads`,
  `detected_shells_serialises_with_the_keys_the_ui_reads`, the second asserted
  with `default_id: None` so the explicit `null` is pinned).
- `crates/core/src/pty/shell.rs` — `ShellCandidate`, `shell_candidates()`, the
  pure `detected_shells()` (candidates + injected `locate`), the pure
  `default_among()`, `is_wsl_bash_launcher()`, the private `path_key` and
  `locate_on_this_machine`, and the thin impure `detect_shells()`. Also extended
  `pick_shell`'s doc comment with the contrast (see notes.md).
- `crates/core/src/pty/shell_tests.rs` — 16 new tests.
- `crates/core/src/pty/mod.rs` — re-exports.
- `src-tauri/src/commands/terminal.rs` — `list_shells`, no `State`, no struct,
  no decision in the body.
- `src-tauri/src/lib.rs` — registered after `terminal_set_label`.
- `src/ipc/types.ts` — hand-mirrored `ShellInfo` / `DetectedShells`
  (`defaultId: string | null`, not optional).
- `src/ipc/api.ts` — `listShells()`.
- `docs/reference/commands.md` — the `list_shells` row; `docs/INDEX.md`
  regenerated.

### Gate, as run

`cargo test -p cb-core` 2988+ pass / 0 fail · `cargo fmt --check` clean ·
`cargo clippy --workspace --all-targets` clean · `pnpm typecheck` clean ·
`pnpm test` 1701 pass · `pnpm docs:index` + `pnpm docs:check` pass.

---

## Phase 2d–2f: the frontend half (done)

### Files touched

New:
- `src/components/terminalShellLogic.ts` — `TERMINAL_SHELL_STORAGE_KEY`
  (`code-basics.terminalShell.v1`), `TerminalShellPref`,
  `DEFAULT_TERMINAL_SHELL`, `readTerminalShell`, `loadTerminalShell`,
  `saveTerminalShell`, `resolvePreferredShell`, `missingShellNotice`. Takes
  `storage` as a **parameter** (the `reviewLogic.ts` pattern), so no `src/*.ts`
  global wrapper was needed.
- `src/components/terminalShellLogic.test.ts` — 19 cases.

Modified:
- `src/components/SettingsDialog.tsx` — `Page` gains `"terminal"`, a **Terminal**
  nav button before Keyboard, the buffered `shell` state, and the mount effect
  calling `api.listShells()`. **The first asynchronously loaded surface in this
  dialog** (it is otherwise entirely synchronous `localStorage` reads) — said so
  in a comment. Three states rendered distinctly: detecting / none detected / a
  list. Written in `apply()`, and **Reset all** also resets it.
- `src/components/terminalMenuLogic.ts` / `.test.ts` —
  `{ kind: "newTerminalIn"; shell: ShellInfo }`, `shells` +
  `shellsLoading` on the state, `SHELLS_SECTION = "New terminal in"`, rows
  appended after the four base rows and before Commands (pinned by an ordering
  test).
- `src/App.tsx` — `openTerminalMenu` clears then re-reads the shell list;
  `runTerminalMenuAction` gains `case "newTerminalIn"`.
- `src/components/WorkspaceTab.tsx` — `mintTerminal`, `openTerminal` (detects at
  open time), `openTerminalIn`; `WorkspaceTabHandle.openTerminalIn` registered
  in all three places.
- `src/components/terminalLogic.ts` / `.test.ts` — `makeTerminal` gains an
  **optional fourth** `command` parameter with `args` copied; the existing pinned
  test asserting `makeTerminal(1, 1, "/ws").command === undefined` still passes,
  which is the point of it being optional.

### Decisions worth keeping

- **The Settings page does not `preview()`**, unlike every other control there,
  and says so in a comment: a terminal reads `command` once at mount and a live
  shell cannot be swapped under a running session, so there is nothing to apply
  live and `cancel()` needs no undo.
- **No check-mark on the preferred shell** in the titlebar menu: those rows are
  one-off launches, and marking one would suggest clicking it changes the
  default.
- **Detected-empty is a disabled row with a reason**, not an omitted section —
  the `Project.unreadable` posture, deliberately unlike the switched-off-plugin
  rule, because "no shell found on this machine" is a machine fact the user may
  need to act on and a silently missing section is indistinguishable from a bug.
- A failed detection at open time **must not stop a terminal opening** (the user
  asked for a terminal) and must not silently claim the preference was honoured
  — hence `mintTerminal(null)` plus a `console.error`.
