# Feature: App launcher (run arbitrary commands)

## Need

The user sometimes runs other apps from a terminal (a local Redis, a Python
script, `docker compose up`, ngrok). They want one place in the IDE to launch
them, see what is running, and stop or read a running one.

## Why nothing existing covers it

- `RunConfig` has no `program` field; `invocation::build_with` always resolves
  the program through an ecosystem adapter, and `RunKind` is only `App`/`Test`.
  The only escape hatch is a hand-authored `.code-basics/adapters/*.toml`, and
  even that is gated behind a `detect` file.
- `ConfigEditor` derives the ecosystem from a selected project and has no
  program field; RunView's "+ New configuration…" hardcodes `dotnet`.
- `PtySpec.shell` is documented as "the shell (or any program)" and `open_inner`
  honours args + cwd, but `terminal_open` hardcodes `default_shell()` with empty
  args and exposes no parameter. The core can; the app cannot.

## Acceptance criteria

1. A titlebar **Launch** button opens a picker with a command box (cwd defaults
   to the active codebase root) and the remembered commands.
2. Running a command records it in recents; a recent keeps command + cwd and can
   be pinned and renamed.
3. Recents are user-global (one store), grouped **this codebase first, then
   everything else**.
4. A launched app runs headless through the existing `Supervisor` (no stdin) and
   survives a codebase tab switch.
5. Output appears in **one shared floating panel with a tab strip**, one tab per
   launched app; the tab stays after exit showing the exit code.
6. Every live app appears in the Running panel with pid, age, Kill and a **View**
   action that focuses its output tab. The row disappears the moment it exits.

## Out of scope

Interactive stdin (that is the floating terminals), auto-discovery of candidate
commands, and any change to `RunConfig` / `ConfigEditor` / the adapters.
