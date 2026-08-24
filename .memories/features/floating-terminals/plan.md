# Implementation plan (short form)

Full plan: ~/.claude/plans/i-want-to-have-structured-origami.md

## Layers
1. `crates/core/src/pty/` — new PTY spawn path (portable-pty v0.9.0).
   - `shell.rs` (pure): `default_shell()`, `clamp_size()` + tests.
   - `model.rs`: `TerminalEvent` (Output{text}, Exited{code,success}), `PtySpec`.
     camelCase, key-pinning test. NOTE: single merged stream (no stdout/stderr),
     no Started banner.
   - `mod.rs`: `PtyManager` (Arc<Mutex<HashMap<id, PtySession>>>, clone-cheap
     like Supervisor). open/write/resize/close/list/is_open. Blocking reader
     thread (portable-pty reader is sync). Reuse process::Utf8Chunker +
     process::kill_tree for close.
   - `tests/pty_roundtrip.rs`: real PTY echo round-trip, guarded on `sh`.
2. `src-tauri/src/commands/terminal.rs` — terminal_open (Channel out) /
   terminal_write / terminal_resize / terminal_close / terminal_list.
   AppState gets `pty: PtyManager` field. Register in lib.rs. Reuse run.rs
   `forward` pattern for mpsc->Channel.
3. Frontend:
   - `TerminalView.tsx` — thin raw xterm (onData->write, fit->resize, raw write).
     NOT OutputConsole (its decorate/filter/rebuild corrupts a TUI).
   - `TerminalPanel.tsx` — floating panel modeled on ReviewPanel; per-instance
     layout key + cascade; minimize-to-pill + attention flash.
   - `terminalLogic.ts` (+test) — id/title, cascade offset, storage key,
     attention rule (minimized && non-empty).
   - `reviewLayoutLogic.ts` — generalize load/savePanelLayout to take a key arg
     (default preserves "cb.agentPanel.layout").
   - App.tsx host + launch button (titlebar/MenuBar).
   - ipc/types.ts (+TerminalEvent), ipc/api.ts (+wrappers).

## Order (tests-first)
shell.rs + tests -> model + pin test -> mod.rs + roundtrip -> commands ->
frontend logic + tests -> components -> wiring -> docs.
