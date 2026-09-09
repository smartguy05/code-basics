# Todos — terminal double paste and AltGr

## OUTSTANDING manual verification — nobody has run any of these

**None of this is done.** vitest runs in the **node environment with no DOM and
cannot load `.tsx` at all**, so `TerminalView.tsx` is unreachable by the suite.
The tests pin the intent; only `pnpm tauri dev` pins the outcome — and for this
bug the outcome is the whole point, because the mechanism (does
`preventDefault()` suppress WebView2's native paste? does `readText()` resolve?)
cannot be read out of the source tree.

Run in **`pwsh` and again in `cmd`** — `cmd` does not enable bracketed paste, so
it exercises the *unbracketed* branch of the same helper:

- [ ] Copy a **two-line** block; press **Ctrl+V** → it appears **once**, as two
      lines, with **nothing executed** until Enter. *(Before the fix: twice,
      and the unbracketed copy ran line by line.)*
- [ ] **Ctrl+Shift+V** → pastes once.
- [ ] **Shift+Insert** → pastes once.
- [ ] **If either of the two above pastes nothing**, `readText()` or the chord
      mapping is the problem. **Record which**, then take the pre-decided
      fallback in `notes.md` §2: rely on the native paste plus an explicit,
      **tested** narrowing of `terminalKeyAction` to the chords WebView2
      actually delivers — a narrowing, not a hope.
- [ ] Start `ping -t 8.8.8.8`, press **Ctrl+C** → it interrupts. *(The
      regression risk of this whole fix: a blanket `preventDefault()` would eat
      the shell interrupt.)*
- [ ] Select text, **Ctrl+Shift+C** → the clipboard holds it.
- [ ] **Ctrl+Insert** with **no** selection → reaches the shell.
- [ ] Confirm `navigator.clipboard.readText()` resolves at all under WebView2 at
      this origin — check the devtools console for
      `code-basics: clipboard read failed; nothing was pasted`.
- [ ] **AltGr+V types its character** rather than pasting. Needs a layout where
      AltGr+V produces one; **cannot be observed on a US layout**, so it is
      unverified rather than passing.

## Adversarial review

No unresolved should-fix items were filed against this work item's files
(`terminalLogic.ts`, `TerminalView.tsx`). The reviews' should-fix items landed on
`src-tauri/build.rs` (see `.memories/features/help-about-dialog/todos.md`),
`src/components/TerminalPanel.tsx` (see
`.memories/bugs/terminal-rename-and-focus/todos.md`), and the absence of these
memory files — which this folder is the resolution of.

## Deliberately not done

- `OutputConsole.tsx` also uses `attachCustomKeyEventHandler`, and so has the
  same `return false` ≠ `preventDefault` exposure — but it has no `onData` → PTY
  wiring, so a native paste there is **inert**. Left alone deliberately. If it
  ever gains a write path, this bug comes with it.
- No attempt was made to detect AltGr as distinct from a real Ctrl+Alt chord.
  Windows does not distinguish them, so the guard is "Alt means passthrough",
  which costs any future Ctrl+Alt terminal chord. None exists.
