# Related docs

- [Agent intent capture](../../../docs/guides/agent-intent-capture.md) — the
  feature's user-facing home. New sections: "Rejecting a change" and "The
  commit guard".
- [Commands](../../../docs/reference/commands.md) — `reject_intent_group`, and
  the note that `enable_intent_capture` now also installs the guard.
- [Configuration](../../../docs/reference/configuration.md) — the `pre-commit`
  hook as part of what enabling capture writes.
- [IPC contract](../../../docs/architecture/ipc-contract.md) — the rules for
  `RejectSummary` crossing to `types.ts`.
- `docs/INDEX.md` — generated; regenerate with `pnpm docs:index`.

## Code the feature leans on

- `crates/core/src/git/repo.rs` — `revert_lines` (the revert half of a
  rejection), `file_diff`, and the new `hooks_dir`.
- `crates/core/src/git/patch.rs` — `Hunk.old_start` / `old_lines` and
  `DiffLine.old_lineno`, which is all `anchors()` has to work from.
- `crates/core/src/intents/providers/instructions.rs` — the marker-bounded
  `CLAUDE.md` / `AGENTS.md` section, which is where the agent is told the
  contract; `refreshed()` keeps it current on re-install.
- `crates/core/src/intents/providers/mod.rs` — `PlannedWrite` / `apply_plan`,
  reused so the hook is previewed and backed up like every other install write.
