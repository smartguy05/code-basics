# Plan — terminal double paste and AltGr

Approved plan: `~/.claude/plans/i-have-a-few-partitioned-twilight.md`, sections
**1c** (double paste) and **1d** (AltGr). Phase 1 of three, shipped as one
reviewable change with the rename/focus bugs.

Phase 1 fanned out to three agents that did not share files: (a) rename + header
focus in `TerminalPanel.tsx`, (b) paste + AltGr in `TerminalView.tsx` and
`terminalLogic.ts` — this work item — (c) the memory files. (a) and (b) were
**sequenced rather than parallel** for the one shared file, `terminalLogic.ts`.

## Steps, as approved and as run

1. In `TerminalView`'s `attachCustomKeyEventHandler`, call
   **`event.preventDefault()`** for any handled chord, because `return false`
   stops only xterm's key translation (see `notes.md` §1).
2. Route the clipboard text through **`term.paste(text)`** rather than
   `onDataRef.current(text)`, so the app has one paste behaviour — the same
   `bracketTextForPaste(prepareTextForTerminal(text))` helper the native path
   uses.
3. Log, rather than swallow, a `readText()` rejection: now that the native path
   is prevented, a rejection means nothing pastes at all.
4. Give the **copy** branch the same `preventDefault()`, labelled separately as
   hardening: `Ctrl+Insert` is a native Copy command and would race our
   `writeText`.
5. Add `altKey` to `TerminalKeyEvent` and return `passthrough` for **anything**
   held with Alt, before any chord is considered — AltGr reports as Ctrl+Alt on
   Windows.
6. **Extract the effect** into `terminalLogic.terminalKeyEffect(action)` →
   `{ preventDefault, copy, paste }`, so the `preventDefault` is a pinned
   decision instead of an untested one-liner in a `.tsx` the suite cannot load.
   `TerminalView` then carries the effect out with no decisions of its own.

## Tests, written first, each watched fail for the right reason

In `src/components/terminalLogic.test.ts`, extending the existing chord block:

- `a_paste_chord_prevents_the_default_so_the_webview_cannot_paste_a_second_time`
  — with a comment naming the bug: `return false` stops only xterm's key
  translation.
- `a_copy_chord_prevents_the_default`.
- `a_passthrough_never_prevents_the_default` — Ctrl+C, F5, ordinary keys. **This
  is the guard that stops someone "simplifying" the fix into a blanket
  `preventDefault()`**, which would break Ctrl+C as the shell interrupt.
- `altgr_v_is_a_character_not_a_paste`, and the copy equivalent.

## What the tests do NOT cover, and must not be claimed to

Per the plan, verbatim in spirit: vitest runs in the node environment with no
DOM, and `TerminalView.tsx` is a rendering shell the suite deliberately does not
load. So **whether `preventDefault()` actually suppresses WebView2's native
paste, whether `term.paste` brackets, and whether `readText()` resolves are all
manual checks.** The tests pin the intent; the app run pins the outcome.

Per CLAUDE.md ("bug fixes begin with a reproduction"), the reproduction is
**two** things and needs both: those tests, plus the written manual reproduction
in `notes.md` §3.
