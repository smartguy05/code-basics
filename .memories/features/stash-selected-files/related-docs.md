# Related docs — stash selected files

- `docs/reference/commands.md` — the `git_stash_paths` row.
- `docs/INDEX.md` — regenerated; lists `stash_paths()` under
  `crates/core/src/git/repo.rs` and the new `src/views/changesSelectionLogic.ts`.
- `CLAUDE.md` — the `git/` layer notes (libgit2-only, stash previewed through
  the ordinary `commit_diff` path) and the rule about migrating the hand-rolled
  context menus.
- Vendored reference read during implementation (not in the repo):
  `~/.cargo/registry/src/*/libgit2-sys-0.17.0+1.8.1/libgit2/src/libgit2/stash.c`
  is the reference implementation for the stash commit graph, and `refdb.c`'s
  `git_refdb_should_write_reflog` is why `refs/stash` needs its log forced.
