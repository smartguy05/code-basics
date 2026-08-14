# Notes — diff viewer and intent labels

## Verified in the running app (2026-08-13)

Part 1 items 1–6 were driven in `pnpm tauri dev` against this repository's own
working tree and confirmed by screenshot, not by unit test:

- Both panes scroll horizontally **together** — the same column of a long line
  sits at the same offset in each, and the line-number gutters stay pinned.
- Added/removed lines are legible, and word-level marks inside a changed line
  are clearly visible (`five` → `six` on one line of `CLAUDE.md`).
- `A+` resizes every editor and the two panes stay row-aligned afterwards.
- `F7` and the toolbar arrows both step to the next change.
- The marker strip draws one mark per hunk on the right edge.
- The toolbar reads "2 differences" for a two-hunk file.

Driving the app on Windows: the dev instance builds `target/debug/cb-app.exe`,
which is a **different file** from the `target/release` binary the user usually
has open — so `pnpm tauri dev` does not hit the "Access is denied (os error 5)"
trap and does not require closing their window. Two gotchas when automating it:
`pnpm tauri dev` is killed when the background task that started it is reaped
(launch it with `nohup … &`), and `SendKeys` goes to whatever window is
foreground, so activate the window and verify `GetForegroundWindow()` first —
an `F7` sent to a stray browser opens its caret-browsing prompt instead.

Verified root causes, so nobody has to re-derive them. All four were confirmed
by reading the installed packages, not inferred.

## The diff's horizontal scrollbar exists but is unreachable

`@codemirror/merge` (6.12.2) base theme, `dist/index.js:1109-1112`:

```js
".cm-mergeView & .cm-scroller, .cm-mergeView &": {
    height: "auto !important",
    overflowY: "visible !important"
}
```

Both editors are therefore full-document height, and `.diff-host { height: 100%;
overflow: auto }` (`styles.css:740`) is what actually scrolls. Each pane's
`.cm-scroller` keeps `overflow-x: auto` from the CodeMirror view base theme — so
a native horizontal scrollbar *is* rendered, at the bottom of the whole
document, thousands of pixels below the viewport.

The library cannot be talked out of `height: auto`: it positions the revert
buttons and the alignment spacers in document coordinates (the comment at
`DiffView.tsx:147-150` already says so). Hence the sticky scrollbar of our own.

## The diff renders in light-theme colours

`EditorView.darkTheme` is set **nowhere** in `src/`. CodeMirror's `&dark`
selector only matches when a theme declares itself dark, so the merge package's
`&light` rules win against the app's dark background. Compounding it, the
changed-line backgrounds are `rgba(…, .08)` — 8% alpha (`index.js:1113-1118`).

## `@codemirror/merge` already exports more than we were using

`getChunks`, `goToNextChunk`, `goToPreviousChunk`, `mergeViewSiblings`,
`collapseUnchanged`, `DiffConfig.override`. Navigation is nonetheless derived
from our own `FileDiff` so one implementation serves both layouts and the
read-only History view — but `mergeViewSiblings` is the supported way to reach
the `a` pane, which `DiffView` does not retain today (only `merge.b`,
`DiffView.tsx:211`).

There is **no** ignore-whitespace option; `DiffConfig.override` is the only hook.

## Where the bad intent label came from

`hook.rs:355` — `parse_labels` falls back to `first_sentence(message)` when the
closing message has no `Intent:` line, and `first_sentence` takes the first
non-blank, non-`#`, non-fence line then splits on `.!?`. The only gate is
`is_usable_label` (`hook.rs:366`): length 3–120 and one alphanumeric character.

So "The workflow is running with Opus. …" became a card title verbatim.
`claude_code.rs::summarise` applies the same rule when mining transcript
history, and its own module doc (`claude_code.rs:12-22`) already admits it.

`grouping.rs:264-279` then trusts the label unconditionally — the attribution
confidence gate governs *whether* a label is shown, never *what* it says.

## A gate at ingest is not enough — it has to run on load too

Caught only by looking at the running app. After the gate was added at the
point of recording, the Changes tab still showed cards titled *"Workflow
running in the ba…"* and *"Running — 10 agents, 5 phases"*: this workspace's
`labels.jsonl` already held **261** labels written before the gate existed, and
recording only protects future turns.

So `intents::load` now re-gates inferred labels on the way out
(`hook::is_usable_inferred_label`). Declared labels are never re-judged. No
migration and no rewriting of the file — a pass over a small JSONL.

The same look also exposed a plain bug: `contains_word` matched whole words
exactly, so `agents` never matched `agent` and every "Running — N agents"
sentence sailed through. It now allows a trailing `s` and nothing longer
("agenda" and "sessionStorage" must not match).

**The lesson worth keeping: a filter that runs at write time leaves everything
already written untouched, and that history is exactly what the UI is showing.**

## Codex cannot be fixed the same way

`codex.rs:211` — `let labels = Vec::new();`. Codex history mines no labels at
all; only its Stop hook can label a turn, and nothing establishes that Codex
honours a blocking stop. Any hook-blocking work is Claude Code only.

## The Stop-hook blocking contract, verified

Checked against `https://code.claude.com/docs/en/hooks.md` rather than taken
from memory, because two secondhand accounts disagreed:

- **Exit code 2 on a `Stop` hook "prevents Claude from stopping, continues the
  conversation"**, and stderr carries the reason to the model. This is what
  `recorder.rs` does.
- `{"continue": false}` is the **opposite** of what it sounds like: it makes
  Claude *stop processing entirely*. Do not reach for it to ask for more work.
  `stopReason` goes to the user and is explicitly not shown to Claude.
- **There is no `stop_hook_active` field** in the current documentation. Any
  loop protection has to be the hook's own — which is why the turn id is
  written to `.code-basics/intents/asked.jsonl` and a turn is asked at most
  once.

## The recorder's contract is deliberate

`recorder.rs:9-11`: "It must never fail loudly. A hook that writes to stderr or
exits non-zero interrupts the agent mid-task… Every failure path returns
success." Blocking the stop breaks this **on purpose, in one case**. Every
failure path must still exit 0 silently.

Loop protection is ours (a per-turn marker file), not the platform's — sources
disagree on whether `stop_hook_active` exists, and the guard must not rest on a
field that may not be there.
