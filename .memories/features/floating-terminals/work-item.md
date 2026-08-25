# Floating interactive terminals

## Request
Floating terminal windows the user can launch Claude Code inside and interact
with, behaving like the app's existing floating panels (drag, resize, minimize),
and **flashing for attention while minimized when output arrives**.

## Confirmed scope
- Fully interactive **PTY** (real typing, arrow keys, prompts, approvals).
- **General shell** — starts the OS shell; `claude` is just one thing to run.
- **DOM floating panels** modeled on `ReviewPanel` (no native OS windows).
- **Multiple** simultaneous, each its own PTY session.

## Acceptance
- Open a floating terminal with a live shell; typing/history/arrow keys work.
- `claude` runs interactively inside it (TUI renders, prompts answerable).
- Multiple terminals, each independent, cascade-positioned.
- Drag / resize / minimize-to-pill; minimized pill **flashes** on new output.
- Closing kills the PTY child tree (no orphaned claude/node).
- Survives tab switches (hosted at app root).

Plan file: ~/.claude/plans/i-want-to-have-structured-origami.md
Branch: claude/floating-terminals
