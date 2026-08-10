# Rejected-change markers

Reviewing grouped changes in the Intent view had two verbs: **Stage group** and
**Revert group**. Revert is silent — the code disappears and the agent that
wrote it learns nothing, so the next turn it writes the same thing again.

Add a third verb, **Reject**: revert the change *and* leave a comment at the
spot saying why it was wrong and asking for it to be done properly, then guard
against that comment reaching a commit.

## Acceptance criteria

- Rejecting a group (or one file's share) reverts it and writes an
  `AI-REJECTED` comment where the code was, indented like the code, one per
  rejected hunk.
- The reason is required; rejecting without one is refused.
- Line-comment languages only. A file with no line comment is reverted and
  reported back as unmarked, never silently.
- Only the working-tree comparison modes can be rejected in.
- The agent is told what the marker means and that it must delete it — in
  `CLAUDE.md` / `AGENTS.md`, via the existing instruction section.
- A `pre-commit` hook refuses a commit whose staged files still carry a note,
  with a documented escape hatch.
- The hook is installed exactly when intent capture is enabled for the repo.

## Decisions taken with the user

- Rejection happens **after the fact, in the Intent view** — not in Claude
  Code's own accept/reject diff prompt.
- **cb-app writes the marker**, atomically with the revert; the user does not
  hand-edit.
- Marker semantics are **"fix it properly, then delete this"**, not
  "never do this again".
- **Line-comment languages only** (the user's call), which is also what makes
  reason text unable to escape its comment.
- Enforcement is a **blocking pre-commit hook**, not a lint rule — the marker
  can land in any language, in any repo, and `git commit` is the one gate they
  all share.
