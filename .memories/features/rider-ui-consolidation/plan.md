# Phase 3 — merge Run + Changes into one "Project" tab

Started 2026-09-02. The largest phase; nearly all the remaining risk sits here.

## Settled by the user, not open to revisit

- Merged tab is **"Project"** with an **icon rail** switching Files <-> Changes.
- The main area **keeps the editor**. A changed file's diff opens as **another
  editor tab**, alongside open files.
- The **toolbar follows the active editor tab**, not the rail — a file tab shows
  the Run toolbar, a diff tab the Changes toolbar. Tying it to the rail would
  show Stage buttons while the user is looking at source.

## Ground truth gathered before starting

Both views return the **same shape**, which is what makes the merge tractable:

    <>  <Sidebar …>…</Sidebar>  <div className="main"> <div className="toolbar">…  <div className="content">… </div>  </>

- `RunView.tsx` return at **1354**; its `<Sidebar>` is **small** (1356-1387:
  the unreadable-projects rows plus `<FileTree>`), its `.main` at 1434 is the
  whole editor/console split.
- `ChangesView.tsx` return at **~1082**; its `<Sidebar className="file-list">`
  is **large** (file list, intent, erosion, commit box), `.main` at 1473 holds
  the diff toolbar (1474-1592) and the diff `.content` (1595-1631).
- `Sidebar` (`src/components/Sidebar.tsx`) stores **one shared width** for every
  view under `code-basics.sidebarWidth`, so the rail must not fight it.
- `grouping === "stashes"` **returns early** and replaces the whole view with
  `<StashPanel>` — a case the merge has to keep working.

## The shortcut trap, confirmed

`view.run` / `view.changes` are generated in `shortcutLogic.ts:31` from a name
array, and `WorkspaceTab` registers a handler per **visible tab**
(`shownTabs.map(… registerCommand(\`view.${id}\`))`). Dropping `run`/`changes`
from `TABS` therefore leaves two commands advertised in Settings with **no
handler** — which the repo rule explicitly forbids. Both must be re-registered
against `project` with the rail preselected.

## Order of work

1. **Stage 1 (parallel, disjoint files)** — `editorSourceLogic` gains a `diff`
   variant; new `projectViewLogic.ts`; and an analysis-only extraction spec for
   the `DiffPane` carve, including an honest feasibility call.
2. **Stage 2 (sequential)** — the `DiffPane` carve out of `ChangesView`.
3. **Stage 3 (integration, owned by me)** — `ProjectView.tsx`, the `RunView`
   changes, `WorkspaceTab` `TABS`, `styles.css`.

## The documented fallback

If the shared-state set makes the DiffPane carve a bad trade, the plan's
fallback is the **swap-the-main-area** design: the Changes rail shows the diff
pane as it does today. Smaller carve, preserves every current behaviour. Stage 1
asks for that call explicitly rather than discovering it halfway through.

## What cannot be verified here

`.tsx` typechecks through the scratch `paths` tsconfig, so compile errors ARE
catchable. Runtime behaviour is not: no vitest, no DOM, no running app. Every
rendering claim in this phase is unverified until the user runs it.

---

## Stage 1 result (2026-09-02)

Done: `editorSourceLogic` gained the `diff` variant, `projectViewLogic.ts` was
created, and the extraction spec was produced. An adversarial pass then found
six problems in the first two, four of which are fixed below.

### The spec's verdict: carve YES, but the plan drew the line in the wrong place

It is **not** a two-way split (diff half / list half). It is a three-way split,
and the axis is **per-file vs per-view**:

- **Six pieces of state are per-file** and become per-tab inside `DiffPane`:
  `contents`, `diff`, `selectedLines`, `diffHandle`, plus `highlight` and
  `groupHunks` — the last two written by the panel today, which become *opening
  arguments* to a tab rather than shared state.
- **Ten are per-view** and stay lifted in the container; `DiffPane` reads them
  as props.

**What makes it feasible** (and this is load-bearing): the three expensive scans
— `intentGroups`, `erosion`, `coverage` — are already narrowed **by path at the
point of use**, in three separately-tested pure modules
(`uncoveredIndicesForPath`, `hunkRisk`, `confidenceForFile`), each with a doc
comment saying that is deliberate because `DiffLine.index` is per-file. So N
diff tabs share one fetch of each and every pane scopes correctly. Had those
been whole-report, the carve would have needed per-tab fetching and the
fallback would have been the better call.

### Three things the carve must not skip (from the spec's own risk note)

1. **`refreshAll` reloads exactly one file** (it closes over `selectedPath`).
   With diffs as tabs, every *other* open diff tab silently goes stale after a
   stage/revert/commit. The fix is a `reloadToken` prop every pane watches.
2. **`changes.stage` / `changes.unstage` dispatch by `document.querySelector`
   first-match** (`src/shortcuts.ts:26`). If a hidden diff tab keeps its toolbar
   mounted, the shortcut stages the **wrong file**. Consoles stay mounted when
   hidden; **toolbars must not**.
3. **`mode` is view-global but its only editor is the diff toolbar.** It drives
   all four scans and every stage/revert call, and the erosion/coverage indices
   are numbered per-mode. It must be lifted, never copied per-tab, or two tabs
   disagree and the risk washes paint on the wrong lines.

### Fixed this session

- **Two mount gates, not one.** `shouldPollChanges`/`shouldRefreshChanges` keyed
  on the rail alone. The old mount was `active && tab === "changes"` — *two*
  conditions — so a rail-only predicate left every open codebase polling git
  behind the Tests tab, and missed the two transitions that matter most
  (leaving the Project tab and returning; switching codebase). Replaced with
  `ChangesVisibility` (pane + tabForeground + codebaseActive) and an
  **edge-triggered** `shouldRefreshChanges`.
- **The cross-module join is now pinned.** `DIFF_SOURCE_KIND` was asserted only
  against itself, so renaming either side left the suite green while every diff
  tab silently got the Run toolbar. The test now asserts it against a real
  `diffFile(...)`.
- **A data-loss path closed at the type level.** `FileEditor` dispatches with
  `kind === "secrets" ? … : source.path`; a diff also has `path`, so it
  type-checked and would have **written the diff buffer over the real file** on
  Ctrl+S. New `EditableSource = Exclude<EditorSource, {kind:"diff"}>` makes that
  a compile error instead of a silent overwrite.
- **`sameWorkspaceFile`** replaces `tab.id === path`. That comparison breaks
  twice over with diffs: a diff tab on a file is not the file's tab (so opening
  the file would find the diff and stop), and a file literally named
  `diff:<mode>:<path>` collides outright.

### Deliberately not done

- `sourceEntersNavStack` still has no caller; `RunView.tsx:1308` keeps the
  hand-rolled `kind === "workspace"` check. Adopting it is an integration edit.
- The workspace id stays the **bare path**. It is compared against raw paths
  (`RunView.tsx:361`, and `activePath={activeFile}` at 1380), so namespacing it
  would break more than it fixes. `sameWorkspaceFile` removes the blast radius.

### Current state

`tsc` reports **two** errors and both are expected: the pre-existing
`vite/client` reference, and `RunView.tsx:1833` — `EditorSource` not assignable
to `EditableSource`. **That second one is the carve's seam**: it is exactly
where `DiffPane` has to branch in, surfaced by the type system rather than
found later. Logic suites: **1520 passed, 0 failed, 0 crashed**.

## Stage 2 design decision: exactly ONE DiffPane is mounted

Made before delegating the carve, because it overrides part of the spec.

**Only the diff being looked at is mounted** — not one pane per diff tab. Two of
the spec's three named risks stop existing rather than needing careful handling:

- **Stale tabs.** `refreshAll` reloads only `selectedPath`, so sibling diff tabs
  would silently show a diff that no longer matches disk. With one mount, a
  switch remounts and re-reads. The spec's `reloadToken` prop is not needed.
- **The wrong file gets staged.** `shortcuts.ts:26` falls back to
  `document.querySelector('[data-command=…]')` and takes the **first match** —
  and `querySelector` does not care about `display: none`. A second, hidden diff
  toolbar in the DOM would answer `changes.stage` for a file the user is not
  looking at. One mount makes that structurally impossible, rather than relying
  on every future edit remembering to unmount hidden toolbars.

Accepted cost, stated rather than hidden: switching away from a diff and back
re-reads that file (one IPC call) and does not preserve scroll position. Cheap,
and it buys a whole class of correctness.

**Shape:** `DiffPane` renders a *fragment* — `.toolbar` then `.content` — which
is exactly what `ChangesView`'s `.main` contains today, so a container can drop
it in unchanged. It does not render `.main` itself.

**Sequencing:** stage 2 is a pure refactor with **no behaviour change** —
`ChangesView` stays the container and renders `DiffPane` itself. That keeps the
app working and typechecking at the end of the stage, and makes the refactor
verifiable. Stage 3 then reuses `DiffPane` from `ProjectView`.

## Stage 2 result — DiffPane carved out (2026-09-02)

`ChangesView.tsx` 1631 → **1306**; new `DiffPane.tsx` (425), `diffPaneLogic.ts`
(150) + 20 tests. `ChangesView` is still the container and renders `DiffPane`
itself, so the app is unchanged and the tree still typechecks.

### Four places the spec was wrong, found by executing it

1. **`reloadToken` would have introduced a bug**, not fixed one. `loadFile`'s
   success path calls `setError(null)`, and it runs *second* inside `refreshAll`
   — after the status read, before the three scans. A token-driven reload lands
   *after* the scans, so a scan that had just set an error would have it wiped.
   Replaced with a `reloadRef` the pane publishes into, which `refreshAll`
   awaits in its original position. Ordering is load-bearing.
2. **`canRevertAll` has two readers with different gates** — the Revert button
   (`busy`-gated) and the deleted-file "Restore it" button (not gated). Folding
   them into one flag would either disable Restore during an action or enable
   Revert during one. Split into `hasRevertableChanges` (raw) and
   `canRevertAll` (gated), pinned by a test.
3. `path` must be `string | null` — the "Select a file" empty state lives in
   `.content`, which is inside the fragment.
4. §3 was self-contradictory: it had the container hoisting the mode select
   *and* the pane rendering the toolbar the select sits in.

### One real defect found by review, fixed

**`onError` carried an undocumented referential-stability contract.** It was a
dependency of `loadFile`, which is a dependency of the load effect — so a
container passing an inline arrow gets a fresh identity per render, refiring the
effect, setting state, rendering again: **an unbounded pair of git IPC calls per
frame**. Safe today only because the single caller passes a `useState` setter,
and stage 3 adds the second caller. Now held in a ref, so identity cannot
matter. `onMutated` is deliberately *not* folded in — it is only called from
event handlers, never from a dependency array.

### "No behaviour change" was not quite true — one accepted regression

Leaving the Stashes sub-view and returning now shows a **blank pane and a zeroed
toolbar for one IPC round trip**. `grouping === "stashes"` returns
`<StashPanel>` early, which unmounts `DiffPane` and discards its `contents` /
`diff`; those used to live in `ChangesView`, which stays mounted, so the diff
re-rendered synchronously.

Accepted rather than fixed, deliberately: keeping `DiffPane` mounted during
Stashes would put a **hidden `changes.stage` toolbar** back in the DOM, which is
exactly the first-match `querySelector` hazard the one-mount decision exists to
prevent. A self-correcting blank frame is the cheaper of the two.

### Verified

Typecheck: the same **two** expected errors, no third. Logic suites: **1540
passed, 0 failed, 0 crashed** (was 1520; +20 from `diffPaneLogic`).

## Stage 3 result — the Project tab exists (2026-09-02)

The first attempt died on a spend limit and the retry was stopped mid-edit,
leaving the tree **broken at 14 errors** (RunView had the new props and imports
but none of the JSX). Finished by hand from there.

### What landed

| File | Change |
|---|---|
| `src/views/useChangesModel.ts` | new (505) — the Changes container state as a hook |
| `src/views/ChangesView.tsx` | 1631 → 1020, side panel only; takes `model` |
| `src/views/RunView.tsx` | **is** the Project tab: rail, pane switch, diff tabs, `DiffPane` branch |
| `src/components/WorkspaceTab.tsx` | `TABS` = project/tests/history/architecture/inspect(+sql); owns `pane` |
| `src/shortcutLogic.ts` | `view.project` added; `run`/`changes` kept |
| `src/styles.css` | `.project-rail` |

### The main-area ordering, which is the load-bearing bit

    <div className="main">
      {isDiff ? <DiffPane …/> : <>…run toolbar…</>}
      <div className="content console-area" hidden={isDiff}>…editors…</div>
    </div>

With a diff active the DOM reads: diff toolbar, diff content, then the editor
area **hidden beside it**. Two consequences, both deliberate — editors stay
mounted and keep their state, scroll and LSP documents; and there is never a
second `.toolbar` on screen, so `changes.stage` cannot be answered by a control
the user is not looking at.

`isDiff` comes from `toolbarFor(activeTab)`, not a hand-rolled kind check, so
the toolbar shown and the pane rendered are the same decision asked once.

### Three things fixed while wiring

- **`view.run` / `view.changes` kept working.** They name tabs that no longer
  exist, but Settings advertises them and the repo rule forbids advertising a
  command with no handler. Both now select Project *and* preselect the pane that
  used to be that tab — which is why `pane` lives in `WorkspaceTab` rather than
  in `RunView`: a command handler cannot reach into a child's state.
- **The file tree is no longer asked to reveal a diff tab.** `activePath` was
  `activeFile`, and a diff id (`diff:<mode>:<path>`) names no file on disk.
- **`sourceEntersNavStack` is now a type guard.** The call site had to retype
  `kind === "workspace"` beside it just to reach `.path` — two conditions
  meaning the same thing, which is exactly how the rule drifts when a fourth
  variant is added.

### Invented CSS tokens, caught

The first rail CSS used `--bg-hover` / `--bg-active`. **Neither exists** — the
palette has `--bg`, `--bg-raised`, `--bg-inset`, `--border-strong`, `--accent`.
Replaced with real tokens, and the active state now marks itself the way
`.tabs button.active` does (accent edge + full-strength text) rather than
inventing a second visual language for the same idea.

### Verified

- Typecheck: **only** the pre-existing `vite/client` error. The deliberate
  `EditableSource` seam error is gone — the branch is wired.
- Logic suites: **1546 passed, 0 failed, 0 crashed**.
- `pnpm docs:check`: passed, 23 files.

### NOT verified, and it matters

Nothing here has been **run**. No vitest, no DOM, no app. Every claim above is
from the typechecker and the pure-logic suites. The rail, the pane switch, the
diff-as-a-tab flow and the toolbar swap are all unexercised.

## Phase 4 result — SQL floats, one terminal button, command shortcuts (2026-09-02)

`cargo test -p cb-core` 2963 → **2971**; logic suites 1546 → **1590**.

### The review's two HIGH findings were both mine

I collected the agents' CSS as structured output (to avoid the concurrent
`styles.css` writes that corrupted it in Phase 1) **and then never appended it**.
So `.sql-panel`, `.sql-pill`, `.shortcut-dot` and the menu classes had no rules
at all: the SQL window opened on top of the App-output panel, and the live
service dot was invisible. Appending both blocks fixed the second HIGH too — the
SQL overlay scoping *was* in the CSS, just not in the file.

Lesson for the pattern: returning CSS as data is right, but appending it is a
step in the task, not a formality — verify the selectors exist afterwards.

### Also fixed

- **Titlebar context menus rendered under the floating panels.** `ContextMenu`
  defaults to z-index 46, below `--z-panel` (60) and well below a raised
  terminal. Correct for a menu opened inside a view; wrong for one opened from
  the titlebar, where the menu *and its click-catching backdrop* hid behind any
  open terminal — so clicking that terminal neither closed the menu nor was
  intercepted. New `elevated` prop applies a **class**, with the band in
  `styles.css` (`calc(var(--z-overlay) - 1)`), keeping the no-integer rule.
- **Headless runs leaked their bookkeeping.** Three refs — `headlessLaunches`,
  `appPending` (up to 200 buffered events) and `appWorkspaceRoots` — were
  cleared only on a *revealed failure* or from `closeAppTab`, and a headless run
  mints no tab, so `closeAppTab` is unreachable. Every headless run that
  succeeded or was stopped leaked. Now cleared whenever the process ends.
- **`featuresLogic.test.ts` pinned a SQL tab that no longer exists.** It used a
  local fixture so it still passed — a suite quietly asserting a dead contract.
  Rewritten to pin the current strip and the fact that `FEATURE_BY_TAB` is empty
  again, pointing at `sqlPanelLogic` for where the gate went.
- `docs/INDEX.md` regenerated (the agent thought pnpm blocked it;
  `generate-index.mjs` imports only node builtins, so `node scripts/…` works).
- `commands.md` said `launch_command` "spawns it headless", which collided with
  the new `headless` flag one row below. Two different senses, now distinguished.

### Known, not fixed — judgement calls left to the user

- **A `persistent` service that exits cleanly signals nothing.** No tab, no
  Running row (it drops on exit), and the menu flips back to Run within 2s. For
  an entry the user declared a long-running service, a clean exit is the
  surprising case. Inventing a notification felt like designing rather than
  fixing.
- **`view.sql` is advertised unconditionally but registered only when the
  feature is on.** Same shape as before this phase, so not a regression.
- **A `global`-group shortcut is attributed to the foreground codebase**, so its
  failure flashes an unrelated workspace tab red.
