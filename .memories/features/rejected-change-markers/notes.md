# Notes

## A marker-grepping hook cannot grep for the bare marker

The guard's first version grepped staged blobs for `AI-REJECTED`. That makes
`reject.rs` (which defines the token), the tests, and the documentation
permanently uncommittable — and if a repo sets `core.hooksPath` to a *tracked*
directory, the hook is itself a committed file containing the token, so it
flags itself.

Two defences, both needed:

- the script assembles the token from two adjacent string literals at runtime
  (`cb_marker="AI-""REJECTED"`), so the file never contains it whole;
- it matches the **head line** shape — token followed by `YYYY-MM-DD` — so a
  file that merely mentions the token is untouched.

Consequence for future edits: **never write a literal marker followed by a real
date into a tracked file.** Documentation examples use `<date>`.

## Anchors are not baseline line numbers

`reject_file` reads the diff *before* reverting, because that is the only moment
the rejected hunks' geometry exists. But a hunk's baseline position is not where
the note goes: a *kept* earlier hunk still has its added lines in the file,
pushing everything below it down. `anchors()` adds the net line change of every
unrejected hunk that starts above the anchor. Without that correction, rejecting
the second of two hunks puts the note in the wrong place — the unit test
`a_kept_earlier_hunk_shifts_the_anchor_by_its_net_line_change` pins it.

## The note becomes part of the next diff

A note is a working-tree change, so it appears in the diff — and therefore
inside some intent group — from the moment it is written. Rejecting the same
place twice replaces the note rather than stacking (`remove_block_at`), but the
group geometry on the second pass includes the note's own lines. Worth
revisiting if re-rejection ever misbehaves; the unit tests cover the text
transformation, not that second-pass geometry.

## `withBusy` clears any message the action set

`ChangesView.withBusy` does `await action(); setError(null)`, so a message set
*inside* the action is wiped on success. Pre-existing: `revertGroup`'s "Nothing
in that group could be reverted." never displays. Not changed here — rejection
routes its feedback back to the panel instead, the way `importHistory` already
does. If that bug is ever fixed, the reject path could be simplified.

## Test harness gotchas hit while building this

- A commit with nothing staged fails with "nothing to commit" on **stdout**,
  and the failure looks like the hook rejecting it. A guard test that deletes
  the note must also make a real change, or it tests nothing.
- `tempfile::tempdir()` paths do not necessarily match `Repo::workdir()`
  byte-for-byte on Windows. Compare hook paths against `repo.workdir()`, not
  against the temp dir.
