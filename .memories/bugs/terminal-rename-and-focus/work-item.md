# Bug: renaming a terminal does nothing, and clicking its header does not focus it

Bugs 1 and 2 of Phase 1 of the "Terminal fixes, a shell picker, and Help → About"
plan (`~/.claude/plans/i-have-a-few-partitioned-twilight.md`).

## Symptoms as reported

1. **Rename is dead.** Double-clicking a floating terminal's header title does
   nothing at all. No input appears. The feature looked like it had never been
   built — and it had: `TerminalPanel.tsx` already had the double-clickable
   `<strong>`, the `.terminal-title-edit` input, `commitRename`, and `onRename`
   wired from `WorkspaceTab.tsx`. Notes' rename works, so this read as a
   "notes-only feature" rather than a bug.
2. **Clicking the terminal header does not put the caret in the terminal.** The
   intent was already in the code — `onHeaderPointerDown` called
   `viewRef.current?.focus()` with a comment saying it exists so you can type
   straight away — and it did not work.

## Root causes (both non-obvious, both in `onHeaderPointerDown`)

1. The header's `onPointerDown` calls `header.setPointerCapture(e.pointerId)`
   for the drag. **While pointer capture is active the browser dispatches
   `click`/`dblclick` to the capturing element** — the header — and a parent
   event never reaches a child handler. So the `<strong>`'s `onDoubleClick`
   could not fire. Notes' rename works because its tab strip is *outside* its
   capturing header.
2. `viewRef.current?.focus()` ran during `pointerdown`, and the browser's
   **default mousedown action then moves focus** to the pressed element,
   blurring xterm's hidden textarea a moment after we focused it.

See `notes.md` — both of these will recur for any control placed inside any
floating panel header in this app.

## Acceptance

- Double-click the header title → an input appears, Enter commits, and the
  header, the minimized pill and the Running panel row all show the new name.
- **Escape abandons** — the original name survives (it did not before: Escape
  unmounted a focused input, firing `onBlur` → `commitRename` with the edited
  value, so Escape silently committed).
- A right-click **Rename…** entry on the header, through the shared
  `ContextMenu.tsx`, because nobody discovers a double-click by looking at it.
- Dragging by an empty area of the header still moves the panel; dragging by
  the title does not.
- Clicking the header background puts the caret in the terminal; clicking ✕ or —
  still works and does not steal focus.
- A title that `normalizeLabel` refuses leaves the existing title untouched.

## Deliberately not done

Terminal names are **not persisted across restarts**. Terminals do not survive a
restart and `term-${seq}` restarts at 1, so a remembered name would attach to an
unrelated terminal — the same reasoning the codebase already states for not
persisting terminal stack order.
