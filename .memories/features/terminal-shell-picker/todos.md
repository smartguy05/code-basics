# Todos — terminal shell picker

## Done

- [x] 2a `ShellInfo`/`DetectedShells` + key-pinning tests + `types.ts` mirror
- [x] 2b `shell_candidates`/`detected_shells`/`default_among`/
      `is_wsl_bash_launcher`/`detect_shells` + 16 tests
- [x] 2c `list_shells` command, `lib.rs` registration, `api.ts` wrapper,
      `docs/reference/commands.md` row, `docs:index`, `docs:check`
- [x] 2d `src/components/terminalShellLogic.ts` + 19 tests
- [x] 2e Settings → Terminal page (the first async-loading Settings surface;
      three distinct states: detecting / none detected / a list; no `preview()`;
      **Reset all** resets it too)
- [x] 2f the one-off pick in `terminalMenuLogic.ts` / `App.tsx` /
      `WorkspaceTab.tsx`, plus `makeTerminal`'s optional `command` parameter

## OUTSTANDING manual verification — nobody has run any of these

**Not done.** vitest runs in the **node environment with no DOM and cannot load
`.tsx` at all**, so `SettingsDialog.tsx`, `App.tsx` and `WorkspaceTab.tsx` are
unreachable by the suite — and the candidate/label table is only meaningful on a
real machine anyway. The smoke test
`detect_shells_describes_this_machine_without_asserting_which_shells_it_has`
checks the invariants (unique ids, every `program` is a real file, `default_id`
names a listed row) and deliberately asserts **nothing** about this machine's
inventory, so what is actually detected here has never been observed — only that
whatever it is, it is internally consistent.

- [ ] What is actually detected on this box, and that each row's tooltip shows a
      sane resolved path.
- [ ] Picking each row **spawns that shell** (check the prompt, not the row).
- [ ] Settings → Terminal shows **"Detecting shells…"** and then the list — i.e.
      the async surface renders both states, not just the settled one.
- [ ] The preference **survives a restart**.
- [ ] Set the preference, **rename the shell's executable**, restart → a
      terminal still opens (on the platform default), `missingShellNotice`
      is shown, and the preference is **not erased**. All three halves matter:
      abstaining at use, saying so, and not deleting a choice over what may be a
      transient absence (broken PATH, a tool mid-upgrade, an unmounted volume).
- [ ] The titlebar menu's shell rows are **disabled with the same title as New
      Terminal** when no codebase is open.
- [ ] The **"No shells detected"** disabled row renders when the list is empty
      (hard to reach on a real Windows box — likely only reviewable by reading
      the code, so record it as reasoned rather than observed).
- [ ] A shell installed **mid-session** appears, because `openTerminalMenu`
      clears and re-reads.

## Adversarial review

No unresolved should-fix items were filed against this work item's files. The
reviews' should-fix items landed on `src-tauri/build.rs` (see
`.memories/features/help-about-dialog/todos.md`), `src/components/TerminalPanel.tsx`
(see `.memories/bugs/terminal-rename-and-focus/todos.md`), and the absence of
the Phase 1 memory folders (now created under `.memories/bugs/`).

## Deliberately deferred

- `args` is empty for every candidate today. The field exists so a shell needing
  one can be added with no wire change; **do not remove it as unused.**
- **No `wsl` row.** Adding one honestly requires `wsl.exe -l -q` — a subprocess
  spawn inside a "list what's here" call. Read `notes.md` before reaching for
  it.
- No per-workspace shell preference. The pref is global (`localStorage`), like
  the launcher and notes stores, on the same reasoning: "the shell I use"
  belongs to the person, not the repository.
