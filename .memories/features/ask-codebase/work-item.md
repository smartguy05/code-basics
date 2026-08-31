# Ask the codebase (Ctrl+/)

Stage 2 of the optional-features plan (see `.memories/features/optional-features/`).

## Goal

Ctrl+/ opens a small modal with a question box and an agent picker. Submitting
opens a **real PTY terminal** running `claude` or `codex` interactively with the
question already asked, so the user keeps chatting in the live TUI — follow-ups
come free.

## Acceptance criteria

- Ctrl+/ opens the ask modal from anywhere in the foreground workspace…
- …but **not** while focus is in a text-entry surface, because `FileEditor.tsx`
  binds `Mod-/` to `toggleComment` and a terminal binds Ctrl+_ to readline undo.
- The question reaches the agent as **one argv argument**, never typed keystrokes.
- An agent whose CLI is not installed is refused by name, not worked around.
- Gated on `FeatureId::AskCodebase`; when off, the key listener is never
  registered, so the chord returns cleanly to the editor.
- Agent/model choice is shared with the Review panel (`cb.agentPanel` prefs).

## Why argv, not keystrokes

`PtyManager` spawns via `CommandBuilder` with args directly; typing a multi-line
question into a TUI would submit at the first `\r`; and `TerminalPanel` resolves
its session id asynchronously, so early writes get dropped. All three point the
same way. **But see `notes.md` — "no shell to interpret it" is false on Windows.**

## Layers

| Piece | Where |
|---|---|
| Interactive argv | `crates/core/src/review.rs::agent_args_interactive` |
| Command wrapper | `src-tauri/src/commands/review.rs::interactive_command` |
| PTY program/args | `src-tauri/src/commands/terminal.rs::spec_program` |
| Pure UI decisions | `src/components/askLogic.ts` |
| Modal | `src/components/AskPanel.tsx` |
| Terminal descriptor | `src/components/terminalLogic.ts::makeAgentTerminal` |
