# Optional features (plugin picker)

## Goal

Let the user choose which non-core features the app shows. Two features seed the
system: a **SQL console** and **Ask the codebase (Ctrl+/)**. Chosen during
installation, and changeable afterwards from inside the app.

## Acceptance criteria

- A user-global store records which optional features are on.
- The Windows installer offers the choice on its own wizard page.
- Ubuntu ships a `.deb`; its choice falls back to the in-app picker.
- Choices are changeable after install without reinstalling.
- A feature switched off hides its UI; switched on, it comes back.

## The constraint that shapes everything

**One binary ships all the code.** No installer can produce a feature-reduced
build, so an installer checkbox can only write a *preference the app reads at
startup*. The installer page is a front-end onto an ordinary settings store, and
the in-app picker is the surface every platform has — a `.deb` cannot ask the
question at all.

## Scope

- Stage 1 (this work item): the store, the commands, the gate, the in-app picker.
- Stage 2: Ask the codebase — `.memories/features/ask-codebase/`.
- Stages 3-5: SQL console — `.memories/features/sql-console/`.
- Stage 6: the installers (forked NSIS template, `deb.files` seed).

Full plan: `~/.claude/plans/i-want-to-create-linked-prism.md`.
