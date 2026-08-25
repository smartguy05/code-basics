# Feature: Notes / Scratchpad panel

A floating, resizable panel for free-form notes / scratchpad / "prompts for the
future", hosted at app level (survives tab switches) like the Terminal/Review panels.
Minimizes to a thin labeled bar that expands back.

## Decisions (from user)
- **Panel style:** free-floating + resizable; minimize → thin labeled bar (pill mechanism restyled).
- **Storage:** GLOBAL across all workspaces — `%APPDATA%/code-basics/notes.json` (sibling of the prompts library). NOT per-workspace.
- **Structure:** one panel, multiple named notes (tab strip, rename/delete, one active).
- **Send to agent:** run the active note's text as an agent prompt (reuses Review panel).
- **Save as instruction:** write the active note into the Enhancements instruction library as a `.md`, so it appears in Enhancements → Add Instructions.

## Acceptance
- Notes persist globally, same in every workspace.
- Panel drags/resizes/minimizes-to-bar/restores.
- Send to agent streams output in the Review panel.
- Save as instruction produces a `<slug>.md` in the instructions dir.
