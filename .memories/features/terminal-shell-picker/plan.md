# Plan — terminal shell picker

Approved plan: `~/.claude/plans/i-have-a-few-partitioned-twilight.md`,
sections 2a–2f. Phase 2 of three; Phase 1 is the four terminal bugs, Phase 3 is
Help → About.

Split as the plan specifies, because the frontend depends on the wire types:

1. **Rust first** — `pty/model.rs`, `pty/shell.rs`, `commands/terminal.rs`,
   `lib.rs`, `ipc/types.ts`, `ipc/api.ts`, `docs/reference/commands.md`.
   (This half; see `completed.md`.)
2. **Frontend second** — `terminalShellLogic.ts`, `terminalMenuLogic.ts`,
   `SettingsDialog.tsx`, `App.tsx`, `WorkspaceTab.tsx`.

Order within the Rust half, as run: the pure `shell_tests.rs` cases → the two
key-pinning tests in `pty/model.rs` → the `types.ts` mirror in the same change →
the implementation → the command, registration, wrapper and docs.

## Related docs

- `docs/architecture/ipc-contract.md` — names `pty/model.rs` as the pinning site
  for terminal types, and warns about the single-word-field trap (`rename_all`
  is a no-op on `id`/`label`/`program`/`args`, which is why they are pinned
  anyway).
- `docs/reference/commands.md` — must track `generate_handler!` in
  `src-tauri/src/lib.rs`.
- `CLAUDE.md`, "On Windows, a PTY spawn *can* be re-parsed by `cmd.exe`" — why
  `detected_shells` consults `check_batch_argv`.
- `crates/core/src/review.rs::detect_agents` / `is_installed` — the precedent
  for "list what is installed", followed only halfway: the type lives in
  `cb-core` so the key-pinning test is possible, unlike
  `commands/review.rs::ReviewAgentInfo`, which has none.
