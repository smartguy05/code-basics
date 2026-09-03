# Related docs

- **CLAUDE.md**, the floating-terminals paragraph in the `src/` architecture
  section — `TerminalPanel.tsx` is "the draggable/minimize-to-pill shell
  modelled on `ReviewPanel`", which is why the pointer-capture finding is not
  local to terminals.
- **CLAUDE.md**, the same section on `ContextMenu.tsx`: "the shared right-click
  shell (backdrop + panel over the existing `dropdown-*` CSS) … it adds
  Escape-to-close and viewport clamping, which the two remaining hand-rolled
  copies in `OutputConsole`/`BranchMenu` still lack — migrate one when you next
  touch it rather than adding another." The new **Rename…** menu follows that
  ask.
- **CLAUDE.md**, "Tests first — not optional", the `*Logic.ts` rule: "components
  stay untested rendering shells". This is why `renameTerminal`'s hardening is
  tested and none of the panel behaviour is.
- **CLAUDE.md**, the z-index paragraph: `:root` owns
  `--z-panel` / `--z-panel-stack-span` / `--z-notes` / `--z-overlay` and **no
  z-index integer is ever written in TypeScript**. The `elevated` prop on
  `ContextMenu` is how the header menu gets above the panel band without
  breaking that rule.
- **In-tree precedents worth reading before changing any of this:**
  - `src/components/NotesPanel.tsx` — the rename that *did* work, and the
    reason (its tab strip is outside its capturing header).
  - `src/App.tsx`'s codebase-tab rename — the source of the `abandoning` /
    Escape-vs-blur guard and of the controlled-input-with-`maxLength` shape.
  - `src/components/workspaceRenameLogic.ts` — `normalizeLabel` and
    `MAX_LABEL_LENGTH`, reused rather than reimplemented.
  - `TerminalPanel.restore()` — the pre-existing `setTimeout(…, 0)` focus
    deferral, six lines from the bug it explains.
