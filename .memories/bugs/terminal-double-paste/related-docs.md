# Related docs

- **CLAUDE.md**, the floating-terminals paragraph: `components/TerminalView.tsx`
  is "a **raw** xterm wrapper — bytes straight in via `term.write`, keystrokes
  straight out via `onData`, copy/paste through `attachCustomKeyEventHandler` +
  `navigator.clipboard` — deliberately **not** `OutputConsole`". That is the
  design this bug lived inside; the fix keeps it raw and moves only the
  *decision* out.
- **CLAUDE.md**, same paragraph: `components/terminalLogic.ts` "also decides
  copy/paste (`Ctrl+Shift+C`/`Ctrl+Insert` copy, `Ctrl+V`/`Ctrl+Shift+V`/
  `Shift+Insert` paste, `Ctrl+C` stays the shell interrupt)". The chord table is
  documented there; `terminalKeyEffect` is the new sibling and the same rule
  covers it.
- **CLAUDE.md**, "Tests first — not optional" and the `*Logic.ts` rule — the
  reason `terminalKeyEffect` exists at all rather than a `preventDefault()`
  inline in the `.tsx`.
- **CLAUDE.md**, "The frontend logic tests CAN be run without pnpm" — relevant
  if the junction block is active: `terminalLogic.test.ts` is a pure-logic file
  and *is* reachable by the `tsc` + vitest-shim recipe. Mind the shim's inverted
  `makeExpect(actual, negated)` second argument before chasing failures.
- `node_modules/@xterm/xterm/typings/xterm.d.ts:1233` — `paste(data: string)`,
  the API the fix routes through. Version pinned in
  `node_modules/@xterm/xterm/package.json` (5.5.0).
- The plan's §1c "Rejected alternatives" — read before replacing `term.paste`
  with a native-paste-only or split-by-chord approach; both were considered and
  refused for stated reasons.
