# Plan — terminal rename and header focus

Approved plan: `~/.claude/plans/i-have-a-few-partitioned-twilight.md`, sections
**1a** (rename) and **1b** (header focus). Phase 1 of three; Phase 2 is the
shell picker, Phase 3 is Help → About.

Phase 1 fanned out to three agents that did not share files: (a) rename + header
focus in `TerminalPanel.tsx`, (b) paste + AltGr in `TerminalView.tsx` and
`terminalLogic.ts`, (c) the memory files. (a) and (b) were **sequenced rather
than parallel** for the one shared file, `terminalLogic.ts` — (a) touches
`renameTerminal`, (b) touches `terminalKeyAction` — because one agent owning that
file for both edits is simpler than coordinating two.

## Steps, as approved and as run

1. **Make `dblclick` reachable.** Extend the drag bail-out in
   `onHeaderPointerDown` to `closest("button, strong, input")`, so no pointer
   capture is taken on a press over the title or the rename input. It must come
   *before* the focus call, or the mounting rename input loses focus.
2. **Defer the focus** into `setTimeout(…, 0)`, following `restore()` in the
   same file. Chosen over `preventDefault()` because preventing the default
   risks suppressing the `dblclick` step 1 exists to deliver.
3. **Add the Escape-vs-blur guard** (`abandoning` ref), mirroring the
   codebase-tab rename in `App.tsx`.
4. **Make the input controlled**, with a `draft` state and `maxLength`, mirroring
   `App.tsx` — so the cap is something the user watches stop them rather than a
   truncation applied silently afterwards.
5. **Harden `renameTerminal`** by reusing `normalizeLabel` +
   `MAX_LABEL_LENGTH` from `components/workspaceRenameLogic.ts` — control/bidi
   stripping, whitespace collapsing, a code-point-sliced 40-char cap, `null` for
   unusable. **Reuse, do not reimplement:** a second copy of that character class
   is a second place for it to drift.
6. **Discoverability:** a right-click **Rename…** on the header through the
   shared `components/ContextMenu.tsx` (`elevated`), keeping the double-click.

## Tests, written first

In `src/components/terminalLogic.test.ts`, over `renameTerminal`:

- a title with control characters is cleaned
- an over-long title truncates by **code point** (uses an astral character, so a
  UTF-16-unit implementation fails)
- a whitespace-only title is refused and leaves the list untouched
- renaming an unknown key is a no-op

## What no test can reach, and why

vitest runs in the **node environment with no DOM** and cannot load `.tsx` at
all, so `TerminalPanel.tsx` is not loadable by the suite. Every one of the
findings in `notes.md` — the pointer-capture bail-out, the focus ordering, the
Escape guard, the context menu's z-band — is therefore a **manual** check. The
tests pin the intent; the app run pins the outcome. See `todos.md`.
