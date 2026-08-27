# Feature: terminal attention, workspace-tab flash, crash-safe notes, terminal copy

Four related UX fixes from one session (2026-08-27).

## Requests (user's words)
1. "no matter what is happening on the terminal, if I minimize it the pill flashes
   like it needs attention when no attention is requested" — pill over-flashes.
2. "I would like notes to survive app crash/restart without specific saving"
   (Sublime hot-exit analogy).
3. "the code-basics tab should flash so I know which project wants attention".
4. "also I can't copy content from the terminal".

## Decisions locked with user
- Attention = terminal **bell (`\x07`) only**. Output / exit / failure do not flash.
- Workspace-tab flash **only for minimized terminals** (matches existing gate).
- Crash-safety scope = **Notes panel only** (not editor file tabs).
- Copy/paste: request #4 arrived mid-turn, not in the approved plan; implemented
  with the standard terminal chords rather than remapping Ctrl+C.
