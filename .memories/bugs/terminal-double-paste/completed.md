# Completed — terminal double paste and AltGr

## Root causes, as fixed

| Bug | Root cause | Fix |
|---|---|---|
| One `Ctrl+V`, two writes to the PTY — the extra one unbracketed, so a multi-line paste executed line by line | `return false` from xterm's `attachCustomKeyEventHandler` stops only xterm's **key translation** and calls no `preventDefault()`, so the webview still ran its native paste onto xterm's hidden textarea, whose `paste` listener fired `term.onData` | `event.preventDefault()` for every handled chord, **and** route our clipboard read through `term.paste(text)` — the same normalize+bracket helper the native path uses — so there is one paste behaviour instead of two |
| A `Ctrl+Insert` copy could clobber the clipboard with an empty native copy | Same missing `preventDefault()`; `Ctrl+Insert` is a native Copy command racing our async `writeText` | `preventDefault()` on the copy branch too, labelled as separate hardening |
| AltGr+V pasted instead of typing its character | `terminalKeyAction` tested `ctrlKey && key === "v"` with no `altKey` check, and **on Windows AltGr is reported as Ctrl+Alt** | `altKey` added to `TerminalKeyEvent`; `passthrough` returned for anything held with Alt, before any chord is considered |

## Files touched

- `src/components/terminalLogic.ts` — `altKey` on `TerminalKeyEvent` with the
  AltGr reason in its doc comment; the `if (e.altKey) return "passthrough"`
  guard at the top of `terminalKeyAction`; new `TerminalKeyEffect` interface and
  `terminalKeyEffect(action)`, whose doc comment records the invisible half of
  the bug (`return false` ≠ `preventDefault`).
- `src/components/TerminalView.tsx` — executes the effect and decides nothing:
  `if (effect.preventDefault) event.preventDefault();` then the copy/paste
  branches, with `term.paste(text)` replacing `onDataRef.current(text)` and the
  rejection now `console.error`-logged rather than swallowed.
- `src/components/terminalLogic.test.ts` — the four chord/effect cases.

## Verified, not assumed

- The xterm bundle really does only
  `if (this._customKeyEventHandler && !1 === this._customKeyEventHandler(e)) return !1;`
  — no `preventDefault()` anywhere on that path.
- `@xterm/xterm` is **5.5.0**; `paste(data: string): void` is declared at
  `typings/xterm.d.ts:1233`; in the bundle it routes through the identical
  `bracketTextForPaste(prepareTextForTerminal(text), …)` the native path uses.

## Still unverified, and it is the load-bearing one

Whether `navigator.clipboard.readText()` **resolves** under WebView2 at this
origin has never been observed — it needs a running app. Now that the native
path is prevented, a rejection means nothing pastes. The fallback is pre-decided
in `notes.md` §2. See `todos.md`.
