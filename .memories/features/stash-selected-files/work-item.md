# Stash selected files from the Changes view

## Ask
Right-click one or more file rows in the Changes view -> "Stash file(s)..." ->
prompt for a message -> create a stash containing ONLY those paths, revert those
paths in index + working tree, leave every other change untouched.

Semantics match `git stash push -m <msg> -u -- <paths>`.

## Confirmed decisions (from the user, 2026-09-01)
- Pure libgit2/git2. NO shelling out to the git CLI.
- Untracked selected files ARE included (third "untracked" stash parent) and removed from disk.
- Staged AND unstaged changes for the selected paths both go in, and both reset to HEAD.
- Multi-file selection supported.
- Prompt for the stash message (window.prompt, matching StashPanel.tsx precedent).

## Acceptance
- Only the selected paths are stashed and reverted.
- Untracked selection round-trips through pop.
- The stash is listed by our own stash_list AND readable by the real `git` binary.
- The first stash in a fresh repo is visible (see notes.md: reference_ensure_log).
