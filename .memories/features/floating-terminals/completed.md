# Completed — floating interactive terminals

Branch: `claude/floating-terminals` (off `main` @ 9f7a96a).

## What shipped

Interactive floating terminal windows (drag/resize/minimize-to-pill, flash for
attention while minimized), each a real PTY you can run Claude Code (or any
shell program) inside. Multiple at once, cascade-positioned, survive tab
switches.

## Files

**Backend (`cb-core`)**
- `crates/core/src/pty/mod.rs` — `PtyManager` (clone-cheap `Arc<std::sync::Mutex<HashMap<id, PtySession>>>`, like `Supervisor`). `open`/`write`/`resize`/`close`/`is_open`/`list`. Blocking reader thread (portable-pty reader is sync) + waiter thread that removes the session and emits `Exited`. `close` reuses `process::kill_tree_async` (kills claude/node children). Reuses `process::Utf8Chunker`, `process::resolve_program`. `std::sync::Mutex` on purpose: reader/waiter threads have no async runtime.
- `crates/core/src/pty/shell.rs` (+`shell_tests.rs`) — pure `default_shell()` (win: pwsh→powershell→cmd; unix: $SHELL→bash→sh), `clamp_size` (floor 1×1), `pick_shell`.
- `crates/core/src/pty/model.rs` — `TerminalEvent` (Output{text} / Exited{code,success} / Failed{message}) + `PtySpec`. camelCase, key-pinning tests. **One merged stream — no stdout/stderr split, no Started banner** (differs from ProcessEvent deliberately).
- `crates/core/tests/pty_roundtrip.rs` — real PTY echo round-trip, exit reporting, close, unknown-id. Guarded on `sh`.
- `crates/core/Cargo.toml` — +`portable-pty = "0.9"`. `lib.rs` — +`pub mod pty`.

**Bridge (`src-tauri`)**
- `src-tauri/src/commands/terminal.rs` — `terminal_open`(Channel out, returns uuid id) / `terminal_write` / `terminal_resize` / `terminal_close` / `terminal_list`. Tested pure helper `spec_cwd`. `terminal_open` falls back to home when no workspace.
- `state.rs` — +`pty: PtyManager` field (not per-workspace; not cleared by set_workspace). `lib.rs` — module + 5 handlers. `Cargo.toml` — +`uuid.workspace`.

**Frontend (`src/`)**
- `components/TerminalView.tsx` — thin raw xterm (onData→write, fit→resize, raw `term.write`). NOT OutputConsole (its decorate/filter/rebuild corrupts a TUI). xterm auto-answers DSR so a shell doesn't hang.
- `components/TerminalPanel.tsx` — floating panel modeled on ReviewPanel; opens PTY on mount, streams output→view, minimize-to-pill + attention flash, closes PTY on unmount/close.
- `components/terminalLogic.ts` (+test) — `makeTerminal(seq)`, `cascadeShift`, `outputNeedsAttention`, `TERMINAL_LAYOUT_KEY`.
- `components/reviewLayoutLogic.ts` — `load/savePanelLayout` now take an optional key (default = old agent-panel key, back-compatible).
- `App.tsx` — terminals state + "+ Terminal" titlebar button + host at app root.
- `ipc/types.ts` (+`TerminalEvent`), `ipc/api.ts` (+5 wrappers). `styles.css` (+`.terminal-panel`/`.terminal-body`/`.terminal-host`).

**Docs** — `docs/reference/commands.md` (+Interactive terminals section), `docs/INDEX.md` regenerated.

## Gotchas learned (see notes.md)
- PTY Enter is `\r` not `\n` (line discipline maps it).
- Git Bash bash under ConPTY **blocks at startup** until the DSR cursor query
  (`ESC[6n`) is answered; xterm.js answers automatically, so the real app is
  fine, but the round-trip test must stand in as the responder.

## Verification (all green)
- `cargo test -p cb-core pty` + `--test pty_roundtrip` (4/4), `cargo test -p cb-app commands::terminal` (3/3).
- `cargo fmt`; clippy clean for new code (3 pre-existing warnings untouched).
- `pnpm test` 867/867, `pnpm typecheck` clean, `pnpm docs:index` + `docs:check`.
- FULL `cargo test -p cb-core` + end-to-end `pnpm tauri dev` still TODO (see todos.md).
