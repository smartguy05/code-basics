# Completed — stash selected files from the Changes view

Date: 2026-09-01

## What shipped

Right-click one or more file rows in the Changes view -> "Stash file…" /
"Stash N files…" -> prompts for a message -> a stash holding only those paths.
Those paths revert in index and working tree; every other change stays.

## Files touched

| File | Change |
|---|---|
| `crates/core/src/git/repo.rs` | `stash_paths`, `revert_stashed_paths`, `stash_entry_from_disk`, `stash_log_len`; free fns `normalise_stash_paths`/`normalise_stash_path`/`stash_commit_message`, const `DETACHED_STASH_BRANCH`; refactored `checkout_tree_tolerating_locks` into a generic `tolerating_locks` + new `checkout_head_tolerating_locks` |
| `crates/core/tests/git_operations.rs` | 17 integration tests for the new path |
| `src-tauri/src/commands/git.rs` | `git_stash_paths` command |
| `src-tauri/src/lib.rs` | registered it |
| `src/ipc/api.ts` | `gitStashPaths` wrapper |
| `src/views/changesSelectionLogic.ts` (+ `.test.ts`) | new: all selection + stash-menu decisions, 14 vitest cases |
| `src/views/ChangesView.tsx` | `checked`/`anchor` multi-select state, `orderedPaths`, `selectRow`, `stashSelected`, the menu item, and the migration of the hand-rolled context menu to `components/ContextMenu.tsx` |
| `src/styles.css` | `.row.checked` marker |
| `docs/reference/commands.md`, `docs/INDEX.md` | command table + regenerated index |

`src/ipc/types.ts` needed **no** change: `StashEntry` is unaltered and the new
command takes and returns primitives, so the key-pinning test in `repo.rs`
stands as written.

## Root design points

libgit2 has no usable path-scoped stash, so the stash commit is hand-built to
the shape `stash.c` produces. See `notes.md` for the two invisible traps
(`refs/stash` reflog; `Index::add` vs `add_frombuffer`) and the commit graph.

`stash_save_ext` (libgit2's `git_stash_save_with_opts`) **exists and must not be
used**: its reset step is `git_checkout_tree(FORCE | REMOVE_UNTRACKED)` with no
path filter, so for a pathspec it reverts the whole working tree and deletes
every untracked file while stashing only the named ones. git2's own test asserts
that data loss as if it were correct. The doc comment on `stash_paths` says so,
to stop someone "simplifying" it back.

## Verification

- `cargo test -p cb-core --test git_operations`: 73 passed, 0 failed.
- `cargo test -p cb-core`: 2929 passed, 2 failed — `git::coverage::tests::two_declared_labels_covering_one_file_surface_as_candidates`
  and `sql::driver::sqlite::tests::the_only_call_that_opens_a_handle_is_the_one_under_the_deadline`.
  **Both confirmed pre-existing**: they fail identically in a worktree at HEAD
  with none of this work applied. Not caused by, and not fixed by, this change.
- `cargo fmt --check` clean, `cargo clippy -p cb-core --all-targets` clean.
- `npx tsc --noEmit` clean; `npx vitest run`: 56 files, 1424 tests passed.
- `node scripts/check-docs.mjs` passed; `generate-index.mjs` re-run.
