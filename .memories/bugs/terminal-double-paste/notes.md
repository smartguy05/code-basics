# Notes — terminal double paste, and AltGr

---

## 1. `return false` from `attachCustomKeyEventHandler` does NOT stop the webview

This is the finding. It is invisible in the source, it reads as obviously
sufficient, and it is the whole bug.

Verified in the shipped xterm bundle:

```js
if (this._customKeyEventHandler && !1 === this._customKeyEventHandler(e)) return !1;
```

That is the *only* thing returning `false` does. It stops **xterm's own key
translation** — xterm will not encode the chord and send it to the PTY. It calls
**no `preventDefault()`**, so the keyboard event continues its life:

1. The webview runs its **native editing command** for `Ctrl+V` — a paste onto
   xterm's hidden `<textarea>`, which is the focused element.
2. xterm registers `paste` listeners on **both** that textarea *and* the root
   element. One of them fires, normalizes and brackets the text, and calls
   `term.onData`.
3. `TerminalView`'s `onData` subscription fires `api.terminalWrite`.

Meanwhile our own handler had already read the clipboard and written it. **One
chord, two writes to the PTY.**

**The two writes were not the same text, and that is the dangerous part.** Ours
was raw. xterm's was CRLF→CR normalized and **bracketed**. Bracketed paste is
live in this app (nothing in `src/` sets `ignoreBracketedPasteMode`), so the
duplicate arriving *unbracketed* is the one the shell executes line by line
rather than holding for Enter. A two-line paste therefore ran its first line
immediately.

**Generalise it:** any `attachCustomKeyEventHandler` branch in this codebase
that *performs an action* rather than merely suppressing one must call
`event.preventDefault()` itself. `return false` is not a suppression of the
browser, only of xterm.

### Why `term.paste()` and not our own write

Verified, not assumed:

- `@xterm/xterm` is **5.5.0** (`node_modules/@xterm/xterm/package.json`).
- `paste(data: string): void` is declared at `typings/xterm.d.ts:1233`.
- In the bundle it routes through the **identical**
  `bracketTextForPaste(prepareTextForTerminal(text), …)` helper the native paste
  path uses.

So routing our own clipboard read through `term.paste()` gives the app **one
paste behaviour instead of two** — same normalization, same bracketing, whether
the paste came from our chord handler or from anywhere else. That is the reason
to prefer it over `onDataRef.current(text)`, independently of the duplicate.

### Rejected alternatives, and why

- **Delete the branch and rely on the native paste.** Fixes the duplicate, but
  makes `Ctrl+Shift+V` and `Shift+Insert` depend on WebView2 editing-command
  mappings that cannot be verified from the source tree — trading a loud double
  paste for a silently dead chord.
- **Split by chord** (native for `Ctrl+V`, ours for the others). Requires
  exactly that unverifiable fact, *and* routes one behaviour through two
  mechanisms with different normalization — which is how this bug arose.

---

## 2. Residual risk and the PRE-DECIDED fallback

`navigator.clipboard.readText()` is async and **permission-gated**. Now that the
native path is prevented, a rejection means **nothing pastes**, where before the
accidental native paste covered for it. Hence the logging `catch` —
`console.error("code-basics: clipboard read failed; nothing was pasted", e)` —
rather than a swallowed `.catch(() => {})`: a silently dead chord is the harder
bug to find.

**Whether `readText()` resolves under WebView2 at this origin can only be
established by running the app.** It has not been established.

**If it does not resolve, the fix flips** to: rely on the native paste, plus an
explicit, **tested** narrowing of `terminalKeyAction` to the chords WebView2
actually delivers. A *narrowing*, not a hope — do not leave the branch matching
chords the webview does not deliver and assume they work.

---

## 3. Manual reproduction (CLAUDE.md: "bug fixes begin with a reproduction")

The vitest cases pin the *intent*. This is the reproduction that pins the
*outcome*, and it needs a running app. Both were required by the plan.

**To reproduce the original bug** (on the pre-fix code):

1. `pnpm tauri dev`, open a workspace, open a floating terminal (`+ Terminal`).
2. Copy a **two-line** block to the clipboard, e.g. a line `echo one` then a
   line `echo two`.
3. Click into the terminal and press **Ctrl+V**.
4. **Observed:** the text appears **twice**, and the first (unbracketed) copy is
   **executed line by line without Enter** — `echo one` runs immediately.

**To confirm the fix:** same steps, in **both `pwsh` and `cmd`**. `cmd` matters
because it does not enable bracketed paste, so it exercises the *unbracketed*
branch of the same helper — the paste must still appear once and still not
execute. Expected: **one** copy, two lines, nothing runs until Enter.

**Then the chords that must not regress:**

- `Ctrl+Shift+V` and `Shift+Insert` → also paste once. **If either pastes
  nothing, `readText()` or the chord mapping is the problem — record which, and
  take the fallback in section 2.**
- Start `ping -t 8.8.8.8`, press **Ctrl+C** → it interrupts. (This is what the
  `a_passthrough_never_prevents_the_default` test guards: a blanket
  `preventDefault()` here would swallow the shell interrupt.)
- Select text, **Ctrl+Shift+C** → the clipboard holds it.
- **Ctrl+Insert** with **no** selection → reaches the shell.

---

## 4. On Windows, AltGr is reported as Ctrl+Alt

Bug 4, and it applies far beyond this file.

`terminalKeyAction` tested `ctrlKey && key === "v"`. On a Windows keyboard
layout where **AltGr+V produces a character**, AltGr sets `ctrlKey` *and*
`altKey`, so that test matched, the chord was claimed as a paste, and there was
no way to type the character into a terminal at all.

**The rule: any `ctrlKey`-based chord handler in this app needs an `altKey`
guard.** The fix here adds `altKey` to `TerminalKeyEvent` and returns
`passthrough` for anything held with Alt, before any chord is considered.

This is not terminal-specific — it is true of every `Ctrl+key` binding in
`shortcutLogic.ts` and every hand-rolled key handler in the tree. It is also
untestable by observation on a US layout, which is why it is written down here
rather than left to be rediscovered.

---

## 5. Copy got the same `preventDefault`, for a smaller reason

Labelled separately in the code as hardening rather than as part of the paste
fix: **`Ctrl+Insert` is a native Copy command.** Leaving the default in place
races our async `writeText(selection)` against a native copy of the hidden
textarea's (usually empty) selection — so the clipboard could end up empty.

## 6. The decision had to leave the `.tsx` to be testable at all

vitest runs in the **node environment with no DOM and cannot load `.tsx`**, so
`TerminalView.tsx` is unreachable by the suite. A `preventDefault()` added
inline there would have been the single most important line in the fix and the
one line nothing could check. Hence `terminalLogic.terminalKeyEffect(action) →
{ preventDefault, copy, paste }`: the view now *executes an effect* and decides
nothing, and `preventDefault: action !== "passthrough"` is pinned by tests —
including the passthrough test, which exists to stop someone "simplifying" the
fix into a blanket `preventDefault()` and breaking Ctrl+C.
