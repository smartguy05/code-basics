# Rider-style UI consolidation

Eleven UI changes requested 2026-09-02, delivered in four phases.

1. Merge Run + Changes into one **Project** tab, icon rail switching Files/Changes,
   toolbar follows the active editor tab. Diffs open as editor tabs beside files.
2. SQL becomes a floating window (replacing its tab), like terminals.
3. Branch dropdown centred in the titlebar.
4. File-type icons in the Run file tree and the Changes files view.
5. Run = Play icon, Stop = Stop icon, both with tooltips.
6. Searchable branch dropdown.
7. Renameable codebase tabs (the top `.ws-tabs` strip).
8. LSP "still loading" message in a bottom-right info bar on the status row.
9. Button + shortcut to expand the tree to, and highlight, the open file.
10. Combine Launch / Running / + Terminal into one split button.
11. Command shortcuts in that menu: named commands, one-shot or persistent
    (persistent = a long-running service), with a headless option.

Decisions taken with the user (2026-09-02):

- Merged tab is named **Project**, switcher is a vertical **icon rail**.
- Main area **keeps the editor**; a diff opens as another editor tab.
- Combined button is a **split** button: body = New Terminal, caret = menu.
- **Persistent = long-running service** (not auto-restart, not "saved").
- Icons come from an **added dependency**, not hand-authored SVG.
- "Rename project tab" means the **codebase tab in the top strip**.
- Tree bar carries **Reveal + Collapse All**.
- SQL **replaces** its tab rather than gaining a pop-out.
- LSP indicator **moves** to the status bar (not duplicated).

Plan file: `~/.claude/plans/enhancements-i-would-smooth-treehouse.md`
