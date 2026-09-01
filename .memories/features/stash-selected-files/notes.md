# Notes — stash selected files

## libgit2 cannot do a path-scoped stash
`git2::Repository::stash_save` takes no pathspec, and neither does libgit2's
`git_stash_save`/`git_stash_save2` (checked git2 0.19 / libgit2 1.8.1 sources in
~/.cargo/registry). So `Repo::stash_paths` builds the stash commit by hand.

## GOTCHA 1 — `refs/stash` is NOT auto-reflogged
`git_refdb_should_write_reflog` (libgit2 src/libgit2/refdb.c) logs only
`refs/heads/*`, `HEAD`, `refs/remotes/*`, `refs/notes/*`, **or a ref that already
has a log**. `refs/stash` is in none of those.

libgit2's own stash therefore calls `git_reference_ensure_log(repo, "refs/stash")`
*before* `git_reference_create` (src/libgit2/stash.c `update_reflog`).

If you skip it, the FIRST stash in a fresh repo writes no reflog entry. Since
`stash_list` is `stash_foreach`, which reads the reflog, the stash is created and
then is **invisible**. Silent, and only reproducible in a repo with no prior stash.

git2 0.19 exposes it as `Repository::reference_ensure_log(&str)`.
Pinned by `the_first_path_stash_in_a_repo_is_listed`.

## GOTCHA 2 — `Index::add_frombuffer`/`add_path` need an owning repository
Building trees uses an in-memory `git2::Index::new()`, which is **bare** (no repo).
`add_frombuffer` and `add_path` both fail on a bare index ("cannot add to an index
without a repository"). Use `repo.blob_path(...)` to create the blob, build an
`IndexEntry` by hand, and call `Index::add(&entry)` — that is pure entry insertion
and works on a bare index. Then `write_tree_to(&repo)`.

## The stash commit shape libgit2 produces (src/libgit2/stash.c)
- `b_commit` = HEAD commit.
- `i_commit` = index tree, parents `[b_commit]`, msg `"index on <branch>: <sha> <subject>\n"`.
- `u_commit` = untracked tree, **no parents**, msg `"untracked files on <branch>: <sha> <subject>\n"`.
  Only created when there are untracked entries.
- `w_commit` = working tree, parents **in order** `[b_commit, i_commit, u_commit?]`
  (3 when untracked exist, else 2). This oid is what `refs/stash` points at and what
  `StashEntry.id` becomes.
- User message becomes `"On <branch>: <message>\n"` — `parse_stash_branch`
  (repo.rs) depends on that exact prefix.
