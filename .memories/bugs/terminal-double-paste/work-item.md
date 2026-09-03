# Bug: Ctrl+V double-pastes in a floating terminal (and AltGr+V pastes instead of typing)

Bugs 3 and 4 of Phase 1 of the "Terminal fixes, a shell picker, and Help → About"
plan (`~/.claude/plans/i-have-a-few-partitioned-twilight.md`).

## Symptoms as reported

1. **One `Ctrl+V`, two pastes.** Text appears twice in the terminal. Worse than
   duplication: the **two writes differ** — ours was raw, xterm's own is
   CRLF→CR normalized and bracketed. Bracketed paste *is* live (nothing in
   `src/` sets `ignoreBracketedPasteMode`), so the duplicate is the dangerous
   one: a multi-line paste arrives **unbracketed** and the shell executes it
   line by line instead of holding it for Enter.
2. **AltGr+V pastes instead of typing its character.** On a layout where AltGr+V
   produces a character, the terminal ate the keystroke and the character could
   not be typed at all.

## Root causes

1. `TerminalView.tsx` mapped the chord to `action === "paste"`, read the
   clipboard, wrote it via `onDataRef.current(text)`, and returned `false`.
   **Returning `false` from xterm's `attachCustomKeyEventHandler` stops only
   xterm's own key *translation*. It calls no `preventDefault()`.** So the
   webview still performed a **native** paste onto xterm's hidden textarea,
   where xterm's own `paste` listeners fired `term.onData` → a second
   `api.terminalWrite`.
2. `terminalKeyAction` tested `ctrlKey && key === "v"` with no `altKey` check,
   and **on Windows AltGr is reported as Ctrl+Alt**.

## The fix

`event.preventDefault()` **and** route the text through `term.paste(text)`
instead of `onDataRef.current(text)`, so there is one paste path with one
normalization. Plus `altKey` on `TerminalKeyEvent`, with every chord guarded by
`!e.altKey`. The decision was extracted into `terminalLogic.terminalKeyEffect`
so the `preventDefault` is a pinned decision rather than an untested one-liner
in a `.tsx` the suite cannot load.

## Acceptance

- A two-line paste appears **once**, as two lines, with **nothing executed**
  until Enter — in both `pwsh` and `cmd` (cmd does not enable bracketed paste,
  so it exercises the unbracketed branch of the same helper).
- `Ctrl+Shift+V` and `Shift+Insert` paste too.
- `Ctrl+C` still interrupts a running process — a blanket `preventDefault()`
  would have broken this, which is what the passthrough test guards.
- `Ctrl+Shift+C` copies a selection; `Ctrl+Insert` with no selection reaches
  the shell.
- AltGr+V types its character.

## Residual risk, stated

Once the native path is prevented, a `navigator.clipboard.readText()` rejection
means **nothing** pastes, where before the accidental native path covered for
it. See `notes.md` for the fallback that is pre-decided if that happens.
