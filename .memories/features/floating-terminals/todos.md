# TODOs — floating terminals

## Done
- [x] PTY backend (`cb-core/pty`): manager, shell, model + tests
- [x] Integration round-trip test (real PTY echo, exit, close)
- [x] Tauri commands + AppState field + handler registration
- [x] Frontend: TerminalView, TerminalPanel, terminalLogic (+tests)
- [x] App wiring + "+ Terminal" button + CSS
- [x] IPC types + api wrappers
- [x] Docs: commands.md section, INDEX regenerated, docs:check
- [x] Quality gate: cargo fmt, clippy (clean for new code), pnpm test (867),
      pnpm typecheck, cargo test pty + commands::terminal

## Remaining (need the user / a GUI or are expensive)
- [ ] **End-to-end `pnpm tauri dev`** — open a terminal, run `claude` interactively,
      multiple terminals, drag/resize/minimize, flash-on-minimized-output,
      close-kills-tree, survive tab switch. Can't be driven headlessly.
- [ ] **Rust coverage gate** (`cargo llvm-cov … --fail-under-lines 70`) — not run
      yet; creates a ~6GB `target/llvm-cov-target` (delete after). New pure
      modules are well covered; `pty/mod.rs` impure parts covered by the
      integration test; `commands/terminal.rs` bodies are untestable by design
      (documented rule) with the `spec_cwd` decision extracted + tested.
- [ ] Commit — only when the user asks (currently uncommitted on
      `claude/floating-terminals`).

## Possible follow-ups (not requested)
- "New Claude terminal" convenience that auto-runs `claude` on open.
- Per-terminal cwd picker / launch in a specific project dir.
- Remember which terminals were open across app restarts.

## Pre-existing (NOT ours)
- `architecture::mermaid_tests::mermaid_id_matches_committed_fixture` fails on
  this Windows checkout (CRLF fixture). See notes.md.
