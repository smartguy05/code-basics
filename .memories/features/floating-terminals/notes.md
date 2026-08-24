# Notes — floating terminals

## PTY gotchas (learned the hard way)

1. **Enter is `\r`, not `\n`.** A PTY's line discipline maps carriage return to
   newline (ICRNL). Writing a bare `\n` to `read`/a shell does NOT complete the
   line — the child waits forever. xterm's `onData` sends `\r` on Enter, which
   is correct; the round-trip test had to send `\r` too.

2. **ConPTY + Git Bash bash blocks at startup on the DSR query.** bash under
   ConPTY emits `ESC[6n` (Device Status Report: "where is the cursor?") and
   **emits nothing else until it gets a cursor-position reply**. A terminal
   emulator answers DSR automatically — xterm.js does — so the real app is fine.
   But a headless test with no emulator hangs. The `pty_roundtrip` test stands
   in for the terminal and replies `ESC[1;1R`. On Linux `sh -c` emits no such
   query and the responder never fires. If a future interactive-terminal bug
   looks like "shell started but shows nothing", suspect an unanswered DSR.

3. **Drop the slave after `spawn_command`.** With the slave still open the
   master never sees EOF when the child exits and the reader thread hangs. We
   destructure `PtyPair { slave, master }` and `drop(slave)` right after spawn.

4. **`std::sync::Mutex`, not tokio.** The reader and waiter threads run on plain
   OS threads (portable-pty's reader is a sync `Read`) with no async runtime, so
   they can't lock a tokio mutex. Every critical section is a quick map op with
   nothing awaited inside, so a std mutex is correct and simpler. `close` is the
   only async method (it awaits `kill_tree_async` off-lock).

## Strip Claude Code session markers from the PTY environment
The app is developed by launching it from inside a Claude Code session, so it
inherits Claude Code's child-session env markers, and the PTY inherits the whole
parent environment. Left in place, a `claude` run in the terminal thinks it is a
nested child: **"Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION
marker"**, and it would reuse the parent's IPC socket. Fix: `pty::open` strips
them before spawning (`shell::is_session_marker`) so a user-opened terminal is a
clean top-level session. Stripped: the whole `CLAUDE_CODE_*` namespace (future-
proof) plus bare `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_EFFORT`, `AI_AGENT`. A
user's own `CLAUDE_*` (e.g. `CLAUDE_API_KEY`) is deliberately kept. Proven by
`pty_roundtrip::a_claude_session_marker_is_stripped_from_the_terminal_environment`
(child prints `MARK[]`, empty). Better than the `CLAUDE_CODE_FORCE_*` override
the warning suggests, since it makes the session genuinely top-level.

## Do NOT reuse OutputConsole for the interactive terminal
`OutputConsole` re-colours (`decorate`), filters by severity, and *rebuilds* the
whole view from a raw buffer. That corrupts any program that redraws its own
screen (Claude Code's TUI, a shell line editor). `TerminalView` writes raw bytes
straight to xterm and forwards keystrokes untouched.

## Pre-existing, unrelated test failure on Windows
`cargo test -p cb-core` reports ONE failure:
`architecture::mermaid_tests::mermaid_id_matches_committed_fixture` — a CRLF-vs-LF
mismatch (`\r\n` on disk vs `\n` serialized). Reproduced with my changes
`git stash -u`'d, so it is a pre-existing issue with the committed fixture on a
Windows (autocrlf) checkout, not caused by this feature. There is no
`.gitattributes`. Left alone: "fixing" it via `UPDATE_FIXTURES=1` would commit
CRLF and break Linux — that is an architecture-tab decision, out of scope here.

## Cross-work-item note
This is a good candidate to promote to root CLAUDE.md if the CRLF fixture issue
bites other work items: the repo has no `.gitattributes`, so any test that reads
a committed text fixture and compares against `\n`-serialized output will fail on
Windows checkouts.
