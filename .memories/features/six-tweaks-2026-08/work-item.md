# Six tweaks (2026-08-30)

Six requested changes, gathered from using the app. Plan file:
`~/.claude/plans/a-few-updates-and-jolly-beacon.md`.

1. Changes tab, Files view — group by directory.
2. Manually group intents, overriding the automatic grouping.
3. One run entry per project plus a Debug/Release dropdown, and a Debug button
   that runs under a debugger.
4. Run tab file panel — right-click to add a file or folder.
5. Stop button — a dropdown of what is running, to choose what to stop.

## Decisions taken with the user

- Files view: it **already had** a List/Tree toggle (`ChangesView.tsx:1092`,
  `folderTreeLogic.ts`, commit `8d2f1b7`). The ask reduced to defaulting to Tree.
  The user will re-test the "renders flat" symptom against a fresh build rather
  than the installed release.
- Intent grouping: move **hunks and whole files** between cards, plus create a
  new card. Overrides persist.
- Run configs: **collapse** the per-build-configuration fanout.
- Debugging: **full in-app DAP**, targeting **.NET and Node together**.
- Stop menu: **everything running**, each stoppable, plus Stop All.
