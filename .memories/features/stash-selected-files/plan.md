# Plan — stash selected files

Delivered as planned. The full approved plan (context, libgit2 mechanics,
verification) is at:
`~/.claude/plans/i-would-like-a-shiny-elephant.md`

Three corrections were made during implementation, each verified against the
libgit2 / git2 sources rather than assumed:

1. **Untracked paths go in the untracked tree only**, not also in the working
   tree. `build_workdir_tree` sets `include_changed` alone, so libgit2 excludes
   them. Including them makes `stash_pop` check the file out twice and leave it
   staged.
2. **`reference_ensure_log` is necessary but not sufficient.** It creates the
   log file, but `git_reference_create` still consults
   `core.logAllRefUpdates`, so under `false` no entry is appended and the stash
   is invisible. The entry is now written explicitly when the ref write did not
   add one — detected by comparing the log length either side, so the normal
   case cannot gain a phantom second stash.
3. **`blob_path` does apply filters** after all — it derives a hintpath for any
   file under the working directory. The claim that it does not was wrong.
   `blob_writer(Some(path))` is still what is used, because it states the hint
   outright instead of relying on that prefix match.
