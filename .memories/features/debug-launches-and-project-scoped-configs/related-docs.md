# Related docs

- `docs/reference/commands.md` — new **Debug launches** section (`start_debug`,
  `stop_debug`, `debug_ids`) and the streaming-commands note.
- `docs/architecture/core-crate.md` — the `dap` section, rewritten: the crate
  owns the pure layers, the Tauri command owns transport and lifecycle.
- `docs/reference/configuration.md` — config schema **version 2** and the
  migration of v1 Rider ids.
- `docs/guides/rider-import.md` — project-scoped ids; ambiguous compound
  members are dropped rather than guessed.
- `docs/getting-started/using-the-app.md` — the Run tab's Debug control.
- `docs/INDEX.md` — regenerated (`node scripts/generate-index.mjs`).
