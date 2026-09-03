# Completed — terminal rename and header focus

## Root causes, as fixed

| Bug | Root cause | Fix |
|---|---|---|
| Rename UI never appeared | The header takes `setPointerCapture`, so `click`/`dblclick` go to the **capturing header**, not the `<strong>` inside it — and a parent event never reaches a child handler | Bail out of `onHeaderPointerDown` before taking capture: `closest("button, strong, input")` |
| Header click did not focus the terminal | `focus()` ran during `pointerdown`; the browser's **default mousedown action then moved focus** to the pressed header | `setTimeout(() => viewRef.current?.focus(), 0)` |
| Escape silently committed the edit | Escape unmounted a **focused** input → `onBlur` → `commitRename(draft)` | `abandoning` ref, set in the Escape branch, checked at the top of `commitRename` |
| A pasted paragraph was truncated silently | The input was uncontrolled with no cap, and `renameTerminal` only refused blank | Controlled `draft` + `maxLength`, and `renameTerminal` now cleans through `normalizeLabel` |

## Files touched

- `src/components/TerminalPanel.tsx` — the pointer-down bail-out and the
  deferred focus (both with the *reason* in the comment, since neither fix looks
  necessary from the code alone); `draft` state + `abandoning` ref;
  `startRename`; a controlled `maxLength` input; a header `onContextMenu`
  opening the shared `ContextMenu` with a **Rename…** item, passed `elevated`.
- `src/components/terminalLogic.ts` — `renameTerminal` now cleans through
  `normalizeLabel` (imported from `workspaceRenameLogic`) and refuses `null`,
  replacing its own `trim()`-only guard.
- `src/components/terminalLogic.test.ts` — the four `renameTerminal` cases,
  including the code-point cap with an astral character.

## Reuse, deliberately

`normalizeLabel` / `MAX_LABEL_LENGTH` come from
`components/workspaceRenameLogic.ts` unchanged. A terminal header title sits
beside other terminals' titles exactly as a tab label sits beside other tabs, so
it wants the same rules — and one rule means one place for it to be wrong.

## Known gap left open

The panel's sibling `api.terminalSetLabel(id, cwd, value.trim())` call still
sends **raw** text, bypassing `normalizeLabel`, so the Running panel row can
show a label the header refused. Recorded as a should-fix in `todos.md`; it is
not fixed here.
