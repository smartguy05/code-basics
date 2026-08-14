# Collapse and restore the Run tab's console panel

## The ask

"The terminal window shows below the code window. I would like to be able to
minimize/restore this panel so that it can be out of the way when I don't need
it." Plus: remember the state per workspace, and keep drag-to-resize.

## What was already there

Drag-to-resize already existed — `RunView.startSplitDrag`, with the fraction
persisted under a single global `code-basics.editorSplit` key. So the resize half
of the request needed no new behaviour, only re-keying per workspace.

## Acceptance

- A collapse control that leaves a thin header bar, not a panel that vanishes
  with no way back.
- Collapsed/expanded **and** the divider position persist per workspace.
- A running process and its scrollback survive being put away.

## Status

Code complete and green (typecheck, 494 vitest tests). **Not yet verified in the
running app** — specifically whether xterm refits correctly on restore. See
`notes.md`.
