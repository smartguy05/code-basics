# Rider-grade diff viewer, and an intent label that has to earn its title

## Context

Two problems, both reported from using the app for real review work.

**The diff viewer is hard to review in.** Concretely, and all four causes are
verified rather than guessed:

- **No reachable horizontal scrollbar.** `@codemirror/merge` forces
  `height: auto !important` on both editors (`merge/dist/index.js:1109-1112`)
  and `.diff-host { height: 100%; overflow: auto }` (`styles.css:740`) does the
  vertical scrolling. Each pane's `.cm-scroller` keeps `overflow-x: auto`, so a
  horizontal scrollbar does exist — at the bottom of the *whole document*,
  thousands of pixels below the viewport. It is unreachable, which is
  indistinguishable from absent.
- **Low contrast.** `EditorView.darkTheme` is never set anywhere in `src/`, so
  the merge package's `&light` rules paint light-theme diff colours onto the
  app's dark background. On top of that its changed-line backgrounds are
  `rgba(…, .08)` — 8% alpha.
- **Fixed 12.5px text.** `styles.css:768-772` hardcodes it for every CodeMirror
  instance; there is no font-size state anywhere in the app.
- **No way to find the changes.** No change-marker strip, no next/previous
  navigation, and no keyboard handling at all in `ChangesView`.

The target is Rider's diff viewer (screenshot supplied): per-pane line-number
gutters, a change-marker strip on the outer edge, ↑/↓ jump-to-change in the
toolbar, a whitespace-ignore dropdown, and a difference counter.

**Intent card titles are unhelpful.** `hook.rs:355` — when the agent's closing
message carries no `Intent:` line, `first_sentence(last_assistant_message)`
becomes the card title verbatim, gated only by `is_usable_label`
(`hook.rs:366`: length 3–120 and at least one alphanumeric). A turn that ended
"The workflow is running with Opus. …" therefore titled a three-file card
*"The workflow is running with Opus"*. The same first-line/first-sentence rule
is used when mining transcript history (`claude_code.rs:365-378`), and its own
module doc already admits the failure mode.

Intended outcome: the diff viewer is usable for real review at a comfortable
size, and a card either carries a title the agent actually meant as a label or
says honestly that it has none.

## Part 1 — Diff viewer

### 1.1 Contrast

`DiffView.tsx` — add `EditorView.darkTheme.of(true)` to the shared extension
list (and to the merge `a` pane, which today gets no shared extensions beyond
`languageFor`/`editorColors`) so the merge package's `&dark` variants apply.
Then override the still-too-faint parts in `styles.css` beside the existing
`.cb-line-selected` rule, targeting the package's real class names:
`.cm-changedLine`, `.cm-changedText`, `.cm-insertedLine`, `.cm-deletedLine`,
`.cm-deletedChunk`, `.cm-changedLineGutter`, `.cm-deletedLineGutter`,
`.cm-collapsedLines`. Raise the line-background alpha well above `.08` and give
word-level changes a solid background rather than the 2px underline gradient.

Keep `.cb-line-selected` visually distinct from the added/removed colours — it
means "picked for staging/revert", not "changed".

### 1.2 Font size

One app-wide editor font size (diff, `FileEditor`, `DiagramEditor`), since the
user asked for it everywhere.

- A CSS custom property `--editor-font-size` on `:root`, defaulting to the
  current `12.5px`; `styles.css:771` becomes `font-size: var(--editor-font-size)`.
- A tiny `src/editorFontSize.ts` (+ `.test.ts`) owning the pure part: clamp to a
  sane range, the step function for zoom in/out, and the localStorage key
  `code-basics.editorFontSize`. Follows the existing `loadDiffLayout` precedent
  in `ChangesView.tsx:23-35`.
- Applied by setting the property on `document.documentElement` — no editor
  rebuild, so scroll position and selection survive.
- CodeMirror caches character metrics, so after a change every live view needs
  `view.requestMeasure()`. `DiffView`/`FileEditor` subscribe to the size and call
  it; **verify in the running app** that long lines re-measure and the merge
  view's alignment spacers re-flow.
- Controls: `A−` / `A+` buttons in the Changes toolbar, plus `Ctrl+=` / `Ctrl+-`
  / `Ctrl+0` registered the same way `SearchEverywhere.tsx:177-203` does it —
  window-level, capture phase, with the decision extracted into a pure function.

### 1.3 Horizontal scrolling, shared between panes

The panes cannot grow their own usable scrollbar while the merge view insists on
`height: auto`, so:

- Hide the unreachable native horizontal scrollbars (`scrollbar-width: none`
  plus the `::-webkit-scrollbar` rule) on `.diff-host .cm-scroller`.
- Render one thin horizontal scrollbar of our own, `position: sticky; bottom: 0`
  inside `.diff-host`, spanning the pane area.
- Dragging it sets `scrollLeft` on **both** panes' `.cm-scroller`, which is what
  makes the two sides stay aligned while scrolling — the explicit request.
  Shift+wheel and horizontal trackpad gestures feed the same path.
- CodeMirror's gutters are `position: sticky`, so line numbers stay pinned while
  the code scrolls — the Rider behaviour in the screenshot.
- The scroll-offset arithmetic (content width, viewport width, thumb size and
  position, clamping) goes in `src/components/diffLogic.ts` with tests; the
  component only wires it to DOM properties.

Inline layout keeps its single `.cm-scroller`; the same sticky bar drives it, so
one code path serves both.

### 1.4 Change-marker strip

A strip on the right edge of `.diff-host`, `position: sticky`, listing every
change in the file as a coloured mark at its proportional position — the "dot on
the outer edge" that makes it obvious where to scroll to. Clicking a mark scrolls
that change into view.

Everything needed is already client-side: `Hunk.newStart` / `newLines` /
`oldStart` / `oldLines` and `DiffLine.origin` (`types.ts:256-281`). Add to
`diffLogic.ts` (with tests):

```ts
export interface ChangeMark { top: number; height: number; kind: "addition" | "deletion" | "modification" }
export function changeMarks(diff: FileDiff, totalLines: number): ChangeMark[]
```

`top`/`height` as fractions of the document, so the strip needs no layout
knowledge. A hunk with both additions and deletions is a `modification`; the
colours match the line colours from 1.1.

When an intent card has scoped the pane (`shownDiff` in `ChangesView.tsx:355`),
the strip shows that card's hunks only — it must describe what is on screen.

### 1.5 Next / previous change

- `nextChangeLine(hunks, fromLine, direction)` in `diffLogic.ts`, returning the
  first line of the next/previous hunk (wrapping at the ends), with tests.
  Deriving it from `FileDiff` rather than the library's `goToNextChunk` keeps one
  implementation working identically in both layouts and in the read-only
  History view, and keeps the decision testable.
- `DiffView` exposes an imperative handle (`useImperativeHandle`) with
  `goToChange(direction)`; it dispatches a selection + `EditorView.scrollIntoView`
  on the `b` pane and mirrors the scroll on `a`.
- Toolbar ↑ / ↓ buttons, and `F7` / `Shift+F7` via the window-level capture
  listener. `ChangesView` and `HistoryView` are mounted only while their tab is
  visible (`App.tsx:287-296`), so the binding is naturally scoped.
- The binding table goes in the pure logic module beside `recogniseShortcut`'s
  precedent in `searchLogic.ts:80-109`, which also documents the keys already
  taken (Ctrl+F, Ctrl+N, Ctrl+Shift+N, Ctrl+Shift+A) — F7 collides with none.

### 1.6 Difference counter

`"N differences"` in the toolbar, from `shownDiff.hunks.length` — no new state.
It must count what the strip shows, including under whitespace-ignore (1.7).

### 1.7 Whitespace handling and collapsing unchanged lines

**Collapse unchanged** is directly supported: pass `collapseUnchanged: { margin,
minSize }` to both `MergeView` and `unifiedMergeView`. A toolbar toggle,
persisted like the layout preference. `.cm-collapsedLines` needs a dark-theme
rule (1.1).

**Whitespace-ignore** is the one genuinely awkward piece and is deliberately
**display-only**:

- `@codemirror/merge` has no ignore-whitespace option, but `DiffConfig.override`
  (`merge/dist/index.d.ts:66-70`) lets us supply the diff function. The override
  diffs whitespace-normalised copies and maps the resulting offsets back to
  original coordinates — a pure function, so it lives in `diffLogic.ts` with
  tests, including the mapping edge cases (leading indentation, trailing
  whitespace, CRLF).
- **Staging and reverting keep using the exact `FileDiff` from Rust.** The line
  indices that `git_stage_lines` / `git_revert_lines` consume must stay truthful;
  a whitespace-only hunk that the display hides is still a real change on disk.
  The toolbar must therefore say the mode is a *view* filter, and "Revert file"
  keeps reverting the whole file.
- The marker strip and the difference counter follow the display mode, filtering
  with the existing `is_formatting_only` logic's client-side equivalent so all
  three agree.

If the offset mapping proves fiddlier than it looks, ship 1.1–1.6 plus
collapse-unchanged first and land whitespace-ignore separately — it is the only
item here that can be cleanly deferred, and I will say so rather than quietly
dropping it.

### 1.8 History tab uses the same viewer

`HistoryView.tsx:200-205` currently renders `<pre>{unifiedText(diff)}</pre>`.
Switching it to `DiffView` needs both sides of each file at that commit, which no
command supplies today:

- `cb-core`: `Repo::commit_file_contents(commit_id, path) -> FileContents`,
  reading the blob from the commit's tree and from its first parent's tree
  (`None` for a file added by the commit, and for the parent side of a root
  commit). Test first, in the existing `crates/core/tests/git_operations.rs`
  harness.
- `src-tauri`: a `git_commit_file_contents` command, registered in `lib.rs` and
  wrapped in `ipc/api.ts`; `docs/reference/commands.md` updated to match.
- `HistoryView` renders `DiffView` with `editable={false}` and `layout` from the
  same persisted preference, gaining the colours, markers, F7 navigation and font
  sizing for free.
- `unifiedText` in `historyLogic.ts` stays — it is what the failure-detail pane
  uses — unless nothing else references it, in which case it and its tests go.

## Part 2 — Intent labels

Two halves: stop bad labels being recorded, and stop the ones already recorded
from titling a card as though the agent had declared them.

### 2.1 The Stop hook asks for a real intent line

The installed `Stop` entry already exists and needs no settings change
(`hooks_json.rs:80-96`, matcher `""`, timeout 5) — a re-install propagates any
command-line change through the marker-keyed replace in `merge_into`.

What changes is the recorder's contract, and this is a deliberate, narrow
exception to a rule the code states explicitly. `recorder.rs:9-11`: *"It must
never fail loudly. A hook that writes to stderr or exits non-zero interrupts the
agent mid-task… Every failure path returns success."* That stays true of every
**failure** path. Interrupting the agent becomes the intended behaviour in
exactly one case, and never in any other.

The conditions, all of which must hold before the hook pushes back:

1. Provider is Claude Code. Codex's hook system is a close relative
   (`codex.rs:1-15`) but nothing in this repo or its docs establishes that Codex
   honours a blocking stop, and Codex history mines no labels at all
   (`codex.rs:211`). Codex keeps today's fire-and-forget behaviour.
2. The workspace opted in — `is_enabled(root)` (`hook.rs:399`) already requires
   `.code-basics/intents/` to exist, so a globally installed hook stays silent in
   every repository that never enabled capture.
3. The turn actually edited files: at least one record in `edits.jsonl` carries
   this turn id. A conversational turn is never nagged.
4. `parse_labels` found no declared `Intent:` line.
5. **We have not already asked for this turn.** The guard is ours, written to
   `.code-basics/intents/asked.jsonl` keyed on turn id, not borrowed from a
   platform field. `turn_id` (`hook.rs:136-145`) is stable across the re-prompt,
   so the second Stop sees the marker and lets the turn end whatever the agent
   did. The hook therefore cannot wedge a session even if the agent never
   complies.
6. A per-workspace opt-out in `.code-basics/config.json` for anyone who finds it
   more annoying than useful.

**Mechanism — to be pinned down against the installed Claude Code before
writing it.** The two candidate contracts disagree in the sources I checked: one
describes `{"decision": "block", "reason": …}` on stdout with a `stop_hook_active`
loop-protection field; a fetch of the current hosted reference instead described
`{"continue": false, "stopReason": …, "systemMessage": …}` and no
`stop_hook_active`. **Exit code 2 with the message on stderr blocks the stop in
both accounts**, so that is the primary mechanism — it is the one that does not
depend on which reading is right. Verify the JSON shape empirically first (write
a throwaway hook, observe the behaviour) and only add it if confirmed. This is
also why condition 5 exists: the loop guard must not rest on a field that may not
be there.

`hook::ingest` returns `Result<usize>` (`hook.rs:123`) — a count, with no channel
for a verdict. It gains a return type carrying "recorded N, and here is what to
say to the agent", which `recorder.rs` turns into an exit code. Every existing
error path still exits 0 silently.

The message asks for the `Intent:` line in the form `instructions.rs:35-70`
already documents, so the wording the agent is asked for and the wording
`parse_labels` accepts stay the same thing — the round-trip that
`instructions_tests.rs:180-204` already pins.

`recorder.rs` has no tests at all today. The decision — *should this turn be
asked?* — belongs in `cb-core` beside `parse_labels` where it is testable, and
`recorder.rs` keeps only the stdin/exit-code glue.

### 2.2 A label has to look like a label

Prevention only helps the next turn. `is_usable_label` (`hook.rs:366`) and
`summarise` (`claude_code.rs:365-378`) get a real gate that rejects narration
rather than only measuring length — status and meta prose, first-person
progress announcements, and sentences about the tooling rather than the code.
Both call sites share one function so the hook and the history importer cannot
disagree.

Abstaining is the correct outcome when it fires: no label recorded.

### 2.3 The card stops claiming a stated intent it does not have

`grouping.rs:264-279` trusts `AttributedSpan::label` unconditionally. Instead:

- `IntentLabel` gains a `source` field distinguishing a **declared** `Intent:`
  line from an **inferred** first sentence. Old records with no field read as
  inferred.
- Only a declared label produces `GroupKind::Intent` and its title verbatim.
- A turn with no usable label still groups — the cross-file turn grouping is
  working and worth keeping — but is titled from the changes themselves
  (enclosing symbols, else file names) and carries a weaker kind and confidence,
  so the card says "these changed together" rather than asserting a reason.

This is the project's own abstain rule applied one level up: a wrong label is
worse than no label.

### 2.4 Contract and docs

- `IntentLabel` is not an IPC type, but `GroupKind` is. Any new variant means
  updating the key-pinning test in `model.rs` **and** `src/ipc/types.ts` together
  (`docs/architecture/ipc-contract.md`), plus `KIND_LABEL` / `KIND_TITLE` in
  `IntentPanel.tsx`.
- `.code-basics/intents/labels.jsonl` gains a field; `load()` (`mod.rs:263-268`)
  reads labels raw today and must tolerate old records without it.

## Files

**Frontend** — `src/components/DiffView.tsx` (the bulk), `src/components/diffLogic.ts`
+ `.test.ts` (marks, navigation, scroll arithmetic, whitespace mapping),
`src/views/ChangesView.tsx` (toolbar), `src/views/HistoryView.tsx`,
`src/editorFontSize.ts` + `.test.ts`, `src/styles.css`, `src/ipc/api.ts`,
`src/ipc/types.ts`.

**Rust** — `crates/core/src/intents/hook.rs` (+ `hook_tests.rs`; the gate, the
ask-once guard and the new `ingest` return type),
`crates/core/src/intents/mod.rs` (`IntentLabel::source`),
`crates/core/src/config.rs` (the opt-out),
`crates/core/src/intents/providers/claude_code.rs` (+ tests),
`crates/core/src/git/grouping.rs` (+ `grouping_tests.rs`),
`crates/core/src/git/repo.rs`, `crates/core/src/model.rs` (key-pinning test),
`src-tauri/src/recorder.rs`, `src-tauri/src/commands/git.rs`,
`src-tauri/src/lib.rs`.

**Docs** — `docs/reference/commands.md`, `docs/getting-started/using-the-app.md`,
then `pnpm docs:index` and `pnpm docs:check`.

## Verification

Tests first, per the project rule — each Rust change starts with a failing test
in the sibling `*_tests.rs`, each pure frontend helper with a failing vitest case.

- `cargo test -p cb-core` from Git Bash (`sh` must be on `PATH`).
- `pnpm test`, `pnpm coverage` (≥70% over the logic modules), `pnpm typecheck`.
- `cargo clippy`, `cargo fmt`, `pnpm docs:index`, `pnpm docs:check`.
- **In the running app** (`pnpm tauri dev`) — this is where most of Part 1 is
  actually judged, and none of it is provable by unit test:
  1. Open a file with lines wider than the pane. Drag the sticky scrollbar and
     confirm both panes move together and the line numbers stay pinned.
  2. Confirm added/removed/changed lines are legible, and that a selected line
     still reads as selected rather than as changed.
  3. `A+` / `A−` and `Ctrl+=` / `Ctrl+-`: text resizes in the diff, the file
     editor and the diagram editor, alignment stays correct, and the size
     survives a restart.
  4. `F7` / `Shift+F7` and the toolbar arrows step through every change and wrap;
     the marker strip marks each one and clicking a mark jumps to it.
  5. Toggle collapse-unchanged and whitespace-ignore; confirm the difference
     counter and the marker strip agree with what is drawn, and that staging a
     line still stages the right line under whitespace-ignore.
  6. History tab: select a commit, confirm the diff renders in the new viewer,
     read-only, with navigation and markers.
  7. Intent, in a real Claude Code session in this repository: finish a turn that
     edited files without an `Intent:` line, and confirm the hook asks for one —
     **once**. Deliberately ignore the request and confirm the turn then ends
     normally rather than looping. Confirm a conversational turn that edited
     nothing is never asked. Confirm a card with no usable label is titled from
     its changes and no longer claims a stated intent.

Ordering note: Part 2.1 changes a hook installed in the user's own
`~/.claude/settings.json`, so it is the one piece where a mistake interrupts
live agent sessions rather than just looking wrong. Build and verify it last,
behind the ask-once guard, with `CODE_BASICS_DEBUG_HOOKS` set while testing.
