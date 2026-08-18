# Feature: Comprehensive Stash Management (Rider-style)

## Request
A more comprehensive way to stash changes, like Rider: view the different stashes,
name them, preview their contents, and apply/pop/drop individually (+ clear all).

## Decisions (from planning)
- UI lives as a **"Stashes" panel inside the Changes tab** (third segmented option:
  Files / Intent / Stashes). Rider keeps its Stash tab in the Commit tool window.
- First version = **core action set**: Create (named), List, Preview diff, Apply (keep),
  Pop (apply+remove), Drop (one), Clear all.

## Key insight
A git stash IS a commit, so preview needs no new backend: reuse existing
`git_commit_diff(id)` / `git_commit_file_contents(id, path)` with the stash's oid.

## Plan file
C:\Users\AnthonyJames\.claude\plans\i-would-like-a-gentle-avalanche.md
