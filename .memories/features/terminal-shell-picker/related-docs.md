# Related docs

- `docs/architecture/ipc-contract.md` — names `crates/core/src/pty/model.rs` as
  the pinning site for terminal types, and warns about the **single-word-field
  trap**: `rename_all = "camelCase"` is a no-op on `id`/`label`/`program`/`args`,
  which is exactly why they are key-pinned anyway. Also the `ArchNode`/`ArchEdge`
  rule that made `default_id` an explicit `null` rather than
  `skip_serializing_if` — "we could not identify the default" must not be
  indistinguishable from a backend that forgot to send one.
- `docs/reference/commands.md` — carries the `list_shells` row, and must track
  `generate_handler!` in `src-tauri/src/lib.rs`.
- `docs/INDEX.md` — regenerated; lists `list_shells`, `listShells`,
  `terminalShellLogic.ts` and the new `pty/shell.rs` exports. **Generated — never
  edit by hand**; `pnpm docs:index`.
- **CLAUDE.md**, "On Windows, a PTY spawn *can* be re-parsed by `cmd.exe`. The
  launcher path cannot." — why `detected_shells` consults
  `pty::argv::check_batch_argv`, and why the asymmetry (a hazardous directory is
  fine for a real `.exe`) is preserved rather than "fixed".
- **CLAUDE.md**, the `pty/` architecture paragraph — `PtyManager`, `spec_program`
  and `default_shell()`, the fallback this feature deliberately leaves in place
  for the abstain case.
- **CLAUDE.md**, the frontend-customization contracts — the Settings dialog and
  the titlebar menus, including `terminalMenuLogic.ts` ("that menu is about
  *running things*") and the *not read yet* vs *none* split that `shellsLoading`
  copies from `shortcutsLoading`.
- **In-tree precedents:**
  - `crates/core/src/review.rs::detect_agents` / `is_installed` — the model for
    "list what is installed", followed only **halfway**:
    `src-tauri/src/commands/review.rs::ReviewAgentInfo` has **no** key-pinning
    test. Do not copy that half; this type lives in `cb-core` so the pin is
    possible.
  - `src/components/reviewLogic.ts` — the `storage`-as-a-parameter pattern
    `terminalShellLogic` follows, which is why it needs no `src/*.ts` global
    wrapper.
  - `WorkspaceTab.openAskTerminal` — the precedent for awaiting a detection
    before minting a terminal descriptor.
