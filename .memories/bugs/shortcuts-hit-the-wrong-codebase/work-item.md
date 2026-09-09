# Run/Changes shortcuts act on the wrong codebase when two are open

Found 2026-09-02 while designing Phase 3. **Pre-existing on `main`** — not
introduced by this work — but Phase 3 widens it, so it is in scope now.

## The mechanism

`executeCommand` (`src/shortcuts.ts:21-28`) walks registered handlers first,
then falls back to:

    document.querySelector(`[data-command="${CSS.escape(id)}"]`)

**First match in document order, and `querySelector` does not care about
`display: none`.**

`run.*` and `changes.*` have **no registered handlers at all** — verified by
grepping `registerCommand` across `src/`: the only registrations are
`agent.ask`, `tree.reveal`, `console.find`, the four `search.*`, and
`view.*`. Every Run and Changes shortcut therefore resolves purely through that
DOM fallback.

Every open codebase renders its own `WorkspaceTab`, and a background one is
merely `hidden={!active}` (`App.tsx`), which is `display: none` — still in the
DOM. `RunView` is inside it and is always mounted. So with two codebases open
there are **two** `[data-command="run.run"]` buttons, and the shortcut fires the
one belonging to whichever workspace is **first in `openWorkspaces`**, not the
one the user is looking at.

The two commands that already register handlers do it correctly, which is the
pattern to copy: `WorkspaceTab.tsx:183` gates registration on `active`, and
`FileTree.tsx:158` returns `false` when `rootRef.current.offsetParent === null`
so a hidden tree declines the command and the next handler gets it.

## Why Phase 3 makes it worse

Today `ChangesView` is mounted only when `active && tab === "changes"`, so
`changes.commit` / `changes.stage` / `changes.unstage` / `change.previous` /
`change.next` have exactly one instance and are safe by accident. The merged
Project tab is always mounted, so those five join `run.*` — including
**`changes.commit`, which would commit in the wrong repository**.

## The intended fix

Choose the first *rendered* match rather than the first match. `offsetParent` is
the wrong test — it is `null` for `position: fixed` elements, and the floating
panels are fixed — so use `getClientRects().length > 0`, which is 0 for
`display: none` and non-zero for fixed elements.

Keep the decision testable: `shortcuts.ts` touches the DOM and vitest runs in
the node environment, so extract the choice (`given these candidates' disabled /
visible flags, which one?`) into a pure helper with tests, and leave the DOM
query as plumbing.

## Fixed 2026-09-02

`shortcutLogic.ts` gained `pickCommandTarget(candidates)` and the
`CommandTargetChoice` union; `shortcuts.ts` now collects **every** match with
`querySelectorAll`, describes each as `{rendered, disabled}`, and acts on the
first *rendered* one.

Four answers, kept distinct rather than collapsed into "no":

- `none` — nothing carries the id.
- `hidden` — matches exist but all are off screen. **Never falls back to one.**
  Acting on a codebase the user cannot see is worse than doing nothing, and
  doing nothing is what the key did before anyone bound it.
- `disabled` — the visible control is refusing. It does **not** look past it for
  an enabled match further down: reaching further would act on some surface
  other than the one on screen, which is the original bug in a new costume.
- `target` — act on this one.

`rendered` is `getClientRects().length > 0`, **not** `offsetParent !== null`.
`offsetParent` is `null` for `position: fixed`, and the floating panels
(terminals, notes, app output) are fixed, so that test would have called half
the app's buttons invisible.

Verified: 10/10 in `shortcutLogic.test.ts`, typecheck unchanged.

Note the supporting comment already in `conflictingCommand`: "All app contexts
can coexist in the DOM (views stay mounted behind panels)". The codebase already
knew this; only the dispatch had not caught up.
