# Work item: Installable quality-gate hook

## Goal
Make the quality-gate Stop hook (added in commit 9fd5dd8 as a hand-written
`.claude/settings.json` + `.claude/hooks/quality-gate.mjs`) installable through
the app the same way the intent-capture hooks are — additive settings.json
merge, `.bak` backup, preview-before-write, repo-vs-user scope, and a setup
panel entry/toggle. Ship `quality-gate.mjs` (+ logic) as a bundled resource
seeded into the target workspace without overwriting user edits.

## Origin
User asked to move CLAUDE.md compliance rules into enforced hooks. The
quality-gate Stop hook was the deterministic "no-Claude" piece:
- runs `pnpm typecheck` (TS/TSX changed) / `cargo fmt --check` (.rs changed), blocks turn on failure
- blocks on unresolved `AI-REJECTED <date>` notes in changed files
- non-blocking reminder when source changed but no `.memories/` touched
- `CB_GATE_FULL=1` opts into `cargo clippy`
- short-circuits on `stop_hook_active`

Currently it works only for THIS repo (committed files); it cannot be installed
into another workspace from the UI. This work item closes that gap.

## Acceptance criteria
- [ ] cb-core installer writes/removes the quality-gate Stop hook entry in
      `.claude/settings.json` additively (never clobbers other hooks), idempotent
      on re-install, with `.bak` backup — reusing the intent installer's
      `apply_writes`/merge machinery.
- [ ] The hook script(s) ship as a bundled resource and are seeded into the
      target `.claude/hooks/` without overwriting user edits (mirror the
      enhancement-defaults seeding).
- [ ] Preview-before-write, like the intent "Enable for repo…" flow.
- [ ] Setup-panel entry/toggle + IPC command(s), registered in lib.rs.
- [ ] Tests-first in cb-core for the merge/seed/uninstall decisions.
- [ ] docs updated (development.md, agent-intent-capture.md or a new guide).
