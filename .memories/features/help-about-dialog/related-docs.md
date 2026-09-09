# Related docs

- `docs/reference/commands.md` — the "## About" section (that doc must track
  `generate_handler!` in `src-tauri/src/lib.rs`).
- `docs/architecture/ipc-contract.md` — why `AboutInfo` is key-pinned by hand
  (there is no codegen) and why no field is optional. Note the deviation this
  type makes deliberately: the struct lives in `src-tauri/src/commands/about.rs`,
  not `cb-core`, because it carries no decision the core crate needs to make —
  the same posture as `commands/review.rs::ReviewAgentInfo`, except this one
  *does* carry the key-pinning test that one lacks.
- `docs/INDEX.md` — regenerated; now lists `commands/about.rs`, `about_info`,
  `aboutInfo`, `AboutDialog.tsx` and `aboutLogic.ts`.
- CLAUDE.md, "The Rust<->TS type contract" — the rule this change follows
  (change the Rust type, update its key-pinning test and `types.ts` together).
- CLAUDE.md, "Tests first" — the `*Logic.ts` rule that put every string in
  `aboutLogic.ts` and left `AboutDialog.tsx` an untested rendering shell.
