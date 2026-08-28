# Todos

- [ ] **Confirm the scrollbar fix in the running app.** The diagnosis (overlay scrollbar
      means `FitAddon` measures zero width, so `.xterm-screen` covers the thumb) is
      reasoned from the layout, not measured. Check `.xterm-screen` `offsetWidth` against
      `.xterm-viewport` `clientWidth` in the Apps panel, then drag the thumb.
- [ ] Walk the four tab signals in a background codebase — green twice on a minimized
      terminal exit, green/red outlines on build, amber on a bell — and confirm an
      `error` is not masked by a later `done`.
- [ ] Run `pnpm test` and `pnpm typecheck` from a normal shell. Both were verified here
      only through the workarounds in `notes.md`.
- [ ] `historyLogic.test.ts` and `language.test.ts` were not run locally (async tests and
      `vi` mocks). Neither covers anything changed here, but confirm with `pnpm test`.
- [ ] Consider `.sidebar-resizer` overlapping the Run sidebar's scrollbar (see `notes.md`)
      — the same class of bug, deliberately left alone.
- [x] Docs, README and CLAUDE.md updated; `docs:index` regenerated and `docs:check` passes.
