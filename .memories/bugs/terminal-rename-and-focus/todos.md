# Todos — terminal rename and header focus

## OUTSTANDING should-fix from the adversarial review — not resolved

- [ ] **`src/components/TerminalPanel.tsx:350` — the rename commits raw text to
      `terminal_set_label`, bypassing the `normalizeLabel` cleaning and refusal
      that the header title goes through, so the Running panel can show a label
      the app refused.**

      Verbatim from the review: `onRename` → `renameTerminal` now cleans through
      `normalizeLabel` (control/bidi → space, whitespace collapsed, `null` for
      control-only), but the sibling call is still
      `api.terminalSetLabel(id, cwd, value.trim())`. `String.prototype.trim`
      does not strip U+0000 or U+202E. Scenario: rename a terminal by pasting a
      control-only string — `normalizeLabel` returns `null`, so the header
      correctly keeps "Terminal 1", but `value.trim()` is non-empty, so the
      running registry is updated and the Running panel row shows the bidi
      string. Milder and more likely: renaming to `build   step` leaves the
      header showing `build step` and the Running panel showing `build   step`.
      Before this change both paths used `.trim()` and could not diverge; the
      panel's own new doc comment claims "the host's `renameTerminal` owns the
      cleaning and the refusal … so one rule decides what a terminal may be
      called, wherever the rename came from", which this line makes untrue.

      Shape of the fix: call `normalizeLabel(value)` once in `commitRename` and
      drive **both** `onRename` and `terminalSetLabel` from that single result,
      skipping the backend call when it is `null`.

## OUTSTANDING manual verification — nobody has run any of these

**These are not done.** vitest runs in the **node environment with no DOM** and
cannot load `.tsx` at all, so `TerminalPanel.tsx` is unreachable by the suite.
Every finding in `notes.md` is verifiable only by `pnpm tauri dev`:

- [ ] Open a terminal, **double-click the title** → an input appears. *This is
      the regression under test — it did nothing at all before.*
- [ ] Type a name, **Enter** → the header updates, the minimized pill updates,
      **and the Running panel row updates too** (`terminal_set_label`).
- [ ] Re-open the edit, change it, press **Escape** → the original name
      survives. (Before the guard, Escape silently committed.)
- [ ] **Right-click the header** → a **Rename…** item appears, and it is drawn
      *above* the terminal, not behind it (the `elevated` z-band).
- [ ] Escape closes that menu, and so does clicking outside it.
- [ ] **Drag the header by an empty area** → the panel still moves.
- [ ] **Drag the header by the title** → the panel does **not** move.
- [ ] **Click the header background** → the caret is in the terminal and typing
      goes straight there. *This is bug 2's regression check.*
- [ ] Click **✕** and **—** → they still work, and neither steals focus into
      the terminal.
- [ ] Paste a long string into the rename box → the field stops at 40 characters
      visibly, rather than truncating after the fact.

## Deliberately not done

- Terminal names are **not persisted across restarts** — terminals do not
  survive a restart and `term-${seq}` restarts at 1, so a remembered name would
  attach to an unrelated terminal.
- The two remaining hand-rolled context menus (`OutputConsole`, `BranchMenu`)
  were **not** migrated to `ContextMenu.tsx`. CLAUDE.md asks that one be
  migrated when next touched; neither was touched here.
