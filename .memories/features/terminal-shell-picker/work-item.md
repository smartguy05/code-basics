# Feature: terminal shell picker

Phase 2 of the "Terminal fixes, a shell picker, and Help → About" plan
(`~/.claude/plans/i-have-a-few-partitioned-twilight.md`).

## Why

`terminal_open` already accepted an arbitrary `program`/`args` end to end, but
nothing on the frontend ever supplied one for a plain terminal, and
`default_shell()`'s Windows order is hardcoded `pwsh → powershell → cmd`. There
was no way to say "open a bash instead", and no way to see what is even
available.

**No wire change was needed** — only a new read-only detection command.

## Acceptance criteria (Rust half, 2a–2c)

- `ShellInfo` / `DetectedShells` in `cb-core`, camelCase, key-pinned, mirrored
  by hand in `src/ipc/types.ts`.
- `ShellInfo.program` is the **resolved absolute path**, not the bare name.
- `DetectedShells.default_id` serialises as an explicit `null` — no
  `skip_serializing_if`.
- Detection omits anything it cannot launch; an empty list is a success.
- `list_shells` command with no `State` and no decision in its body.

## Not in this half

2d–2f (the `terminalShellLogic.ts` preference, the Settings page, the one-off
pick in the titlebar menu) belong to the frontend agent.
